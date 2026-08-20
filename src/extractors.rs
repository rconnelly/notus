use std::path::Path;

use crate::hashing::{content_hash, hash8};
use crate::state::StateDb;
use crate::Result;

pub fn extract_pdf(source_id: &str, path: &Path, db: &StateDb) -> Result<Vec<String>> {
    let bytes = std::fs::read(path)?;
    let text = pdf_extract::extract_text_from_mem(&bytes)
        .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
    db.upsert_source_document(
        source_id,
        "paper",
        Some(&path.to_string_lossy()),
        path.file_stem().and_then(|s| s.to_str()),
        Some(&content_hash(&text)),
    )?;
    let mut segments = Vec::new();
    for (i, chunk) in crate::vault::chunk_text(&text, 800, 80)
        .into_iter()
        .enumerate()
    {
        let id = format!("{source_id}:page:{}:{}", i + 1, hash8(&chunk));
        db.insert_source_segment(
            &id,
            &format!("page-{}", i + 1),
            i as i64,
            source_id,
            &format!("chunk {i}"),
            &chunk,
        )?;
        segments.push(id);
    }
    Ok(segments)
}

pub fn import_source(
    config: &crate::config::Config,
    source: &Path,
    source_type: &str,
    force: bool,
) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(config.app_dir().join("sources"))?;
    std::fs::create_dir_all(config.raw_dir())?;
    let bytes = std::fs::read(source)?;
    let hash = content_hash(&String::from_utf8_lossy(&bytes));
    let id = format!("{}-{}", chrono::Local::now().format("%Y%m%d"), &hash[..8]);
    let archived = config
        .app_dir()
        .join("sources")
        .join(source.file_name().unwrap_or_default());
    if archived.exists() && !force {
        tracing::info!("already archived {}", archived.display());
    } else {
        std::fs::copy(source, &archived)?;
    }
    let db = crate::state::StateDb::open(&config.state_db_path())?;
    let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
    let body = if ext.eq_ignore_ascii_case("pdf") {
        extract_pdf(&id, source, &db)?;
        pdf_extract::extract_text_from_mem(&bytes).unwrap_or_default()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let raw = config.raw_dir().join(format!("{id}.md"));
    let mut meta = serde_yaml::Mapping::new();
    meta.insert(
        "title".into(),
        source
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    meta.insert("source_type".into(), source_type.into());
    meta.insert("origin".into(), source.display().to_string().into());
    crate::vault::write_note(&raw, &meta, &body)?;
    db.upsert_source_document(
        &id,
        source_type,
        Some(&source.to_string_lossy()),
        source.file_stem().and_then(|s| s.to_str()),
        Some(&hash),
    )?;
    Ok(raw)
}
