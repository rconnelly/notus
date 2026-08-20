use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::Value as Json;

use crate::concept_text::{concept_key, match_key};
use crate::hashing::{content_hash, generate_article_id, relation_id};
use crate::models::{ItemMentionRecord, KnowledgeItemRecord, RawNoteRecord, WikiArticleRecord};
use crate::paths::to_posix;
use crate::{Error, Result};

pub const CURRENT_SCHEMA_VERSION: i64 = 29;
pub const CHECKPOINT_SCHEMA_VERSION: i64 = 2;
pub const REJECTION_CAP: i64 = 5;

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, Clone, Default)]
pub struct ResolveResult {
    pub ids: Vec<String>,
    pub ambiguous: bool,
}

pub struct StateDb {
    conn: Mutex<Connection>,
    pub path: PathBuf,
}

impl StateDb {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        conn.execute_batch(SCHEMA)?;
        let db = Self {
            conn: Mutex::new(conn),
            path: db_path.to_path_buf(),
        };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_readonly(db_path: &Path) -> Result<Self> {
        if !db_path.exists() {
            return Err(Error::msg(format!(
                "database not found: {}",
                db_path.display()
            )));
        }
        let conn = Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: db_path.to_path_buf(),
        })
    }

    pub fn schema_version(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let v: Option<i64> = conn
            .query_row(
                "SELECT version FROM schema_version ORDER BY rowid DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.unwrap_or(0))
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let current: Option<i64> = conn
            .query_row(
                "SELECT version FROM schema_version ORDER BY rowid DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let current = current.unwrap_or(0);
        if current > CURRENT_SCHEMA_VERSION {
            return Err(Error::msg(format!(
                "On-disk DB schema_version={current} is newer than this notus binary (supports v{CURRENT_SCHEMA_VERSION}). Upgrade notus."
            )));
        }
        drop(conn);
        self.ensure_compile_state_entity_id()?;
        self.ensure_relation_keys()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (id, version) VALUES (1, ?1)",
            params![CURRENT_SCHEMA_VERSION],
        )?;
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_concepts_entity ON concepts(entity_id)",
            [],
        );
        let _ = conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_wiki_articles_qid ON wiki_articles(question_hash) WHERE question_hash IS NOT NULL",
            [],
        );
        let _ = conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_wiki_articles_article_id ON wiki_articles(article_id) WHERE article_id IS NOT NULL",
            [],
        );
        Ok(())
    }

    fn ensure_compile_state_entity_id(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(concept_compile_state)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        if cols.iter().any(|c| c == "entity_id") {
            return Ok(());
        }
        conn.execute_batch(
            r#"
            CREATE TABLE concept_compile_state_new (
                entity_id    TEXT NOT NULL,
                source_path  TEXT NOT NULL,
                concept_name TEXT NOT NULL,
                status       TEXT NOT NULL DEFAULT 'pending',
                error        TEXT,
                compiled_at  TEXT,
                updated_at   TEXT NOT NULL,
                PRIMARY KEY (entity_id, source_path),
                CHECK (status IN ('pending', 'failed', 'compiled', 'deferred_draft', 'deferred_manual_edit'))
            );
            INSERT OR IGNORE INTO concept_compile_state_new
                (entity_id, source_path, concept_name, status, error, compiled_at, updated_at)
            SELECT concept_name, source_path, concept_name, status, error, compiled_at, updated_at
            FROM concept_compile_state;
            DROP TABLE concept_compile_state;
            ALTER TABLE concept_compile_state_new RENAME TO concept_compile_state;
            CREATE INDEX IF NOT EXISTS idx_concept_compile_status ON concept_compile_state(status, source_path);
            CREATE INDEX IF NOT EXISTS idx_concept_compile_name ON concept_compile_state(lower(concept_name));
            CREATE INDEX IF NOT EXISTS idx_concept_compile_entity ON concept_compile_state(entity_id);
            "#,
        )?;
        Ok(())
    }

    fn ensure_relation_keys(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(relations)")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        if !cols.iter().any(|c| c == "subject_key") {
            let _ = conn.execute(
                "ALTER TABLE relations ADD COLUMN subject_key TEXT NOT NULL DEFAULT ''",
                [],
            );
            let _ = conn.execute(
                "ALTER TABLE relations ADD COLUMN object_key TEXT NOT NULL DEFAULT ''",
                [],
            );
        }
        Ok(())
    }

    fn now() -> String {
        chrono::Local::now().to_rfc3339()
    }

    pub fn upsert_raw(&self, rec: &RawNoteRecord) -> Result<()> {
        let path = to_posix(&rec.path);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO raw_notes (path, content_hash, status, summary, quality, language, ingested_at, compiled_at, error, prompt_version)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(path) DO UPDATE SET
               content_hash=excluded.content_hash, status=excluded.status, summary=excluded.summary,
               quality=excluded.quality, language=excluded.language, ingested_at=excluded.ingested_at,
               compiled_at=excluded.compiled_at, error=excluded.error, prompt_version=excluded.prompt_version",
            params![
                path, rec.content_hash, rec.status, rec.summary, rec.quality, rec.language,
                rec.ingested_at, rec.compiled_at, rec.error, rec.prompt_version
            ],
        )?;
        Ok(())
    }

    pub fn get_raw(&self, path: &str) -> Result<Option<RawNoteRecord>> {
        let path = to_posix(path);
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT path, content_hash, status, summary, quality, language, prompt_version, ingested_at, compiled_at, error FROM raw_notes WHERE path = ?1",
            params![path],
            row_to_raw,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn get_raw_by_hash(&self, hash: &str) -> Result<Option<RawNoteRecord>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT path, content_hash, status, summary, quality, language, prompt_version, ingested_at, compiled_at, error FROM raw_notes WHERE content_hash = ?1 LIMIT 1",
            params![hash],
            row_to_raw,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_raw(&self) -> Result<Vec<RawNoteRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT path, content_hash, status, summary, quality, language, prompt_version, ingested_at, compiled_at, error FROM raw_notes ORDER BY path",
        )?;
        let rows = stmt.query_map([], row_to_raw)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn mark_raw_status(&self, path: &str, status: &str, error: Option<&str>) -> Result<()> {
        let path = to_posix(path);
        let conn = self.conn.lock().unwrap();
        if status == "compiled" {
            conn.execute(
                "UPDATE raw_notes SET status=?1, compiled_at=?2, error=?3 WHERE path=?4",
                params![status, Self::now(), error, path],
            )?;
        } else {
            conn.execute(
                "UPDATE raw_notes SET status=?1, error=?2 WHERE path=?3",
                params![status, error, path],
            )?;
        }
        Ok(())
    }

    pub fn rekey_raw_path(&self, old_path: &str, new_path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE raw_notes SET path=?1 WHERE path=?2",
            params![to_posix(new_path), to_posix(old_path)],
        )?;
        Ok(())
    }

    pub fn list_note_languages(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT DISTINCT language FROM raw_notes WHERE language IS NOT NULL")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn ensure_entity(&self, preferred_label: &str, source: &str) -> Result<Option<String>> {
        let label = preferred_label.trim();
        if label.is_empty() {
            return Ok(None);
        }
        let lk = concept_key(label);
        if lk.is_empty() {
            return Ok(None);
        }
        if let Some(id) = self.entity_id_for_name(label)? {
            return Ok(Some(id));
        }
        let id = generate_article_id();
        let now = Self::now();
        let mk = match_key(label);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO concept_entities (id, kind, status, created_at, updated_at) VALUES (?1,'concept','active',?2,?2)",
            params![id, now],
        )?;
        conn.execute(
            "INSERT INTO concept_labels (entity_id, label, label_key, match_key, role, source, created_at)
             VALUES (?1,?2,?3,?4,'preferred',?5,?6)",
            params![id, label, lk, mk, source, now],
        )?;
        Ok(Some(id))
    }

    pub fn entity_id_for_name(&self, name: &str) -> Result<Option<String>> {
        let lk = concept_key(name);
        let conn = self.conn.lock().unwrap();
        let id: Option<String> = conn
            .query_row(
                "SELECT entity_id FROM concept_labels WHERE label_key=?1 AND role='preferred' LIMIT 1",
                params![lk],
                |r| r.get(0),
            )
            .optional()?;
        if id.is_some() {
            return Ok(id);
        }
        conn.query_row(
            "SELECT entity_id FROM concept_labels WHERE label_key=?1 LIMIT 1",
            params![lk],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn preferred_label_for_entity(&self, entity_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT label FROM concept_labels WHERE entity_id=?1 AND role='preferred' LIMIT 1",
            params![entity_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn resolve_label(&self, label: &str) -> Result<ResolveResult> {
        let lk = concept_key(label);
        let mk = match_key(label);
        let conn = self.conn.lock().unwrap();
        let mut ids = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT entity_id FROM concept_labels cl
             JOIN concept_entities ce ON ce.id = cl.entity_id
             WHERE ce.status='active' AND (cl.label_key=?1 OR cl.match_key=?2)",
        )?;
        let rows = stmt.query_map(params![lk, mk], |r| r.get::<_, String>(0))?;
        for id in rows.flatten() {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let ambiguous = ids.len() > 1;
        Ok(ResolveResult { ids, ambiguous })
    }

    pub fn list_all_concept_names(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT label FROM concept_labels WHERE role='preferred' ORDER BY label")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn replace_concepts_for_source(
        &self,
        source_path: &str,
        names: &[(String, Vec<String>)],
    ) -> Result<Vec<String>> {
        let source_path = to_posix(source_path);
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "DELETE FROM concepts WHERE source_path=?1",
                params![source_path],
            )?;
        }
        let mut entity_ids = Vec::new();
        for (name, aliases) in names {
            if let Some(id) = self.ensure_entity(name, "extracted")? {
                let conn = self.conn.lock().unwrap();
                conn.execute(
                    "INSERT OR REPLACE INTO concepts (entity_id, source_path, name) VALUES (?1,?2,?3)",
                    params![id, source_path, name],
                )?;
                drop(conn);
                self.upsert_aliases(&id, aliases, "extracted")?;
                entity_ids.push(id);
            }
        }
        Ok(entity_ids)
    }

    pub fn upsert_aliases(&self, entity_id: &str, aliases: &[String], source: &str) -> Result<()> {
        let now = Self::now();
        let conn = self.conn.lock().unwrap();
        for alias in aliases {
            let lk = concept_key(alias);
            if lk.is_empty() {
                continue;
            }
            let denied: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM concept_alias_denials WHERE entity_id=?1 AND label_key=?2",
                    params![entity_id, lk],
                    |r| r.get(0),
                )
                .optional()?;
            if denied.is_some() {
                continue;
            }
            let preferred_clash: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM concept_labels WHERE label_key=?1 AND role='preferred' AND entity_id != ?2",
                    params![lk, entity_id],
                    |r| r.get(0),
                )
                .optional()?;
            if preferred_clash.is_some() {
                continue;
            }
            let _ = conn.execute(
                "INSERT OR IGNORE INTO concept_labels (entity_id, label, label_key, match_key, role, source, created_at)
                 VALUES (?1,?2,?3,?4,'alias',?5,?6)",
                params![entity_id, alias, lk, match_key(alias), source, now],
            );
        }
        Ok(())
    }

    pub fn get_aliases(&self, entity_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT label FROM concept_labels WHERE entity_id=?1 AND role='alias'")?;
        let rows = stmt.query_map(params![entity_id], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn load_concept_alias_map(&self) -> Result<std::collections::HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT lower(a.label), p.label FROM concept_labels a
             JOIN concept_labels p ON p.entity_id=a.entity_id AND p.role='preferred'
             WHERE a.role='alias'",
        )?;
        let mut counts: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows.flatten() {
            counts.entry(row.0).or_default().push(row.1);
        }
        Ok(counts
            .into_iter()
            .filter_map(|(k, v)| {
                let mut u = v;
                u.sort();
                u.dedup();
                if u.len() == 1 {
                    Some((k, u[0].clone()))
                } else {
                    None
                }
            })
            .collect())
    }

    pub fn get_sources_for_entity(&self, entity_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT DISTINCT source_path FROM concepts WHERE entity_id=?1")?;
        let rows = stmt.query_map(params![entity_id], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_sources_for_concept(&self, name: &str) -> Result<Vec<String>> {
        if let Some(id) = self.entity_id_for_name(name)? {
            self.get_sources_for_entity(&id)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn concepts_needing_compile(&self) -> Result<Vec<(String, String)>> {
        // (entity_id, preferred_label)
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT c.entity_id, COALESCE(p.label, c.name)
            FROM concepts c
            LEFT JOIN concept_labels p ON p.entity_id=c.entity_id AND p.role='preferred'
            LEFT JOIN blocked_concepts b ON b.concept = COALESCE(p.label, c.name)
            WHERE b.concept IS NULL
              AND NOT EXISTS (
                SELECT 1 FROM wiki_articles w
                WHERE w.entity_id = c.entity_id AND w.status IN ('draft','verified','published')
              )
            "#,
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn mark_compile_state_for_entity(
        &self,
        entity_id: &str,
        source_path: &str,
        concept_name: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        let now = Self::now();
        let compiled = if status == "compiled" {
            Some(now.clone())
        } else {
            None
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO concept_compile_state (entity_id, source_path, concept_name, status, error, compiled_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(entity_id, source_path) DO UPDATE SET
               concept_name=excluded.concept_name, status=excluded.status, error=excluded.error,
               compiled_at=excluded.compiled_at, updated_at=excluded.updated_at",
            params![entity_id, to_posix(source_path), concept_name, status, error, compiled, now],
        )?;
        Ok(())
    }

    pub fn upsert_article(&self, rec: &WikiArticleRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let sources = serde_json::to_string(&rec.sources)?;
        let syn_src = serde_json::to_string(&rec.synthesis_sources)?;
        let syn_hash = serde_json::to_string(&rec.synthesis_source_hashes)?;
        let article_id = rec.article_id.clone().unwrap_or_else(generate_article_id);
        conn.execute(
            "INSERT INTO wiki_articles
             (path,title,sources,content_hash,created_at,updated_at,status,approved_at,approval_notes,kind,question_hash,synthesis_sources,synthesis_source_hashes,article_id,last_compile_pipeline,entity_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(path) DO UPDATE SET
               title=excluded.title, sources=excluded.sources, content_hash=excluded.content_hash,
               updated_at=excluded.updated_at, status=excluded.status, kind=excluded.kind,
               question_hash=excluded.question_hash, synthesis_sources=excluded.synthesis_sources,
               synthesis_source_hashes=excluded.synthesis_source_hashes, article_id=COALESCE(wiki_articles.article_id, excluded.article_id),
               last_compile_pipeline=excluded.last_compile_pipeline, entity_id=excluded.entity_id",
            params![
                to_posix(&rec.path), rec.title, sources, rec.content_hash, rec.created_at, rec.updated_at,
                rec.status, rec.approved_at, rec.approval_notes, rec.kind, rec.question_hash,
                syn_src, syn_hash, article_id, rec.last_compile_pipeline, rec.entity_id
            ],
        )?;
        Ok(())
    }

    pub fn insert_synthesis_atomic(&self, rec: &WikiArticleRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let sources = serde_json::to_string(&rec.sources)?;
        let syn_src = serde_json::to_string(&rec.synthesis_sources)?;
        let syn_hash = serde_json::to_string(&rec.synthesis_source_hashes)?;
        let article_id = rec.article_id.clone().unwrap_or_else(generate_article_id);
        match conn.execute(
            "INSERT INTO wiki_articles
             (path,title,sources,content_hash,created_at,updated_at,status,kind,question_hash,synthesis_sources,synthesis_source_hashes,article_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,'synthesis',?8,?9,?10,?11)",
            params![
                to_posix(&rec.path), rec.title, sources, rec.content_hash, rec.created_at, rec.updated_at,
                rec.status, rec.question_hash, syn_src, syn_hash, article_id
            ],
        ) {
            Ok(_) => Ok(()),
            Err(e) if e.to_string().contains("UNIQUE") => {
                Err(Error::SynthesisConflict(e.to_string()))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_article(&self, path: &str) -> Result<Option<WikiArticleRecord>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT path,title,sources,content_hash,created_at,updated_at,status,approved_at,approval_notes,kind,question_hash,synthesis_sources,synthesis_source_hashes,article_id,last_compile_pipeline,entity_id FROM wiki_articles WHERE path=?1",
            params![to_posix(path)],
            row_to_article,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_articles(&self) -> Result<Vec<WikiArticleRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT path,title,sources,content_hash,created_at,updated_at,status,approved_at,approval_notes,kind,question_hash,synthesis_sources,synthesis_source_hashes,article_id,last_compile_pipeline,entity_id FROM wiki_articles ORDER BY title",
        )?;
        let rows = stmt.query_map([], row_to_article)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn find_synthesis_by_question_hash(&self, qh: &str) -> Result<Option<WikiArticleRecord>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT path,title,sources,content_hash,created_at,updated_at,status,approved_at,approval_notes,kind,question_hash,synthesis_sources,synthesis_source_hashes,article_id,last_compile_pipeline,entity_id FROM wiki_articles WHERE question_hash=?1",
            params![qh],
            row_to_article,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn publish_article(&self, old_path: &str, new_path: &str) -> Result<()> {
        let now = Self::now();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE wiki_articles SET path=?1, status='published', approved_at=COALESCE(approved_at,?2), updated_at=?2 WHERE path=?3",
            params![to_posix(new_path), now, to_posix(old_path)],
        )?;
        Ok(())
    }

    pub fn verify_article(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE wiki_articles SET status='verified', updated_at=?1 WHERE path=?2",
            params![Self::now(), to_posix(path)],
        )?;
        Ok(())
    }

    pub fn delete_article(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM wiki_articles WHERE path=?1",
            params![to_posix(path)],
        )?;
        Ok(())
    }

    pub fn count_articles_by_status(&self) -> Result<(i64, i64, i64)> {
        let conn = self.conn.lock().unwrap();
        let draft: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wiki_articles WHERE status='draft'",
            [],
            |r| r.get(0),
        )?;
        let verified: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wiki_articles WHERE status='verified'",
            [],
            |r| r.get(0),
        )?;
        let published: i64 = conn.query_row(
            "SELECT COUNT(*) FROM wiki_articles WHERE status='published'",
            [],
            |r| r.get(0),
        )?;
        Ok((draft, verified, published))
    }

    pub fn add_rejection(
        &self,
        concept: &str,
        feedback: &str,
        rejected_body: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO rejections (concept, feedback, rejected_body, rejected_at) VALUES (?1,?2,?3,?4)",
            params![concept, feedback, rejected_body, Self::now()],
        )?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM rejections WHERE concept=?1",
            params![concept],
            |r| r.get(0),
        )?;
        if count >= REJECTION_CAP {
            drop(conn);
            self.mark_concept_blocked(concept)?;
        }
        Ok(count)
    }

    pub fn get_rejections(&self, concept: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT feedback, COALESCE(rejected_body,'') FROM rejections WHERE concept=?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![concept], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn rejection_count(&self, concept: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM rejections WHERE concept=?1",
            params![concept],
            |r| r.get(0),
        )
        .map_err(Into::into)
    }

    pub fn mark_concept_blocked(&self, concept: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO blocked_concepts (concept, blocked_at) VALUES (?1,?2)",
            params![concept, Self::now()],
        )?;
        Ok(())
    }

    pub fn is_concept_blocked(&self, concept: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM blocked_concepts WHERE concept=?1",
            params![concept],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn unblock_concept(&self, concept: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM blocked_concepts WHERE concept=?1",
            params![concept],
        )?;
        Ok(n > 0)
    }

    pub fn list_blocked_concepts(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT concept FROM blocked_concepts ORDER BY concept")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn add_stub(&self, concept: &str, source: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO stubs (concept, created_at, source) VALUES (?1,?2,?3)",
            params![concept, Self::now(), source],
        )?;
        Ok(())
    }

    pub fn has_stub(&self, concept: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM stubs WHERE concept=?1",
            params![concept],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn get_stubs(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT concept FROM stubs ORDER BY concept")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn delete_stub(&self, concept: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM stubs WHERE concept=?1", params![concept])?;
        Ok(())
    }

    pub fn upsert_item(&self, rec: &KnowledgeItemRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO knowledge_items (name,kind,subtype,status,confidence,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(name) DO UPDATE SET kind=excluded.kind, subtype=excluded.subtype, status=excluded.status,
               confidence=excluded.confidence, updated_at=excluded.updated_at",
            params![rec.name, rec.kind, rec.subtype, rec.status, rec.confidence, rec.created_at, rec.updated_at],
        )?;
        Ok(())
    }

    pub fn list_items(&self) -> Result<Vec<KnowledgeItemRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT name,kind,subtype,status,confidence,created_at,updated_at FROM knowledge_items ORDER BY name")?;
        let rows = stmt.query_map([], |r| {
            Ok(KnowledgeItemRecord {
                name: r.get(0)?,
                kind: r.get(1)?,
                subtype: r.get(2)?,
                status: r.get(3)?,
                confidence: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_item(&self, name: &str) -> Result<Option<KnowledgeItemRecord>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT name,kind,subtype,status,confidence,created_at,updated_at FROM knowledge_items WHERE name=?1",
            params![name],
            |r| {
                Ok(KnowledgeItemRecord {
                    name: r.get(0)?,
                    kind: r.get(1)?,
                    subtype: r.get(2)?,
                    status: r.get(3)?,
                    confidence: r.get(4)?,
                    created_at: r.get(5)?,
                    updated_at: r.get(6)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn add_item_mention(&self, rec: &ItemMentionRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO item_mentions (item_name, source_path, mention_text, context, evidence_level, confidence)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![rec.item_name, to_posix(&rec.source_path), rec.mention_text, rec.context, rec.evidence_level, rec.confidence],
        )?;
        Ok(())
    }

    pub fn get_item_mentions(&self, name: &str) -> Result<Vec<ItemMentionRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id,item_name,source_path,mention_text,context,evidence_level,confidence FROM item_mentions WHERE item_name=?1",
        )?;
        let rows = stmt.query_map(params![name], |r| {
            Ok(ItemMentionRecord {
                id: r.get(0)?,
                item_name: r.get(1)?,
                source_path: r.get(2)?,
                mention_text: r.get(3)?,
                context: r.get(4)?,
                evidence_level: r.get(5)?,
                confidence: r.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn upsert_relation(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        confidence: f64,
        segment_id: &str,
        evidence: &str,
    ) -> Result<String> {
        let sk = concept_key(subject);
        let ok = concept_key(object);
        let id = relation_id(&sk, predicate, &ok);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO relations (id,subject,predicate,object,confidence,source_segment_id,subject_key,object_key)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id) DO UPDATE SET confidence=MAX(confidence, excluded.confidence)",
            params![id, subject, predicate, object, confidence, segment_id, sk, ok],
        )?;
        conn.execute(
            "INSERT INTO relation_evidence (relation_id, evidence_text, source_segment_id) VALUES (?1,?2,?3)",
            params![id, evidence, segment_id],
        )?;
        Ok(id)
    }

    pub fn list_relations(&self) -> Result<Vec<(String, String, String, String, f64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id,subject,predicate,object,confidence FROM relations")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn list_relations_for_concept(
        &self,
        name: &str,
    ) -> Result<Vec<(String, String, String, String, f64)>> {
        let key = concept_key(name);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id,subject,predicate,object,confidence FROM relations WHERE subject_key=?1 OR object_key=?1",
        )?;
        let rows = stmt.query_map(params![key], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_relation(&self, id: &str) -> Result<Option<(String, String, String, String, f64)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id,subject,predicate,object,confidence FROM relations WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_relation_evidence(&self, relation_id: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT evidence_text, source_segment_id FROM relation_evidence WHERE relation_id=?1",
        )?;
        let rows = stmt.query_map(params![relation_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn upsert_source_document(
        &self,
        id: &str,
        source_type: &str,
        origin_uri: Option<&str>,
        title: Option<&str>,
        raw_hash: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO source_documents (id, source_type, origin_uri, title, imported_at, raw_hash, redistribution)
             VALUES (?1,?2,?3,?4,?5,?6,'unknown')
             ON CONFLICT(id) DO UPDATE SET title=COALESCE(excluded.title, title), origin_uri=COALESCE(excluded.origin_uri, origin_uri)",
            params![id, source_type, origin_uri, title, Self::now(), raw_hash],
        )?;
        Ok(())
    }

    pub fn insert_source_segment(
        &self,
        id: &str,
        identity: &str,
        ordinal: i64,
        source_id: &str,
        locator: &str,
        text: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO source_segments (id,identity,ordinal,source_id,structural_locator,content_hash,text)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![id, identity, ordinal, source_id, locator, content_hash(text), text],
        )?;
        Ok(())
    }

    pub fn list_segments_for_source(&self, source_id: &str) -> Result<Vec<(String, String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, text, ordinal FROM source_segments WHERE source_id=?1 ORDER BY ordinal",
        )?;
        let rows = stmt.query_map(params![source_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn fetch_segment_by_id(&self, id: &str) -> Result<Option<(String, String, String)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, source_id, text FROM source_segments WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_source_documents(&self) -> Result<Vec<(String, String, Option<String>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, source_type, title FROM source_documents ORDER BY imported_at")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn any_source_license_declared(&self) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM source_documents WHERE license IS NOT NULL AND license != ''",
            [],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn fetch_source_license(&self, source_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT license FROM source_documents WHERE id=?1",
            params![source_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn cache_get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let row: Option<String> = conn
            .query_row(
                "SELECT response_json FROM llm_cache WHERE cache_key=?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?;
        if row.is_some() {
            conn.execute(
                "UPDATE llm_cache SET hit_count=hit_count+1, last_hit_at=?1 WHERE cache_key=?2",
                params![Self::now(), key],
            )?;
        }
        Ok(row)
    }

    pub fn cache_put(&self, key: &str, model: &str, response: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO llm_cache (cache_key, model, response_json, created_at) VALUES (?1,?2,?3,?4)",
            params![key, model, response, Self::now()],
        )?;
        Ok(())
    }

    pub fn cache_clear(&self, older_than_days: Option<i64>) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = if let Some(days) = older_than_days {
            let cutoff = (chrono::Local::now() - chrono::Duration::days(days)).to_rfc3339();
            conn.execute(
                "DELETE FROM llm_cache WHERE created_at < ?1",
                params![cutoff],
            )?
        } else {
            conn.execute("DELETE FROM llm_cache", [])?
        };
        Ok(n)
    }

    pub fn cache_stats(&self) -> Result<(i64, i64, f64)> {
        let conn = self.conn.lock().unwrap();
        let (entries, hits): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(hit_count),0) FROM llm_cache",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let total = entries + hits;
        let rate = if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        };
        Ok((entries, hits, rate))
    }

    pub fn insert_metric_event(
        &self,
        event_type: &str,
        model: Option<&str>,
        tier: Option<&str>,
        prompt_tokens: Option<i64>,
        completion_tokens: Option<i64>,
        latency_ms: Option<i64>,
        success: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO metric_events (ts,event_type,model,tier,prompt_tokens,completion_tokens,latency_ms,success)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![Self::now(), event_type, model, tier, prompt_tokens, completion_tokens, latency_ms, success as i64],
        )?;
        Ok(())
    }

    pub fn clear_metrics(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let a = conn.execute("DELETE FROM metric_events", [])?;
        let b = conn.execute("DELETE FROM metric_daily_rollups", [])?;
        Ok(a + b)
    }

    pub fn metric_event_totals(&self, since: Option<&str>) -> Result<(i64, i64, i64, i64)> {
        let conn = self.conn.lock().unwrap();
        let sql = if since.is_some() {
            "SELECT COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(latency_ms),0) FROM metric_events WHERE ts >= ?1"
        } else {
            "SELECT COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(latency_ms),0) FROM metric_events"
        };
        if let Some(s) = since {
            conn.query_row(sql, params![s], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
        } else {
            conn.query_row(sql, [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
        }
        .map_err(Into::into)
    }

    pub fn stats(&self) -> Result<Json> {
        let conn = self.conn.lock().unwrap();
        let raw: i64 = conn.query_row("SELECT COUNT(*) FROM raw_notes", [], |r| r.get(0))?;
        let articles: i64 =
            conn.query_row("SELECT COUNT(*) FROM wiki_articles", [], |r| r.get(0))?;
        let concepts: i64 =
            conn.query_row("SELECT COUNT(DISTINCT entity_id) FROM concepts", [], |r| {
                r.get(0)
            })?;
        Ok(serde_json::json!({
            "raw_notes": raw,
            "articles": articles,
            "concepts": concepts,
        }))
    }

    pub fn start_compile_run(
        &self,
        pipeline_json: &str,
        fast: &str,
        heavy: &str,
    ) -> Result<String> {
        let id = generate_article_id();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO compile_runs (run_ulid, pipeline_json, fast_model, heavy_model, started_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, pipeline_json, fast, heavy, Self::now()],
        )?;
        Ok(id)
    }

    pub fn finish_compile_run(
        &self,
        id: &str,
        article_count: i64,
        total_tokens: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE compile_runs SET finished_at=?1, article_count=?2, total_tokens=?3 WHERE run_ulid=?4",
            params![Self::now(), article_count, total_tokens, id],
        )?;
        Ok(())
    }

    pub fn rename_preferred_label(
        &self,
        entity_id: &str,
        new_label: &str,
        keep_old_alias: bool,
    ) -> Result<()> {
        let now = Self::now();
        let old = self
            .preferred_label_for_entity(entity_id)?
            .unwrap_or_default();
        let conn = self.conn.lock().unwrap();
        if keep_old_alias && !old.is_empty() {
            let _ = conn.execute(
                "UPDATE concept_labels SET role='alias', source='rename' WHERE entity_id=?1 AND role='preferred'",
                params![entity_id],
            );
        } else {
            conn.execute(
                "DELETE FROM concept_labels WHERE entity_id=?1 AND role='preferred'",
                params![entity_id],
            )?;
        }
        conn.execute(
            "INSERT OR REPLACE INTO concept_labels (entity_id,label,label_key,match_key,role,source,created_at)
             VALUES (?1,?2,?3,?4,'preferred','rename',?5)",
            params![entity_id, new_label, concept_key(new_label), match_key(new_label), now],
        )?;
        conn.execute(
            "INSERT INTO concept_identity_log (op, entity_ids, labels, ts) VALUES ('rename', ?1, ?2, ?3)",
            params![
                serde_json::to_string(&vec![entity_id])?,
                serde_json::to_string(&vec![&old, new_label])?,
                now
            ],
        )?;
        Ok(())
    }

    pub fn merge_entities(&self, loser: &str, winner: &str) -> Result<()> {
        let now = Self::now();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE concept_entities SET status='merged', merged_into=?1, updated_at=?2 WHERE id=?3",
            params![winner, now, loser],
        )?;
        conn.execute(
            "UPDATE concepts SET entity_id=?1 WHERE entity_id=?2",
            params![winner, loser],
        )?;
        conn.execute(
            "UPDATE wiki_articles SET entity_id=?1 WHERE entity_id=?2",
            params![winner, loser],
        )?;
        // Move non-conflicting labels
        let _ = conn.execute(
            "INSERT OR IGNORE INTO concept_labels (entity_id,label,label_key,match_key,role,source,created_at)
             SELECT ?1, label, label_key, match_key, CASE WHEN role='preferred' THEN 'alias' ELSE role END, 'rename', ?2
             FROM concept_labels WHERE entity_id=?3",
            params![winner, now, loser],
        );
        conn.execute(
            "DELETE FROM concept_labels WHERE entity_id=?1",
            params![loser],
        )?;
        conn.execute(
            "INSERT INTO concept_identity_log (op, entity_ids, labels, ts) VALUES ('merge', ?1, '[]', ?2)",
            params![serde_json::to_string(&vec![winner, loser])?, now],
        )?;
        Ok(())
    }

    pub fn list_identity_log(&self) -> Result<Vec<(String, String, String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT op, entity_ids, labels, ts FROM concept_identity_log ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn add_alias(&self, entity_id: &str, alias: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM concept_alias_denials WHERE entity_id=?1 AND label_key=?2",
            params![entity_id, concept_key(alias)],
        )?;
        drop(conn);
        self.upsert_aliases(entity_id, &[alias.to_string()], "user")
    }

    pub fn remove_alias(&self, entity_id: &str, alias: &str) -> Result<()> {
        let lk = concept_key(alias);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM concept_labels WHERE entity_id=?1 AND label_key=?2 AND role='alias'",
            params![entity_id, lk],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO concept_alias_denials (entity_id, label, label_key, created_at) VALUES (?1,?2,?3,?4)",
            params![entity_id, alias, lk, Self::now()],
        )?;
        Ok(())
    }

    pub fn database_size_bytes(&self) -> Result<u64> {
        Ok(std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0))
    }
}

fn row_to_raw(r: &Row) -> rusqlite::Result<RawNoteRecord> {
    Ok(RawNoteRecord {
        path: r.get(0)?,
        content_hash: r.get(1)?,
        status: r.get(2)?,
        summary: r.get(3)?,
        quality: r.get(4)?,
        language: r.get(5)?,
        prompt_version: r.get(6)?,
        ingested_at: r.get(7)?,
        compiled_at: r.get(8)?,
        error: r.get(9)?,
    })
}

fn row_to_article(r: &Row) -> rusqlite::Result<WikiArticleRecord> {
    let sources: String = r.get(2)?;
    let syn: Option<String> = r.get(11)?;
    let syn_h: Option<String> = r.get(12)?;
    Ok(WikiArticleRecord {
        path: r.get(0)?,
        title: r.get(1)?,
        sources: serde_json::from_str(&sources).unwrap_or_default(),
        content_hash: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
        status: r.get(6)?,
        approved_at: r.get(7)?,
        approval_notes: r.get(8)?,
        kind: r.get(9)?,
        question_hash: r.get(10)?,
        synthesis_sources: syn
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        synthesis_source_hashes: syn_h
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        article_id: r.get(13)?,
        last_compile_pipeline: r.get(14)?,
        entity_id: r.get(15)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_fresh_and_roundtrip_raw() {
        let dir = tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        assert_eq!(db.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        let rec = RawNoteRecord {
            path: "raw\\note.md".into(),
            content_hash: "abc".into(),
            status: "ingested".into(),
            summary: Some("s".into()),
            quality: Some("high".into()),
            language: Some("en".into()),
            prompt_version: None,
            ingested_at: Some("t".into()),
            compiled_at: None,
            error: None,
        };
        db.upsert_raw(&rec).unwrap();
        let got = db.get_raw("raw/note.md").unwrap().unwrap();
        assert_eq!(got.path, "raw/note.md");
        assert_eq!(got.summary.as_deref(), Some("s"));
    }

    #[test]
    fn entity_identity_roundtrip() {
        let dir = tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        db.replace_concepts_for_source("raw/a.md", &[("Qubit".into(), vec!["qubits".into()])])
            .unwrap();
        let id = db.entity_id_for_name("Qubit").unwrap().unwrap();
        assert_eq!(
            db.preferred_label_for_entity(&id).unwrap().as_deref(),
            Some("Qubit")
        );
        assert!(db.get_aliases(&id).unwrap().iter().any(|a| a == "qubits"));
    }
}
