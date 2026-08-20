CREATE TABLE IF NOT EXISTS schema_version (
    id      INTEGER PRIMARY KEY CHECK(id = 1),
    version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS raw_notes (
    path              TEXT PRIMARY KEY,
    content_hash      TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'new',
    summary           TEXT,
    quality           TEXT,
    language          TEXT,
    ingested_at       TEXT,
    compiled_at       TEXT,
    error             TEXT,
    prompt_version    TEXT
);

CREATE TABLE IF NOT EXISTS concepts (
    entity_id   TEXT NOT NULL,
    source_path TEXT NOT NULL,
    name        TEXT NOT NULL,
    PRIMARY KEY (entity_id, source_path)
);
-- idx_concepts_entity is created in the v22 post-hook, NOT here: _SCHEMA runs on
-- every open before migrations, and a pre-v22 `concepts` table (PK name) has no
-- entity_id column yet, so indexing it here would fail on the upgrade path.

CREATE TABLE IF NOT EXISTS wiki_articles (
    path           TEXT PRIMARY KEY,
    title          TEXT NOT NULL,
    sources        TEXT NOT NULL,
    content_hash   TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'draft'
                       CHECK (status IN ('draft','verified','published')),
    approved_at    TEXT,
    approval_notes TEXT,
    kind           TEXT NOT NULL DEFAULT 'concept',
    question_hash  TEXT,
    synthesis_sources TEXT,
    synthesis_source_hashes TEXT,
    article_id     TEXT,
    last_compile_pipeline TEXT,
    entity_id      TEXT
);

CREATE TABLE IF NOT EXISTS source_documents (
    id                TEXT PRIMARY KEY,
    source_type       TEXT NOT NULL DEFAULT 'unknown_text',
    origin_uri        TEXT,
    title             TEXT,
    imported_at       TEXT,
    raw_hash          TEXT,
    normalized_hash   TEXT,
    extractor_version TEXT,
    license           TEXT,
    redistribution    TEXT NOT NULL DEFAULT 'unknown',
    metadata_json     TEXT
);

CREATE TABLE IF NOT EXISTS source_segments (
    id                  TEXT PRIMARY KEY,
    identity            TEXT NOT NULL,
    ordinal             INTEGER NOT NULL,
    source_id           TEXT NOT NULL,
    structural_locator  TEXT NOT NULL,
    content_hash        TEXT NOT NULL,
    text                TEXT NOT NULL,
    section_path_json   TEXT,
    page_start          INTEGER,
    page_end            INTEGER,
    char_start          INTEGER,
    char_end            INTEGER,
    metadata_json       TEXT,
    FOREIGN KEY (source_id) REFERENCES source_documents(id)
);

CREATE TABLE IF NOT EXISTS source_warnings (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id    TEXT NOT NULL,
    severity     TEXT NOT NULL CHECK(severity IN ('info', 'warning', 'error')),
    category     TEXT NOT NULL,
    message      TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    FOREIGN KEY (source_id) REFERENCES source_documents(id)
);

CREATE TABLE IF NOT EXISTS metric_events (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    ts                TEXT NOT NULL,
    vault_id          TEXT,
    event_type        TEXT NOT NULL,
    model             TEXT,
    tier              TEXT,
    prompt_tokens     INTEGER,
    completion_tokens INTEGER,
    latency_ms        INTEGER,
    success           INTEGER CHECK(success IN (0, 1)),
    source_id_hash    TEXT,
    metadata_json     TEXT
);

CREATE TABLE IF NOT EXISTS metric_daily_rollups (
    day               TEXT NOT NULL,
    vault_id          TEXT NOT NULL,
    event_type        TEXT NOT NULL,
    tier              TEXT NOT NULL DEFAULT '',
    calls             INTEGER NOT NULL DEFAULT 0,
    prompt_tokens     INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    latency_ms_total  INTEGER NOT NULL DEFAULT 0,
    successes         INTEGER NOT NULL DEFAULT 0,
    failures          INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, vault_id, event_type, tier)
);

CREATE TABLE IF NOT EXISTS generated_assets (
    path                TEXT PRIMARY KEY,
    source_id           TEXT NOT NULL,
    asset_type          TEXT NOT NULL,
    master_path         TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    last_referenced_at  TEXT,
    referenced_by_json  TEXT NOT NULL DEFAULT '[]',
    FOREIGN KEY (source_id) REFERENCES source_documents(id)
);

CREATE TABLE IF NOT EXISTS rejections (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    concept       TEXT NOT NULL,
    feedback      TEXT NOT NULL,
    rejected_body TEXT,
    rejected_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS stubs (
    concept    TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    source     TEXT NOT NULL DEFAULT 'auto'
);

CREATE TABLE IF NOT EXISTS blocked_concepts (
    concept    TEXT PRIMARY KEY,
    blocked_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS knowledge_items (
    name       TEXT PRIMARY KEY,
    kind       TEXT NOT NULL DEFAULT 'ambiguous',
    subtype    TEXT,
    status     TEXT NOT NULL DEFAULT 'candidate',
    confidence REAL NOT NULL DEFAULT 0.5,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS item_mentions (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    item_name      TEXT NOT NULL,
    source_path    TEXT NOT NULL,
    mention_text   TEXT NOT NULL,
    context        TEXT,
    evidence_level TEXT NOT NULL,
    confidence     REAL NOT NULL DEFAULT 0.5,
    UNIQUE(item_name, source_path, mention_text, evidence_level)
);

CREATE TABLE IF NOT EXISTS ingest_chunks (
    source_path        TEXT NOT NULL,
    content_hash       TEXT NOT NULL,
    chunk_index        INTEGER NOT NULL,
    chunk_count        INTEGER NOT NULL,
    chunk_size         INTEGER NOT NULL,
    checkpoint_schema  INTEGER NOT NULL,
    result_json        TEXT NOT NULL,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    PRIMARY KEY (source_path, content_hash, chunk_index, chunk_count, chunk_size, checkpoint_schema)
);

CREATE TABLE IF NOT EXISTS concept_compile_state (
    concept_name TEXT NOT NULL,
    source_path  TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',
    error        TEXT,
    compiled_at  TEXT,
    updated_at   TEXT NOT NULL,
    PRIMARY KEY (concept_name, source_path),
    CHECK (status IN ('pending', 'failed', 'compiled', 'deferred_draft', 'deferred_manual_edit'))
);

CREATE INDEX IF NOT EXISTS idx_raw_hash ON raw_notes(content_hash);
CREATE INDEX IF NOT EXISTS idx_raw_status ON raw_notes(status);
CREATE INDEX IF NOT EXISTS idx_concept_name ON concepts(name);
CREATE INDEX IF NOT EXISTS idx_ingest_chunks_source ON ingest_chunks(source_path, content_hash);
CREATE INDEX IF NOT EXISTS idx_concept_compile_status ON concept_compile_state(status, source_path);
CREATE INDEX IF NOT EXISTS idx_concept_compile_name ON concept_compile_state(lower(concept_name));
CREATE INDEX IF NOT EXISTS idx_rejections_concept ON rejections(concept);
CREATE INDEX IF NOT EXISTS idx_items_kind ON knowledge_items(kind);
CREATE INDEX IF NOT EXISTS idx_items_status ON knowledge_items(status);
CREATE INDEX IF NOT EXISTS idx_mentions_item ON item_mentions(item_name);
CREATE INDEX IF NOT EXISTS idx_mentions_source ON item_mentions(source_path);
CREATE INDEX IF NOT EXISTS idx_source_segments_source ON source_segments(source_id);
CREATE INDEX IF NOT EXISTS idx_source_segments_identity ON source_segments(identity);
CREATE INDEX IF NOT EXISTS idx_source_warnings_source ON source_warnings(source_id);
CREATE INDEX IF NOT EXISTS idx_metric_events_ts ON metric_events(ts);
CREATE INDEX IF NOT EXISTS idx_metric_events_type_ts ON metric_events(event_type, ts);
CREATE INDEX IF NOT EXISTS idx_metric_daily_rollups_day ON metric_daily_rollups(day);
CREATE INDEX IF NOT EXISTS idx_generated_assets_source ON generated_assets(source_id);

CREATE TABLE IF NOT EXISTS compile_runs (
    run_ulid        TEXT PRIMARY KEY,
    pipeline_json   TEXT NOT NULL,
    fast_model      TEXT NOT NULL,
    heavy_model     TEXT NOT NULL,
    started_at      TEXT NOT NULL,
    finished_at     TEXT,
    article_count   INTEGER NOT NULL DEFAULT 0,
    total_tokens    INTEGER NOT NULL DEFAULT 0,
    total_cost_usd  REAL NOT NULL DEFAULT 0.0
);

CREATE TABLE IF NOT EXISTS llm_cache (
    cache_key    TEXT PRIMARY KEY,
    model        TEXT NOT NULL,
    response_json TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    last_hit_at  TEXT,
    hit_count    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS concept_occurrences (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    concept_name      TEXT NOT NULL,
    source_segment_id TEXT,
    source_path       TEXT,
    ordinal           INTEGER NOT NULL DEFAULT 0,
    confidence        REAL NOT NULL DEFAULT 1.0,
    extraction_run    TEXT,
    entity_id         TEXT,
    surface           TEXT,
    resolution_status TEXT NOT NULL DEFAULT 'unresolved'
                      CHECK (resolution_status IN ('resolved', 'ambiguous', 'unresolved'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_occ_seg
    ON concept_occurrences(concept_name, source_segment_id)
    WHERE source_segment_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_concept_occurrences_concept ON concept_occurrences(concept_name);
-- idx_occ_path (source_path) and idx_concept_occurrences_entity (entity_id) are NOT created
-- here: those columns are added by the v19 migration, and this base schema runs (via
-- executescript) BEFORE migrations. On a pre-v19 vault the columns don't exist yet, so
-- creating the indexes here would raise "no such column" and block the upgrade. They are
-- created in _extend_occurrences_v19 instead, which runs for both fresh and upgraded DBs.

CREATE TABLE IF NOT EXISTS concept_occurrence_candidates (
    occurrence_id INTEGER NOT NULL REFERENCES concept_occurrences(id) ON DELETE CASCADE,
    entity_id     TEXT NOT NULL,
    PRIMARY KEY (occurrence_id, entity_id)
);

CREATE TABLE IF NOT EXISTS concept_entities (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL DEFAULT 'concept',
    status      TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'merged')),
    merged_into TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS concept_labels (
    entity_id  TEXT NOT NULL REFERENCES concept_entities(id),
    label      TEXT NOT NULL,
    label_key  TEXT NOT NULL,
    match_key  TEXT NOT NULL,
    role       TEXT NOT NULL CHECK (role IN ('preferred', 'alias')),
    source     TEXT NOT NULL
                   CHECK (source IN ('extracted', 'user', 'rename', 'legacy_backfill')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (entity_id, label_key)
);

CREATE INDEX IF NOT EXISTS idx_concept_labels_label_key ON concept_labels(label_key);
CREATE INDEX IF NOT EXISTS idx_concept_labels_match_key ON concept_labels(match_key);
CREATE UNIQUE INDEX IF NOT EXISTS idx_concept_labels_preferred_global
    ON concept_labels(label_key) WHERE role = 'preferred';
CREATE UNIQUE INDEX IF NOT EXISTS idx_concept_labels_preferred_per_entity
    ON concept_labels(entity_id) WHERE role = 'preferred';

CREATE TABLE IF NOT EXISTS concept_identity_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    op         TEXT NOT NULL CHECK (op IN ('merge', 'split', 'rename', 'unmerge')),
    entity_ids TEXT NOT NULL,
    labels     TEXT NOT NULL,
    meta       TEXT,
    ts         TEXT NOT NULL
);

-- Advisory worklist (v26): pairs the identity rule promoted apart but that a human may want
-- merged (e.g. "GD" extracted as a concept after it was a weak alias of "Gradient Descent").
-- The pair is ordered by preferred label_key (NOT entity_id, which is random) so the same
-- logical pair dedups to one row regardless of ingest order. Advisory only; re-derived on
-- re-ingest, so it stays out of the .notus/INDEX.json durability seed.
CREATE TABLE IF NOT EXISTS concept_merge_candidates (
    entity_a   TEXT NOT NULL,
    entity_b   TEXT NOT NULL,
    surface    TEXT NOT NULL,
    reason     TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY (entity_a, entity_b, surface)
);

-- `notus concept alias remove` tombstone (v27, discussion #94): extraction is
-- LLM-non-deterministic and upsert_aliases(source='extracted') runs on every ingest, so a
-- plain DELETE of a wrong alias gets silently re-attached by the next ingest. Every live
-- alias-insert path checks this table (via _is_alias_denied) before writing a role='alias'
-- row. Separate table, not a concept_labels.role value — the role CHECK is locked and a
-- separate table keeps the >=2-entities ambiguity counting in list_alias_map/
-- load_concept_alias_map untouched.
CREATE TABLE IF NOT EXISTS concept_alias_denials (
    entity_id  TEXT NOT NULL,
    label      TEXT NOT NULL,
    label_key  TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (entity_id, label_key)
);

-- Concept-to-concept relations (feature 26, v28/v29). `relation_candidates` is the raw
-- LLM output log (cleared per source on re-ingest); `relations` is the approved set keyed
-- by concept_key(subject):predicate:concept_key(object) so casing drift across extraction
-- runs can't fork identity. subject_key/object_key (v29) store concept_key(...) so reads
-- match by normalized key, not display string. Their indexes are created in the v29
-- post-hook, NOT here: _SCHEMA runs on every open BEFORE migrations, and a pre-v29 vault
-- lacks the columns (same trap as idx_concepts_entity / v22). source_segment_id columns
-- are TEXT with no foreign key: pseudo-segment ids like `note:<stem>:<idx>` are written
-- for plain notes.
CREATE TABLE IF NOT EXISTS relations (
    id TEXT PRIMARY KEY,
    subject TEXT NOT NULL,
    predicate TEXT NOT NULL,
    object TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.0,
    source_segment_id TEXT NOT NULL,
    subject_key TEXT NOT NULL DEFAULT '',
    object_key TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS relation_evidence (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    relation_id TEXT NOT NULL REFERENCES relations(id),
    evidence_text TEXT NOT NULL,
    source_segment_id TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS relation_candidates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject TEXT NOT NULL,
    predicate TEXT NOT NULL,
    object TEXT NOT NULL,
    evidence TEXT NOT NULL DEFAULT '',
    source_segment_id TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.0,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_relations_subject ON relations(subject);
CREATE INDEX IF NOT EXISTS idx_relations_object ON relations(object);
