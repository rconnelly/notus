use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::concept_text::{clean_concept_text, concept_key};
use crate::config::Config;
use crate::hashing::content_hash;
use crate::indexer::append_log;
use crate::llm::{request_structured, ModelRouter};
use crate::models::{AnalysisResult, RawNoteRecord};
use crate::paths::rel_posix;
use crate::pipeline::prompts::load_prompt;
use crate::state::StateDb;
use crate::vault::{strip_image_text_blocks, write_note};
use crate::Result;

pub const INGEST_ANALYSIS_PROMPT_VERSION: &str = "analysis-v2-language-policy";

pub fn ingest_prompt_version(config: &Config) -> String {
    let language = config.pipeline.language.as_deref().unwrap_or("auto");
    let prompt_hash = &crate::hashing::sha256_hex(load_prompt("notes").as_bytes())[..12];
    format!("{INGEST_ANALYSIS_PROMPT_VERSION}|language={language}|notes={prompt_hash}")
}

fn quality_cap(quality: &str, ceiling: u32) -> usize {
    match quality {
        "low" => 2.min(ceiling as usize),
        "medium" => 4.min(ceiling as usize),
        _ => ceiling as usize,
    }
}

pub fn ingest_note(
    path: &Path,
    config: &Config,
    router: &ModelRouter,
    db: &Arc<StateDb>,
    existing_topics: &[String],
    force: bool,
) -> Result<Option<AnalysisResult>> {
    let (_meta, body) = crate::vault::parse_note(path)?;
    let body = strip_image_text_blocks(&body);
    let hash = content_hash(&body);
    let rel = rel_posix(path, &config.vault)
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
    let prompt_ver = ingest_prompt_version(config);

    if let Some(existing) = db.get_raw(&rel)? {
        if existing.content_hash == hash
            && existing.status != "failed"
            && existing.prompt_version.as_deref() == Some(&prompt_ver)
            && !force
        {
            tracing::info!("skip unchanged {rel}");
            return Ok(None);
        }
        if existing.content_hash == hash && existing.path != rel {
            db.rekey_raw_path(&existing.path, &rel)?;
        }
    }

    db.upsert_raw(&RawNoteRecord {
        path: rel.clone(),
        content_hash: hash.clone(),
        status: "new".into(),
        summary: None,
        quality: None,
        language: None,
        prompt_version: Some(prompt_ver.clone()),
        ingested_at: None,
        compiled_at: None,
        error: None,
    })?;

    let source_type = infer_source_type(path, &_meta);
    let system = load_prompt(&source_type);
    let ceiling = config.pipeline.max_concepts_for(&source_type);
    let topics = if existing_topics.is_empty() {
        "(none yet)".into()
    } else {
        existing_topics
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };
    let prompt = format!(
        "Existing wiki topics: {topics}\n\nNote path: {rel}\n\n---\n{body}\n---\n\nExtract concepts, a short summary, quality, language, and suggested topics."
    );

    let ep = router.fast();
    let analysis: AnalysisResult = match request_structured(
        ep.client.as_ref(),
        &prompt,
        "AnalysisResult",
        &ep.model,
        system,
        ep.ctx,
        -1,
        ep.temperature,
        ep.think,
        2,
    ) {
        Ok(a) => a,
        Err(e) => {
            db.mark_raw_status(&rel, "failed", Some(&e.to_string()))?;
            tracing::error!("ingest failed for {rel}: {e}");
            return Ok(None);
        }
    };

    let cap = quality_cap(&analysis.quality, ceiling);
    let mut seen = std::collections::HashSet::new();
    let mut concepts = Vec::new();
    for c in analysis.concepts {
        let name = clean_concept_text(&c.name);
        if name.is_empty() {
            continue;
        }
        let key = concept_key(&name);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        concepts.push(crate::models::Concept {
            name,
            aliases: c
                .aliases
                .into_iter()
                .map(|a| clean_concept_text(&a))
                .filter(|a| !a.is_empty())
                .collect(),
        });
        if concepts.len() >= cap {
            break;
        }
    }
    let analysis = AnalysisResult {
        concepts: concepts.clone(),
        ..analysis
    };

    let pairs: Vec<(String, Vec<String>)> = analysis
        .concepts
        .iter()
        .map(|c| (c.name.clone(), c.aliases.clone()))
        .collect();
    db.replace_concepts_for_source(&rel, &pairs)?;

    db.upsert_raw(&RawNoteRecord {
        path: rel.clone(),
        content_hash: hash,
        status: "ingested".into(),
        summary: Some(analysis.summary.clone()),
        quality: Some(analysis.quality.clone()),
        language: analysis.language.clone(),
        prompt_version: Some(prompt_ver),
        ingested_at: Some(chrono::Local::now().to_rfc3339()),
        compiled_at: None,
        error: None,
    })?;

    write_source_page(config, &rel, &analysis)?;
    append_log(config, &format!("ingested {rel}"))?;
    Ok(Some(analysis))
}

fn infer_source_type(path: &Path, meta: &serde_yaml::Mapping) -> String {
    if let Some(t) = crate::vault::mapping_get_str(meta, "source_type") {
        return t;
    }
    if let Some(t) = crate::vault::mapping_get_str(meta, "type") {
        return t;
    }
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "pdf" => "paper".into(),
        _ => "notes".into(),
    }
}

fn write_source_page(config: &Config, rel: &str, analysis: &AnalysisResult) -> Result<()> {
    std::fs::create_dir_all(config.sources_dir())?;
    let stem = Path::new(rel)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let filename = crate::vault::sanitize_filename(&stem, 100);
    let path = config.sources_dir().join(format!("{filename}.md"));
    let mut meta = serde_yaml::Mapping::new();
    meta.insert("title".into(), format!("Source: {stem}").into());
    meta.insert(
        "tags".into(),
        serde_yaml::Value::Sequence(vec!["source".into()]),
    );
    meta.insert("status".into(), "published".into());
    meta.insert("source_file".into(), rel.into());
    meta.insert("quality".into(), analysis.quality.clone().into());
    if let Some(lang) = &analysis.language {
        meta.insert("language".into(), lang.clone().into());
    }
    let mut body = format!(
        "# Source: {stem}\n\n{}\n\n## Concepts\n\n",
        analysis.summary
    );
    for c in &analysis.concepts {
        body.push_str(&format!("- [[{}]]\n", c.name));
    }
    write_note(&path, &meta, &body)
}

pub fn ingest_all(
    config: &Config,
    router: &ModelRouter,
    db: &Arc<StateDb>,
    force: bool,
    paths: Option<&[PathBuf]>,
) -> Result<Vec<(PathBuf, Option<AnalysisResult>)>> {
    let mut topics = db.list_all_concept_names().unwrap_or_default();
    let files: Vec<PathBuf> = if let Some(p) = paths {
        p.to_vec()
    } else {
        let mut out = Vec::new();
        if config.raw_dir().exists() {
            for e in walkdir::WalkDir::new(config.raw_dir())
                .into_iter()
                .flatten()
            {
                if e.path().extension().and_then(|s| s.to_str()) == Some("md") {
                    out.push(e.path().to_path_buf());
                }
            }
        }
        out.sort();
        out
    };
    let mut results = Vec::new();
    for path in files {
        match ingest_note(&path, config, router, db, &topics, force) {
            Ok(Some(a)) => {
                for c in &a.concepts {
                    if !topics
                        .iter()
                        .any(|t| concept_key(t) == concept_key(&c.name))
                    {
                        topics.push(c.name.clone());
                    }
                }
                results.push((path, Some(a)));
            }
            Ok(None) => results.push((path, None)),
            Err(e) => {
                tracing::error!("{}: {e}", path.display());
                results.push((path, None));
            }
        }
    }
    Ok(results)
}
