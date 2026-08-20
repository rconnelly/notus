use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::Config;
use crate::hashing::content_hash;
use crate::indexer::{append_log, generate_index};
use crate::llm::{request_structured, ModelRouter};
use crate::models::{SingleArticle, WikiArticleRecord};
use crate::state::StateDb;
use crate::vault::{
    build_wiki_frontmatter, dump_note, extract_wikilinks, mapping_get_f64, mapping_get_str,
    parse_note, sanitize_filename, write_note,
};
use crate::Result;

pub fn compile_concepts(
    config: &Config,
    router: &ModelRouter,
    db: &Arc<StateDb>,
    force: bool,
    dry_run: bool,
    only: Option<&[String]>,
) -> Result<(
    Vec<PathBuf>,
    Vec<String>,
    std::collections::HashMap<String, f64>,
)> {
    let mut needed = db.concepts_needing_compile()?;
    if let Some(filter) = only {
        let keys: std::collections::HashSet<_> = filter
            .iter()
            .map(|s| crate::concept_text::concept_key(s))
            .collect();
        needed.retain(|(_, name)| keys.contains(&crate::concept_text::concept_key(name)));
    }
    let mut drafted = Vec::new();
    let mut failed = Vec::new();
    let mut confidence = std::collections::HashMap::new();
    std::fs::create_dir_all(config.drafts_dir())?;
    let run_id = db.start_compile_run(
        "concept",
        &config.model_name("fast"),
        &config.model_name("heavy"),
    )?;

    for (entity_id, name) in needed {
        if db.is_concept_blocked(&name)? {
            continue;
        }
        let sources = db.get_sources_for_entity(&entity_id)?;
        if sources.is_empty() {
            failed.push(name);
            continue;
        }
        if !force {
            let draft_path = config
                .drafts_dir()
                .join(format!("{}.md", sanitize_filename(&name, 100)));
            if draft_path.exists() {
                continue;
            }
            let published = config
                .wiki_dir()
                .join(format!("{}.md", sanitize_filename(&name, 100)));
            if published.exists() {
                if let Ok(Some(art)) =
                    db.get_article(&format!("{}.md", sanitize_filename(&name, 100)))
                {
                    if let Ok((_, body)) = parse_note(&published) {
                        if content_hash(&body) != art.content_hash {
                            tracing::info!("skip {name}: manual edit protection");
                            continue;
                        }
                    }
                }
            }
        }
        if dry_run {
            drafted.push(
                config
                    .drafts_dir()
                    .join(format!("{}.md", sanitize_filename(&name, 100))),
            );
            continue;
        }

        let mut source_blobs = String::new();
        for sp in &sources {
            let abs = config.vault.join(sp);
            if let Ok((_, body)) = parse_note(&abs) {
                source_blobs.push_str(&format!("## {sp}\n\n{body}\n\n"));
            }
        }
        let rejections = db.get_rejections(&name).unwrap_or_default();
        let rejection_block = if rejections.is_empty() {
            String::new()
        } else {
            let fb: Vec<_> = rejections.iter().map(|(f, _)| format!("- {f}")).collect();
            format!(
                "\nPrevious rejection feedback (address these):\n{}\n",
                fb.join("\n")
            )
        };
        let stub = db.has_stub(&name).unwrap_or(false);
        let prompt = if stub {
            format!("Write a short stub wiki article titled '{name}'. Keep it under 200 words. Use [[wikilinks]].\n{source_blobs}")
        } else {
            format!(
                "Write one complete wiki article titled '{name}' synthesizing the sources below.\n\
Use [[wikilinks]] to related concepts. No frontmatter. Markdown body only.{rejection_block}\n\n{source_blobs}"
            )
        };
        let ep = if stub { router.fast() } else { router.heavy() };
        let cap = if stub {
            800
        } else {
            config.pipeline.concept_soft_cap_tokens() as i64
        };
        match request_structured::<SingleArticle>(
            ep.client.as_ref(),
            &prompt,
            "SingleArticle",
            &ep.model,
            "You write concise, accurate wiki articles. Respond with JSON only.",
            ep.ctx,
            cap,
            ep.temperature,
            ep.think,
            2,
        ) {
            Ok(article) => {
                let conf = ((sources.len() as f64).min(5.0) / 5.0 * 0.6 + 0.3).min(1.0);
                let path = write_draft(
                    config, db, &entity_id, &name, &article, &sources, conf, &run_id,
                )?;
                confidence.insert(name.clone(), conf);
                drafted.push(path);
                for sp in &sources {
                    let _ =
                        db.mark_compile_state_for_entity(&entity_id, sp, &name, "compiled", None);
                    let _ = db.mark_raw_status(sp, "compiled", None);
                }
            }
            Err(e) => {
                tracing::error!("compile {name}: {e}");
                failed.push(name.clone());
                for sp in &sources {
                    let _ = db.mark_compile_state_for_entity(
                        &entity_id,
                        sp,
                        &name,
                        "failed",
                        Some(&e.to_string()),
                    );
                }
            }
        }
    }
    let _ = db.finish_compile_run(&run_id, drafted.len() as i64, 0);
    Ok((drafted, failed, confidence))
}

fn write_draft(
    config: &Config,
    db: &Arc<StateDb>,
    entity_id: &str,
    _name: &str,
    article: &SingleArticle,
    sources: &[String],
    confidence: f64,
    run_id: &str,
) -> Result<PathBuf> {
    let stem = sanitize_filename(&article.title, 100);
    let path = config.drafts_dir().join(format!("{stem}.md"));
    let mut content = article.content.clone();
    let others: Vec<String> = db.list_all_concept_names().unwrap_or_default();
    content = crate::vault::ensure_wikilinks(&content, &others);
    let meta = build_wiki_frontmatter(
        &article.title,
        &article.tags,
        sources,
        confidence,
        true,
        None,
        None,
    );
    write_note(&path, &meta, &content)?;
    let body_hash = content_hash(&content);
    let rel = format!("wiki/.drafts/{stem}.md");
    db.upsert_article(&WikiArticleRecord {
        path: rel,
        title: article.title.clone(),
        sources: sources.to_vec(),
        content_hash: body_hash,
        created_at: chrono::Local::now().to_rfc3339(),
        updated_at: chrono::Local::now().to_rfc3339(),
        status: "draft".into(),
        approved_at: None,
        approval_notes: None,
        kind: "concept".into(),
        question_hash: None,
        synthesis_sources: Vec::new(),
        synthesis_source_hashes: Vec::new(),
        article_id: None,
        last_compile_pipeline: Some(run_id.into()),
        entity_id: Some(entity_id.into()),
    })?;
    Ok(path)
}

pub fn compile_notes(
    config: &Config,
    router: &ModelRouter,
    db: &Arc<StateDb>,
    dry_run: bool,
) -> Result<(Vec<PathBuf>, Vec<String>)> {
    let (d, f, _) = compile_concepts(config, router, db, false, dry_run, None)?;
    Ok((d, f))
}

pub fn publish_drafts(
    config: &Config,
    db: &Arc<StateDb>,
    paths: Option<&[PathBuf]>,
    notes: &str,
    min_confidence: f64,
) -> Result<Vec<PathBuf>> {
    let drafts = collect_drafts(config, paths)?;
    let mut published = Vec::new();
    for draft in drafts {
        let (meta, body) = parse_note(&draft)?;
        let conf = mapping_get_f64(&meta, "confidence").unwrap_or(0.0);
        if conf < min_confidence {
            continue;
        }
        let title = mapping_get_str(&meta, "title")
            .unwrap_or_else(|| draft.file_stem().unwrap().to_string_lossy().into());
        let stem = sanitize_filename(&title, 100);
        let dest = config.wiki_dir().join(format!("{stem}.md"));
        let mut pub_meta = meta.clone();
        pub_meta.insert("status".into(), "published".into());
        write_note(&dest, &pub_meta, &body)?;
        let old_rel = format!(
            "wiki/.drafts/{}",
            draft.file_name().unwrap().to_string_lossy()
        );
        let new_rel = format!("{stem}.md");
        db.publish_article(&old_rel, &new_rel)?;
        let _ = std::fs::remove_file(&draft);
        published.push(dest);
    }
    generate_index(config, db)?;
    if config.pipeline.auto_commit {
        crate::git_ops::git_commit(&config.vault, "approve drafts", None);
    }
    let _ = notes;
    append_log(config, &format!("published {} drafts", published.len()))?;
    Ok(published)
}

pub fn verify_drafts(
    config: &Config,
    db: &Arc<StateDb>,
    paths: Option<&[PathBuf]>,
    min_confidence: f64,
) -> Result<Vec<PathBuf>> {
    let drafts = collect_drafts(config, paths)?;
    let mut out = Vec::new();
    for draft in drafts {
        let (mut meta, body) = parse_note(&draft)?;
        let conf = mapping_get_f64(&meta, "confidence").unwrap_or(0.0);
        if conf < min_confidence {
            continue;
        }
        meta.insert("status".into(), "verified".into());
        write_note(&draft, &meta, &body)?;
        let rel = format!(
            "wiki/.drafts/{}",
            draft.file_name().unwrap().to_string_lossy()
        );
        db.verify_article(&rel)?;
        out.push(draft);
    }
    Ok(out)
}

pub fn reject_draft(
    draft_path: &Path,
    config: &Config,
    db: &Arc<StateDb>,
    feedback: &str,
) -> Result<()> {
    let (meta, body) = parse_note(draft_path)?;
    let title = mapping_get_str(&meta, "title")
        .unwrap_or_else(|| draft_path.file_stem().unwrap().to_string_lossy().into());
    db.add_rejection(&title, feedback, Some(&body))?;
    let rel = format!(
        "wiki/.drafts/{}",
        draft_path.file_name().unwrap().to_string_lossy()
    );
    db.delete_article(&rel)?;
    std::fs::remove_file(draft_path)?;
    let _ = config;
    Ok(())
}

fn collect_drafts(config: &Config, paths: Option<&[PathBuf]>) -> Result<Vec<PathBuf>> {
    if let Some(p) = paths {
        return Ok(p.to_vec());
    }
    let mut out = Vec::new();
    if config.drafts_dir().exists() {
        for e in std::fs::read_dir(config.drafts_dir())? {
            let e = e?;
            if e.path().extension().and_then(|s| s.to_str()) == Some("md") {
                out.push(e.path());
            }
        }
    }
    out.sort();
    Ok(out)
}

pub fn list_draft_paths(config: &Config) -> Result<Vec<PathBuf>> {
    collect_drafts(config, None)
}

#[allow(dead_code)]
fn _unused_extract(s: &str) -> Vec<String> {
    extract_wikilinks(s)
}
#[allow(dead_code)]
fn _unused_dump(m: &serde_yaml::Mapping, b: &str) -> String {
    dump_note(m, b).unwrap_or_default()
}
