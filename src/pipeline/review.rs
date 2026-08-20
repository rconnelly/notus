use std::path::PathBuf;

use crate::config::Config;
use crate::vault::{mapping_get_f64, mapping_get_str, parse_note};

#[derive(Debug, Clone)]
pub struct DraftSummary {
    pub path: PathBuf,
    pub title: String,
    pub status: String,
    pub confidence: f64,
}

pub fn list_drafts(config: &Config) -> Vec<DraftSummary> {
    let mut out = Vec::new();
    if !config.drafts_dir().exists() {
        return out;
    }
    if let Ok(rd) = std::fs::read_dir(config.drafts_dir()) {
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            if let Ok((meta, _)) = parse_note(&path) {
                out.push(DraftSummary {
                    path,
                    title: mapping_get_str(&meta, "title")
                        .unwrap_or_else(|| e.file_name().to_string_lossy().into()),
                    status: mapping_get_str(&meta, "status").unwrap_or_else(|| "draft".into()),
                    confidence: mapping_get_f64(&meta, "confidence").unwrap_or(0.0),
                });
            }
        }
    }
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}
