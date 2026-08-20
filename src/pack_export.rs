use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Config;
use crate::state::StateDb;
use crate::vault::atomic_write;
use crate::Result;

pub struct ExportResult {
    pub out_dir: PathBuf,
    pub n_articles: usize,
    pub capabilities: Vec<String>,
}

pub fn export_pack(config: &Config, target: &str, out: Option<PathBuf>) -> Result<ExportResult> {
    if target != "agents" {
        return Err(crate::Error::msg("only target 'agents' is supported"));
    }
    let out_dir = out.unwrap_or_else(|| config.app_dir().join("exports").join("agents"));
    std::fs::create_dir_all(out_dir.join("articles"))?;
    std::fs::create_dir_all(out_dir.join("index"))?;
    std::fs::create_dir_all(out_dir.join("agent"))?;
    let db = Arc::new(StateDb::open(&config.state_db_path())?);
    let articles = db.list_articles().unwrap_or_default();
    let published: Vec<_> = articles
        .into_iter()
        .filter(|a| a.status == "published" && a.kind == "concept")
        .collect();
    let mut n = 0;
    for a in &published {
        let src = if a.path.starts_with("wiki/") {
            config.vault.join(&a.path)
        } else {
            config.wiki_dir().join(&a.path)
        };
        if src.exists() {
            let dest = out_dir.join("articles").join(src.file_name().unwrap());
            std::fs::copy(&src, dest)?;
            n += 1;
        }
    }
    let index = crate::indexer::generate_index_json(config, &db)?;
    std::fs::copy(&index, out_dir.join("index").join("INDEX.json"))?;
    atomic_write(
        &out_dir.join("pack.toml"),
        &format!(
            "name = \"synto-pack\"\nversion = \"{}\"\ntarget = \"agents\"\n",
            crate::paths::VERSION
        ),
    )?;
    let concepts = db.list_all_concept_names().unwrap_or_default();
    atomic_write(
        &out_dir.join("agent").join("concepts.json"),
        &serde_json::to_string_pretty(&concepts)?,
    )?;
    atomic_write(
        &out_dir.join("agent").join("manifest.json"),
        &serde_json::json!({"name":"synto","articles": n}).to_string(),
    )?;
    let mut caps = vec!["articles".into(), "concepts".into()];
    if !db.list_relations().unwrap_or_default().is_empty() {
        caps.push("graph".into());
    }
    Ok(ExportResult {
        out_dir,
        n_articles: n,
        capabilities: caps,
    })
}
