use std::collections::HashSet;
use std::sync::Arc;

use crate::config::Config;
use crate::hashing::content_hash;
use crate::lock::has_invalid_lock_file;
use crate::models::{LintIssue, LintResult, ADVISORY_ISSUE_TYPES};
use crate::sanitize::sanitize_tag;
use crate::state::StateDb;
use crate::vault::{
    extract_wikilinks, is_concept_article_path, mapping_get_f64, mapping_get_str, parse_note,
    sanitize_filename,
};
use crate::Result;

pub fn run_lint(config: &Config, db: &Arc<StateDb>, _fix: bool) -> Result<LintResult> {
    let mut issues = Vec::new();
    if config.pipeline.article_max_tokens == 4096 {
        issues.push(LintIssue {
            path: "notus.toml".into(),
            issue_type: "config_outdated".into(),
            description: "article_max_tokens is pinned at the legacy 4096 default".into(),
            suggestion: "Raise pipeline.article_max_tokens (16384 is the current default)".into(),
            auto_fixable: false,
        });
    }
    if has_invalid_lock_file(&config.vault) {
        issues.push(LintIssue {
            path: ".notus/pipeline.lock".into(),
            issue_type: "stale_lock".into(),
            description: "pipeline.lock exists but does not contain a valid PID".into(),
            suggestion: "Delete .notus/pipeline.lock if no pipeline is running".into(),
            auto_fixable: true,
        });
    }

    let mut titles = HashSet::new();
    let mut inbound: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    if config.wiki_dir().exists() {
        for entry in walkdir::WalkDir::new(config.wiki_dir())
            .into_iter()
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let rel = path
                .strip_prefix(&config.vault)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if !is_concept_article_path(
                &path
                    .strip_prefix(config.wiki_dir())
                    .unwrap_or(path)
                    .to_string_lossy(),
            ) {
                continue;
            }
            let (meta, body) = match parse_note(path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let title = mapping_get_str(&meta, "title")
                .unwrap_or_else(|| path.file_stem().unwrap().to_string_lossy().into());
            titles.insert(title.clone());
            if mapping_get_str(&meta, "title").is_none()
                || mapping_get_str(&meta, "status").is_none()
            {
                issues.push(LintIssue {
                    path: rel.clone(),
                    issue_type: "missing_frontmatter".into(),
                    description: "missing required frontmatter keys".into(),
                    suggestion: "Ensure title, status, and tags are set".into(),
                    auto_fixable: true,
                });
            }
            if let Some(serde_yaml::Value::Sequence(seq)) =
                meta.get(serde_yaml::Value::String("tags".into()))
            {
                for t in seq {
                    if let Some(s) = t.as_str() {
                        if sanitize_tag(s) != s {
                            issues.push(LintIssue {
                                path: rel.clone(),
                                issue_type: "invalid_tag".into(),
                                description: format!("tag '{s}' is not sanitized"),
                                suggestion: format!("use '{}'", sanitize_tag(s)),
                                auto_fixable: true,
                            });
                        }
                    }
                }
            }
            if mapping_get_f64(&meta, "confidence").unwrap_or(1.0) < 0.3 {
                issues.push(LintIssue {
                    path: rel.clone(),
                    issue_type: "low_confidence".into(),
                    description: "confidence below 0.3".into(),
                    suggestion: "Review or recompile this article".into(),
                    auto_fixable: false,
                });
            }
            let stem = path.file_stem().unwrap().to_string_lossy();
            if stem.as_ref() != sanitize_filename(&title, 100) {
                issues.push(LintIssue {
                    path: rel.clone(),
                    issue_type: "filename_drift".into(),
                    description: format!("filename stem '{stem}' != sanitize_filename(title)"),
                    suggestion: format!("rename to {}.md", sanitize_filename(&title, 100)),
                    auto_fixable: true,
                });
            }
            if let Ok(Some(art)) = db.get_article(&format!("{stem}.md")) {
                if content_hash(&body) != art.content_hash && art.status == "published" {
                    issues.push(LintIssue {
                        path: rel.clone(),
                        issue_type: "stale".into(),
                        description: "on-disk body hash differs from DB (manual edit)".into(),
                        suggestion: "Keep the edit or recompile with --force".into(),
                        auto_fixable: false,
                    });
                }
            }
            for link in extract_wikilinks(&body) {
                *inbound.entry(link.clone()).or_default() += 1;
            }
            if crate::markdown_math::has_malformed_obsidian_math(&body) {
                issues.push(LintIssue {
                    path: rel.clone(),
                    issue_type: "malformed_latex".into(),
                    description: "LaTeX delimiters are not Obsidian-friendly".into(),
                    suggestion: "Run notus maintain --fix".into(),
                    auto_fixable: true,
                });
            }
        }
    }
    let known_stems: HashSet<String> = titles.iter().map(|t| sanitize_filename(t, 100)).collect();
    if config.wiki_dir().exists() {
        for entry in walkdir::WalkDir::new(config.wiki_dir())
            .into_iter()
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let rel = path
                .strip_prefix(&config.vault)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if let Ok((_, body)) = parse_note(path) {
                for link in extract_wikilinks(&body) {
                    let stem = sanitize_filename(&link, 100);
                    if !known_stems.contains(&stem) && !titles.contains(&link) {
                        issues.push(LintIssue {
                            path: rel.clone(),
                            issue_type: "broken_link".into(),
                            description: format!("unresolved wikilink [[{link}]]"),
                            suggestion: "Create a stub or retarget the link".into(),
                            auto_fixable: false,
                        });
                    }
                }
            }
        }
    }
    for title in &titles {
        let count = inbound.get(title).copied().unwrap_or(0)
            + inbound
                .get(&sanitize_filename(title, 100))
                .copied()
                .unwrap_or(0);
        if count == 0 {
            issues.push(LintIssue {
                path: format!("{}.md", sanitize_filename(title, 100)),
                issue_type: "orphan".into(),
                description: format!("no inbound wikilinks to {title}"),
                suggestion: "Link this concept from related articles".into(),
                auto_fixable: false,
            });
        }
    }

    let advisory = issues
        .iter()
        .filter(|i| ADVISORY_ISSUE_TYPES.contains(&i.issue_type.as_str()))
        .count();
    let structural = issues.len() - advisory;
    let health = (100.0 - structural as f64 * 2.0).clamp(0.0, 100.0);
    Ok(LintResult {
        summary: format!("{} issues ({} advisory)", issues.len(), advisory),
        health_score: health,
        advisory_issue_count: advisory,
        issues,
    })
}

pub fn partition_acked(issues: Vec<LintIssue>, ack: &[String]) -> (Vec<LintIssue>, Vec<LintIssue>) {
    let mut visible = Vec::new();
    let mut acked = Vec::new();
    for i in issues {
        let key = format!("{}:{}", i.issue_type, i.path);
        if ack.iter().any(|a| a == &i.issue_type || a == &key) {
            acked.push(i);
        } else {
            visible.push(i);
        }
    }
    (visible, acked)
}
