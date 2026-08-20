use std::sync::Arc;

use crate::config::Config;
use crate::models::LintIssue;
use crate::state::StateDb;
use crate::vault::{
    extract_wikilinks, parse_note, rename_wikilink_targets, sanitize_filename, write_note,
};
use crate::Result;

#[derive(Debug, Default)]
pub struct RenameReport {
    pub entity_id: String,
    pub old_name: String,
    pub new_name: String,
    pub files_rewritten: usize,
}

pub fn rename_concept(
    config: &Config,
    db: &Arc<StateDb>,
    old_name: &str,
    new_name: &str,
    keep_alias: bool,
    dry_run: bool,
) -> Result<RenameReport> {
    let entity_id = db
        .entity_id_for_name(old_name)?
        .ok_or_else(|| crate::Error::msg(format!("unknown concept: {old_name}")))?;
    let old_stem = sanitize_filename(old_name, 100);
    let new_stem = sanitize_filename(new_name, 100);
    let mut files = 0usize;
    if !dry_run {
        db.rename_preferred_label(&entity_id, new_name, keep_alias)?;
    }
    if config.wiki_dir().exists() {
        for entry in walkdir::WalkDir::new(config.wiki_dir())
            .into_iter()
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let Ok((meta, body)) = parse_note(path) else {
                continue;
            };
            let rewritten = rename_wikilink_targets(&body, &old_stem, &new_stem, new_name);
            if rewritten != body && !dry_run {
                write_note(path, &meta, &rewritten)?;
                files += 1;
            } else if rewritten != body {
                files += 1;
            }
        }
    }
    let old_file = config.wiki_dir().join(format!("{old_stem}.md"));
    let new_file = config.wiki_dir().join(format!("{new_stem}.md"));
    if old_file.exists() && old_stem != new_stem && !dry_run {
        std::fs::rename(&old_file, &new_file)?;
        db.publish_article(&format!("{old_stem}.md"), &format!("{new_stem}.md"))
            .ok();
    }
    crate::indexer::generate_index(config, db)?;
    Ok(RenameReport {
        entity_id,
        old_name: old_name.into(),
        new_name: new_name.into(),
        files_rewritten: files,
    })
}

pub fn merge_concepts(
    config: &Config,
    db: &Arc<StateDb>,
    loser: &str,
    winner: &str,
    dry_run: bool,
) -> Result<()> {
    let loser_id = db
        .entity_id_for_name(loser)?
        .ok_or_else(|| crate::Error::msg(format!("unknown concept: {loser}")))?;
    let winner_id = db
        .entity_id_for_name(winner)?
        .ok_or_else(|| crate::Error::msg(format!("unknown concept: {winner}")))?;
    if dry_run {
        return Ok(());
    }
    db.merge_entities(&loser_id, &winner_id)?;
    rename_concept(config, db, loser, winner, true, false)?;
    let loser_file = config
        .wiki_dir()
        .join(format!("{}.md", sanitize_filename(loser, 100)));
    if loser_file.exists() {
        let _ = std::fs::remove_file(loser_file);
    }
    crate::indexer::generate_index(config, db)?;
    Ok(())
}

pub fn create_stubs(
    config: &Config,
    db: &Arc<StateDb>,
    broken: &[LintIssue],
    max_stubs: usize,
) -> Result<Vec<std::path::PathBuf>> {
    let mut created = Vec::new();
    for issue in broken
        .iter()
        .filter(|i| i.issue_type == "broken_link")
        .take(max_stubs)
    {
        let name = issue
            .description
            .rsplit("[[")
            .next()
            .and_then(|s| s.split("]]").next())
            .unwrap_or("");
        if name.is_empty() {
            continue;
        }
        db.add_stub(name, "auto")?;
        let path = config
            .drafts_dir()
            .join(format!("{}.md", sanitize_filename(name, 100)));
        if !path.exists() {
            std::fs::create_dir_all(config.drafts_dir())?;
            let mut meta = serde_yaml::Mapping::new();
            meta.insert("title".into(), name.into());
            meta.insert("status".into(), "draft".into());
            meta.insert(
                "tags".into(),
                serde_yaml::Value::Sequence(vec!["stub".into()]),
            );
            crate::vault::write_note(
                &path,
                &meta,
                &format!("# {name}\n\n_Stub — waiting for sources._\n"),
            )?;
            created.push(path);
        }
    }
    Ok(created)
}

pub fn unlink_alias_links(
    config: &Config,
    _db: &Arc<StateDb>,
    alias: &str,
    canonical: &str,
) -> Result<usize> {
    let mut n = 0;
    if !config.wiki_dir().exists() {
        return Ok(0);
    }
    for entry in walkdir::WalkDir::new(config.wiki_dir())
        .into_iter()
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Ok((meta, body)) = parse_note(path) else {
            continue;
        };
        let rewritten = crate::vault::rename_wikilink_targets(
            &body,
            &sanitize_filename(alias, 100),
            &sanitize_filename(canonical, 100),
            canonical,
        );
        if rewritten != body {
            write_note(path, &meta, &rewritten)?;
            n += 1;
        }
    }
    Ok(n)
}

pub fn suggest_concept_merges(db: &Arc<StateDb>) -> Result<Vec<(String, String)>> {
    let names = db.list_all_concept_names()?;
    let mut out = Vec::new();
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if crate::concept_text::match_key(&names[i])
                == crate::concept_text::match_key(&names[j])
                && crate::concept_text::concept_key(&names[i])
                    != crate::concept_text::concept_key(&names[j])
            {
                out.push((names[i].clone(), names[j].clone()));
            }
        }
    }
    Ok(out)
}

#[allow(dead_code)]
fn _unused(s: &str) -> Vec<String> {
    extract_wikilinks(s)
}
