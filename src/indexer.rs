use std::sync::Arc;

use crate::config::Config;
use crate::state::StateDb;
use crate::vault::atomic_write;
use crate::Result;

pub fn generate_index(config: &Config, db: &Arc<StateDb>) -> Result<std::path::PathBuf> {
    let now = chrono::Local::now().format("%Y-%m-%d");
    let articles = db.list_articles().unwrap_or_default();
    let mut concepts = Vec::new();
    let mut synthesis = Vec::new();
    for a in &articles {
        if a.status != "published" {
            continue;
        }
        if a.kind == "synthesis" {
            synthesis.push(a);
        } else if !a.path.contains("sources/") {
            concepts.push(a);
        }
    }
    concepts.sort_by_key(|a| a.title.to_lowercase());
    let mut body = String::from("# Wiki Index\n\n## Concepts\n\n");
    for a in &concepts {
        body.push_str(&format!("- [[{}]]\n", a.title));
    }
    body.push_str("\n## Sources\n\n");
    if config.sources_dir().exists() {
        let mut pages: Vec<_> = std::fs::read_dir(config.sources_dir())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
            .collect();
        pages.sort();
        for p in pages {
            let title = crate::vault::parse_note(&p)
                .ok()
                .and_then(|(m, _)| crate::vault::mapping_get_str(&m, "title"))
                .unwrap_or_else(|| p.file_stem().unwrap().to_string_lossy().into());
            body.push_str(&format!("- [[{title}]]\n"));
        }
    }
    if !synthesis.is_empty() {
        body.push_str("\n## Synthesis\n\n");
        for a in synthesis.iter().take(25) {
            body.push_str(&format!("- [[{}]]\n", a.title));
        }
    }
    body.push_str(&format!("\n---\n_Updated {now} by notus._\n"));
    let mut meta = serde_yaml::Mapping::new();
    meta.insert("title".into(), "Index".into());
    meta.insert(
        "tags".into(),
        serde_yaml::Value::Sequence(vec!["index".into()]),
    );
    meta.insert("status".into(), "published".into());
    let path = config.wiki_dir().join("index.md");
    crate::vault::write_note(&path, &meta, &body)?;
    let _ = generate_index_json(config, db);
    Ok(path)
}

pub fn generate_index_json(config: &Config, db: &Arc<StateDb>) -> Result<std::path::PathBuf> {
    let articles = db.list_articles().unwrap_or_default();
    let names = db.list_all_concept_names().unwrap_or_default();
    let payload = serde_json::json!({
        "schema_version": 1,
        "pack": {"name": "vault", "version": "0.0.0-vault"},
        "articles": articles.iter().map(|a| serde_json::json!({
            "title": a.title,
            "path": a.path,
            "kind": a.kind,
            "status": a.status,
            "entity_id": a.entity_id,
        })).collect::<Vec<_>>(),
        "concepts": names,
        "stats": db.stats().unwrap_or(serde_json::json!({})),
    });
    let path = config.app_dir().join("INDEX.json");
    atomic_write(&path, &serde_json::to_string_pretty(&payload)?)?;
    Ok(path)
}

pub fn append_log(config: &Config, entry: &str) -> Result<std::path::PathBuf> {
    let path = config.wiki_dir().join("log.md");
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M");
    let line = format!("- {ts} — {entry}\n");
    if path.exists() {
        let mut existing = std::fs::read_to_string(&path)?;
        existing.push_str(&line);
        atomic_write(&path, &existing)?;
    } else {
        atomic_write(&path, &format!("# Vault log\n\n{line}"))?;
    }
    Ok(path)
}
