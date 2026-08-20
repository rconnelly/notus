use std::sync::Arc;

use crate::config::Config;
use crate::hashing::{content_hash, question_hash};
use crate::llm::{request_structured, ModelRouter};
use crate::models::{PageSelection, QueryAnswer, WikiArticleRecord};
use crate::state::StateDb;
use crate::vault::{parse_note, sanitize_filename, write_note};
use crate::Result;

#[derive(Debug, Clone)]
pub struct QuerySaveResult {
    pub path: std::path::PathBuf,
    pub duplicate_detected: bool,
}

#[derive(Debug, Clone)]
pub struct QueryRunResult {
    pub answer: String,
    pub title: Option<String>,
    pub selected_pages: Vec<String>,
    pub save: Option<QuerySaveResult>,
}

pub fn run_query(
    config: &Config,
    router: &ModelRouter,
    db: &Arc<StateDb>,
    question: &str,
    save: bool,
    synthesize: bool,
) -> Result<QueryRunResult> {
    let index_path = config.wiki_dir().join("index.md");
    let index = if index_path.exists() {
        std::fs::read_to_string(&index_path).unwrap_or_default()
    } else {
        String::new()
    };
    let alias_map = db.load_concept_alias_map().unwrap_or_default();
    let mut q_expanded = question.to_string();
    for (alias, canonical) in &alias_map {
        if q_expanded.to_lowercase().contains(alias) {
            q_expanded = format!("{q_expanded} (related: {canonical})");
        }
    }
    let ep = router.fast();
    let selection: PageSelection = request_structured(
        ep.client.as_ref(),
        &format!("Wiki index:\n{index}\n\nQuestion: {q_expanded}\n\nSelect up to 5 page titles that would help answer the question."),
        "PageSelection",
        &ep.model,
        "Select relevant wiki pages. JSON only.",
        ep.ctx,
        512,
        ep.temperature,
        ep.think,
        2,
    )
    .unwrap_or(PageSelection { pages: Vec::new() });

    let mut pages = selection.pages;
    pages.truncate(5);
    let mut bodies = String::new();
    for title in &pages {
        let stem = sanitize_filename(title, 100);
        let path = config.wiki_dir().join(format!("{stem}.md"));
        if let Ok((_, body)) = parse_note(&path) {
            let clipped: String = body.chars().take(8000).collect();
            bodies.push_str(&format!("## {title}\n\n{clipped}\n\n"));
        }
    }
    let heavy = router.heavy();
    let answer: QueryAnswer = request_structured(
        heavy.client.as_ref(),
        &format!("Question: {question}\n\nSource pages:\n{bodies}\n\nAnswer using only these pages. Use [[wikilinks]]."),
        "QueryAnswer",
        &heavy.model,
        "Answer from the provided wiki pages. JSON only.",
        heavy.ctx,
        2048,
        heavy.temperature,
        heavy.think,
        2,
    )?;

    let mut save_res = None;
    if save {
        std::fs::create_dir_all(config.queries_dir())?;
        let date = chrono::Local::now().format("%Y-%m-%d");
        let stem = sanitize_filename(
            &answer
                .title
                .clone()
                .unwrap_or_else(|| question.chars().take(40).collect()),
            80,
        );
        let path = config.queries_dir().join(format!("{date}-{stem}.md"));
        let mut meta = serde_yaml::Mapping::new();
        meta.insert("title".into(), question.into());
        meta.insert(
            "tags".into(),
            serde_yaml::Value::Sequence(vec!["query".into()]),
        );
        meta.insert("status".into(), "published".into());
        write_note(
            &path,
            &meta,
            &format!("# {question}\n\n{}\n", answer.answer),
        )?;
        save_res = Some(QuerySaveResult {
            path,
            duplicate_detected: false,
        });
    }
    if synthesize {
        let qh = question_hash(question);
        if let Some(existing) = db.find_synthesis_by_question_hash(&qh)? {
            tracing::info!("synthesis already exists at {}", existing.path);
        } else {
            std::fs::create_dir_all(config.synthesis_dir())?;
            let title = answer
                .title
                .clone()
                .unwrap_or_else(|| question.chars().take(60).collect::<String>());
            let stem = sanitize_filename(&title, 100);
            let dest = crate::vault::next_available_path(
                &config.synthesis_dir().join(format!("{stem}.md")),
                None,
            );
            let mut meta = serde_yaml::Mapping::new();
            meta.insert("title".into(), title.clone().into());
            meta.insert(
                "tags".into(),
                serde_yaml::Value::Sequence(vec!["synthesis".into()]),
            );
            meta.insert("status".into(), "published".into());
            let body = answer.answer.clone();
            write_note(&dest, &meta, &body)?;
            let rel = format!(
                "wiki/synthesis/{}",
                dest.file_name().unwrap().to_string_lossy()
            );
            db.insert_synthesis_atomic(&WikiArticleRecord {
                path: rel,
                title,
                sources: pages.clone(),
                content_hash: content_hash(&body),
                created_at: chrono::Local::now().to_rfc3339(),
                updated_at: chrono::Local::now().to_rfc3339(),
                status: "published".into(),
                approved_at: Some(chrono::Local::now().to_rfc3339()),
                approval_notes: None,
                kind: "synthesis".into(),
                question_hash: Some(qh),
                synthesis_sources: pages.clone(),
                synthesis_source_hashes: Vec::new(),
                article_id: None,
                last_compile_pipeline: None,
                entity_id: None,
            })?;
        }
    }
    Ok(QueryRunResult {
        answer: answer.answer,
        title: answer.title,
        selected_pages: pages,
        save: save_res,
    })
}
