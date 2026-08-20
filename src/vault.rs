use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use regex::Regex;
use serde_yaml::Value;

use crate::markdown_math::{mask_markdown_regions, restore_markdown_regions, MaskOptions};
use crate::sanitize::sanitize_tags;
use crate::Result;

static PICTURE_TEXT_BLOCK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)\*\*-+\s*Start of picture text\s*-+\*\*.*?\*\*-+\s*End of picture text\s*-+\*\*(?:<br>)?")
        .unwrap()
});
static OMITTED_PICTURE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\*\*==>\s*picture\b[^\n]*?intentionally omitted\s*<==\*\*").unwrap()
});
static WIKILINK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\[\[([^\]|#]+)(?:[|#][^\]]*)?\]\]").unwrap());
static WIKILINK_FULL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\[\[([^\]|#]+?)(?:#([^\]|]*))?(?:\|([^\]]*))?\]\]").unwrap());
static FORBIDDEN_CHARS: Lazy<Regex> = Lazy::new(|| Regex::new(r#"[*"\\/<>:|?#^\[\]]"#).unwrap());
static CONTROL_CHARS: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\x00-\x1f\x7f]").unwrap());

const MEDIA_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".bmp", ".tiff", ".avif", ".mp4", ".webm",
    ".ogv", ".mov", ".mkv", ".avi", ".mp3", ".wav", ".ogg", ".flac", ".m4a", ".pdf", ".csv",
    ".xlsx", ".docx",
];

fn windows_reserved_stems() -> HashSet<String> {
    let mut set = HashSet::new();
    for s in ["con", "prn", "aux", "nul"] {
        set.insert(s.to_string());
    }
    for i in 1..10 {
        set.insert(format!("com{i}"));
        set.insert(format!("lpt{i}"));
    }
    set
}

pub fn parse_note(path: &Path) -> Result<(serde_yaml::Mapping, String)> {
    let text = fs::read_to_string(path)?;
    parse_note_text(&text)
}

pub fn parse_note_text(text: &str) -> Result<(serde_yaml::Mapping, String)> {
    let text = text.replace("\r\n", "\n");
    if let Some(rest) = text.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let yaml = &rest[..end];
            let body = rest[end + 4..]
                .strip_prefix('\n')
                .unwrap_or(&rest[end + 4..]);
            let value: Value = if yaml.trim().is_empty() {
                Value::Mapping(serde_yaml::Mapping::new())
            } else {
                serde_yaml::from_str(yaml).unwrap_or(Value::Mapping(serde_yaml::Mapping::new()))
            };
            let map = match value {
                Value::Mapping(m) => m,
                _ => serde_yaml::Mapping::new(),
            };
            return Ok((map, body.to_string()));
        }
    }
    Ok((serde_yaml::Mapping::new(), text))
}

pub fn dump_note(metadata: &serde_yaml::Mapping, body: &str) -> Result<String> {
    if metadata.is_empty() {
        return Ok(body.to_string());
    }
    let yaml = serde_yaml::to_string(&Value::Mapping(metadata.clone()))?;
    let yaml = yaml.trim_end();
    Ok(format!(
        "---\n{yaml}\n---\n\n{}",
        body.trim_start_matches('\n')
    ))
}

pub fn write_note(path: &Path, metadata: &serde_yaml::Mapping, body: &str) -> Result<()> {
    atomic_write(path, &dump_note(metadata, body)?)
}

pub fn update_frontmatter(path: &Path, updates: &serde_yaml::Mapping) -> Result<()> {
    let (mut meta, body) = parse_note(path)?;
    for (k, v) in updates {
        meta.insert(k.clone(), v.clone());
    }
    write_note(path, &meta, &body)
}

pub fn strip_image_text_blocks(body: &str) -> String {
    let body = PICTURE_TEXT_BLOCK_RE.replace_all(body, "");
    OMITTED_PICTURE_RE.replace_all(&body, "").into_owned()
}

pub fn extract_wikilinks(content: &str) -> Vec<String> {
    let (masked, _) = mask_markdown_regions(
        content,
        MaskOptions {
            mask_wikilinks: false,
            mask_embeds: false,
            mask_links: true,
        },
    );
    WIKILINK_RE
        .captures_iter(&masked)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .filter(|t| {
            let lower = t.to_lowercase();
            !MEDIA_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
        })
        .collect()
}

pub fn sanitize_filename(title: &str, max_len: usize) -> String {
    let mut name = FORBIDDEN_CHARS.replace_all(title, "").into_owned();
    name = CONTROL_CHARS.replace_all(&name, "").trim().to_string();
    if name.chars().count() > max_len {
        let truncated: String = name.chars().take(max_len).collect();
        name = truncated
            .rsplit_once(' ')
            .map(|(a, _)| a.to_string())
            .unwrap_or(truncated);
    }
    name = name.trim_end_matches(['.', ' ']).to_string();
    if windows_reserved_stems().contains(&name.to_lowercase()) {
        name.push('_');
    }
    if name.is_empty() {
        "untitled".into()
    } else {
        name
    }
}

pub fn sanitize_wikilink_target(name: &str) -> String {
    sanitize_filename(name, 100)
}

pub fn next_available_path(path: &Path, reserved_names: Option<&[String]>) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let suffix = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut existing_lower: HashSet<String> = HashSet::new();
    if parent.exists() {
        if let Ok(rd) = fs::read_dir(parent) {
            for e in rd.flatten() {
                existing_lower.insert(e.file_name().to_string_lossy().to_lowercase());
            }
        }
    }
    if let Some(names) = reserved_names {
        for n in names {
            existing_lower.insert(n.to_lowercase());
        }
    }
    let mut candidate = path.to_path_buf();
    let mut n = 2;
    while existing_lower.contains(
        &candidate
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase(),
    ) {
        candidate = parent.join(format!("{stem}-{n}{suffix}"));
        n += 1;
    }
    candidate
}

pub fn atomic_write(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(content.as_bytes())?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn is_concept_article_path(relative_path: &str) -> bool {
    let parts: Vec<&str> = Path::new(relative_path)
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    if parts.contains(&".drafts") {
        return false;
    }
    let lowered: HashSet<String> = parts.iter().map(|p| p.to_lowercase()).collect();
    if lowered.contains("sources") || lowered.contains("queries") || lowered.contains("synthesis") {
        return false;
    }
    let name = Path::new(relative_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    !matches!(name.as_str(), "index.md" | "log.md")
}

pub fn is_synthesis_article_path(relative_path: &str) -> bool {
    let parts: Vec<&str> = Path::new(relative_path)
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    if parts.contains(&".drafts") {
        return false;
    }
    let lowered: HashSet<String> = parts.iter().map(|p| p.to_lowercase()).collect();
    if !lowered.contains("synthesis") {
        return false;
    }
    let name = Path::new(relative_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    !matches!(name.as_str(), "index.md" | "log.md")
}

pub fn list_wiki_articles(wiki_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut articles = Vec::new();
    for entry in walkdir::WalkDir::new(wiki_dir).into_iter().flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Ok(rel) = path.strip_prefix(wiki_dir) else {
            continue;
        };
        if !is_concept_article_path(&rel.to_string_lossy().replace('\\', "/")) {
            continue;
        }
        let title = parse_note(path)
            .ok()
            .and_then(|(m, _)| {
                m.get(Value::String("title".into()))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| path.file_stem().unwrap().to_string_lossy().into());
        articles.push((title, path.to_path_buf()));
    }
    articles
}

pub fn ensure_wikilinks(content: &str, targets: &[String]) -> String {
    if targets.is_empty() {
        return content.to_string();
    }
    let (mut masked, spans) = mask_markdown_regions(
        content,
        MaskOptions {
            mask_wikilinks: false,
            mask_embeds: true,
            mask_links: true,
        },
    );
    for target in targets {
        let safe_target = sanitize_wikilink_target(target);
        if masked.contains(&format!("[[{target}]]"))
            || masked.contains(&format!("[[{target}|"))
            || masked.contains(&format!("[[{safe_target}]]"))
            || masked.contains(&format!("[[{safe_target}|"))
        {
            continue;
        }
        let pattern = Regex::new(&format!(r"\b{}\b", regex::escape(target))).ok();
        if let Some(re) = pattern {
            let repl = if safe_target != *target {
                format!("[[{safe_target}|{target}]]")
            } else {
                format!("[[{safe_target}]]")
            };
            if let Some(m) = re.find(&masked) {
                let start = m.start();
                let already = start >= 2
                    && masked.as_bytes()[start - 2] == b'['
                    && masked.as_bytes()[start - 1] == b'[';
                if !already {
                    masked = format!("{}{}{}", &masked[..m.start()], repl, &masked[m.end()..]);
                }
            }
        }
    }
    restore_markdown_regions(masked, &spans)
}

pub fn generate_aliases(title: &str, source_text: &str) -> Vec<String> {
    let mut aliases = HashSet::new();
    let lower = title.to_lowercase();
    if lower != title {
        aliases.insert(lower);
    }
    let pattern = Regex::new(&format!(r"{}\s*\(([A-Z]{{2,}})\)", regex::escape(title))).unwrap();
    for caps in pattern.captures_iter(source_text) {
        aliases.insert(caps.get(1).unwrap().as_str().to_string());
    }
    let mut out: Vec<_> = aliases.into_iter().collect();
    out.sort();
    out
}

fn split_on_h2(text: &str) -> Vec<&str> {
    let mut sections = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if bytes[i] == b'\n' && bytes[i + 1] == b'#' && bytes[i + 2] == b'#' && bytes[i + 3] == b' '
        {
            sections.push(&text[start..i]);
            start = i + 1;
            i += 4;
        } else {
            i += 1;
        }
    }
    sections.push(&text[start..]);
    sections
}

pub fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    let sections = split_on_h2(text);
    let mut chunks = Vec::new();
    for section in sections {
        let words: Vec<&str> = section.split_whitespace().collect();
        if words.len() <= chunk_size {
            if !section.trim().is_empty() {
                chunks.push(section.trim().to_string());
            }
        } else {
            let step = chunk_size.saturating_sub(overlap).max(1);
            let mut i = 0;
            while i < words.len() {
                let end = (i + chunk_size).min(words.len());
                let chunk = words[i..end].join(" ");
                if !chunk.trim().is_empty() {
                    chunks.push(chunk.trim().to_string());
                }
                if end == words.len() {
                    break;
                }
                i += step;
            }
        }
    }
    if chunks.is_empty() {
        vec![text.trim().to_string()]
    } else {
        chunks
    }
}

pub fn build_wiki_frontmatter(
    title: &str,
    tags: &[String],
    sources: &[String],
    confidence: f64,
    is_draft: bool,
    existing_meta: Option<&serde_yaml::Mapping>,
    aliases: Option<&[String]>,
) -> serde_yaml::Mapping {
    let now = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut sanitized = sanitize_tags(tags);
    if sanitized.is_empty() {
        if let Some(existing) = existing_meta {
            if let Some(Value::Sequence(seq)) = existing.get(Value::String("tags".into())) {
                let raw: Vec<String> = seq
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                sanitized = sanitize_tags(&raw);
            }
        }
    }
    let mut meta = serde_yaml::Mapping::new();
    meta.insert("title".into(), title.into());
    meta.insert(
        "tags".into(),
        Value::Sequence(sanitized.into_iter().map(Value::String).collect()),
    );
    meta.insert(
        "sources".into(),
        Value::Sequence(sources.iter().cloned().map(Value::String).collect()),
    );
    meta.insert(
        "confidence".into(),
        Value::Number(serde_yaml::Number::from(
            (confidence * 100.0).round() / 100.0,
        )),
    );
    meta.insert(
        "status".into(),
        Value::String(if is_draft { "draft" } else { "published" }.into()),
    );
    meta.insert("updated".into(), now.clone().into());
    if let Some(aliases) = aliases {
        if !aliases.is_empty() {
            meta.insert(
                "aliases".into(),
                Value::Sequence(aliases.iter().cloned().map(Value::String).collect()),
            );
        }
    }
    if let Some(existing) = existing_meta {
        if let Some(created) = existing.get(Value::String("created".into())) {
            meta.insert("created".into(), created.clone());
        } else {
            meta.insert("created".into(), now.into());
        }
    } else {
        meta.insert("created".into(), now.into());
    }
    meta
}

pub fn normalize_wikilinks(
    body: &str,
    alias_map: &std::collections::HashMap<String, String>,
    known_titles: &HashSet<String>,
) -> String {
    let (masked, spans) = mask_markdown_regions(
        body,
        MaskOptions {
            mask_wikilinks: false,
            mask_embeds: true,
            mask_links: true,
        },
    );
    let known_lower: HashSet<String> = known_titles.iter().map(|t| t.to_lowercase()).collect();
    let rewritten = WIKILINK_FULL_RE.replace_all(&masked, |caps: &regex::Captures| {
        let target = caps.get(1).unwrap().as_str().trim();
        if known_lower.contains(&target.to_lowercase()) {
            return caps.get(0).unwrap().as_str().to_string();
        }
        let Some(canonical) = alias_map.get(&target.to_lowercase()) else {
            return caps.get(0).unwrap().as_str().to_string();
        };
        let fragment = caps.get(2).map(|m| m.as_str());
        let display = caps.get(3).map(|m| m.as_str());
        let effective_display = display.unwrap_or(target);
        let frag_part = fragment.map(|f| format!("#{f}")).unwrap_or_default();
        format!("[[{canonical}{frag_part}|{effective_display}]]")
    });
    restore_markdown_regions(rewritten.into_owned(), &spans)
}

pub fn rename_wikilink_targets(
    body: &str,
    old_stem: &str,
    new_stem: &str,
    new_name: &str,
) -> String {
    let (masked, spans) = mask_markdown_regions(
        body,
        MaskOptions {
            mask_wikilinks: false,
            mask_embeds: true,
            mask_links: true,
        },
    );
    let old_key = old_stem.to_lowercase();
    let rewritten = WIKILINK_FULL_RE.replace_all(&masked, |caps: &regex::Captures| {
        let target = caps.get(1).unwrap().as_str().trim();
        if target.to_lowercase() != old_key
            && sanitize_filename(target, 100).to_lowercase() != old_key
        {
            return caps.get(0).unwrap().as_str().to_string();
        }
        let fragment = caps.get(2).map(|m| m.as_str());
        let display = caps.get(3).map(|m| m.as_str());
        let frag_part = fragment.map(|f| format!("#{f}")).unwrap_or_default();
        let echoes_old = display.is_some_and(|d| {
            d.to_lowercase() == old_key || sanitize_filename(d, 100).to_lowercase() == old_key
        });
        if let Some(d) = display.filter(|_| !echoes_old) {
            format!("[[{new_stem}{frag_part}|{d}]]")
        } else if new_stem == new_name {
            format!("[[{new_stem}{frag_part}]]")
        } else {
            format!("[[{new_stem}{frag_part}|{new_name}]]")
        }
    });
    restore_markdown_regions(rewritten.into_owned(), &spans)
}

pub fn mapping_get_str(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    map.get(Value::String(key.into()))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

pub fn mapping_get_f64(map: &serde_yaml::Mapping, key: &str) -> Option<f64> {
    map.get(Value::String(key.into())).and_then(|v| match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    })
}

pub fn mapping_get_seq_str(map: &serde_yaml::Mapping, key: &str) -> Vec<String> {
    match map.get(Value::String(key.into())) {
        Some(Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_strips_slash() {
        assert_eq!(sanitize_filename("TCP/IP", 100), "TCPIP");
    }

    #[test]
    fn filename_reserved() {
        assert_eq!(sanitize_filename("NUL", 100), "NUL_");
    }

    #[test]
    fn concept_path_filters() {
        assert!(is_concept_article_path("Qubit.md"));
        assert!(!is_concept_article_path("sources/foo.md"));
        assert!(!is_concept_article_path("index.md"));
        assert!(is_synthesis_article_path("synthesis/Foo.md"));
    }

    #[test]
    fn extract_links() {
        let links = extract_wikilinks("see [[Qubit]] and ![[img.png]]");
        assert_eq!(links, vec!["Qubit"]);
    }
}
