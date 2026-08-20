# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Install (from this checkout)
cargo install --path .

# Tests (all offline, no Ollama required)
cargo test
cargo test --test ingest_mock -- --nocapture   # single integration crate
cargo test hashing::tests --lib

# Lint & format
cargo fmt --all -- --check
cargo fmt --all
cargo clippy --all-targets -- -D warnings

# Smoke tests (user preference: run against LM Studio with gemma4:e4b)
# Requires LM Studio server at http://localhost:1234/v1 with gemma4:e4b loaded.
# Build the binary first; smoke scripts call `synto` via SYNTO_BIN or target/debug/synto.
cargo build
PROVIDER=lm_studio FAST_MODEL=gemma4:e4b HEAVY_MODEL=gemma4:e4b SYNTO_BIN=./target/debug/synto bash scripts/smoke_test.sh
# If LM Studio rejects the alias, use the exact loaded id it reports, e.g.
# PROVIDER=lm_studio FAST_MODEL=google/gemma-4-e4b HEAVY_MODEL=google/gemma-4-e4b bash scripts/smoke_test.sh
# The main smoke can take 15+ minutes with gemma4:e4b; use a long command timeout.
```

## Architecture

Three-stage local LLM pipeline turning Obsidian raw notes into a synthesized wiki.

**Data flow:** `raw/*.md` → **ingest** (fast model → AnalysisResult) → **compile** (heavy model → SingleArticle drafts) → **approve** (publish to `wiki/`)

**Vault structure:** `raw/` (immutable user notes), `wiki/` (published articles), `wiki/.drafts/`, `wiki/sources/`, `wiki/queries/`, `wiki/synthesis/`, `.synto/state.db`

### Query synthesis

- `synto query --save` writes dated Q&A notes to `wiki/queries/`
- `synto query --synthesize` writes published synthesis articles to `wiki/synthesis/`
- synthesis rows are tracked in `wiki_articles` with `kind="synthesis"`, `question_hash`, `synthesis_sources`, and `synthesis_source_hashes`
- duplicate syntheses are keyed by normalized question hash
- `update_in_place` must respect manual-edit protection by comparing the on-disk body hash with the DB `content_hash`
- compare runs must not create or mutate active `wiki/synthesis/` content

### Key modules (`src/`)

- `cli.rs` — clap CLI entry point, all commands registered here
- `config.rs` / `global_config.rs` — Two-tier config: per-vault `synto.toml` + user-level `~/.config/synto/config.toml`
- `models.rs` — serde schemas for LLM I/O (AnalysisResult, SingleArticle, PageSelection, QueryAnswer) and internal state (RawNoteRecord, WikiArticleRecord)
- `state.rs` — SQLite state DB tracking note lifecycle (new → ingested → compiled → published/failed), concepts, and articles
- `llm.rs` — httpx-free reqwest wrapper around Ollama / OpenAI-compat / Anthropic-compat HTTP APIs
- `llm.rs` (`request_structured`) — 3-tier JSON extraction fallback: native `format=json` → regex extraction → retry with error feedback
- `vault.rs` — Frontmatter parsing, wikilink extraction, atomic writes
- `indexer.rs` — Generates `wiki/index.md` and append-only operation log
- `git_ops.rs` — Auto-commit with `[synto]` prefix, safe undo via `git revert`
- `watcher.rs` — Debounced file watcher (notify)

### Pipeline stages (`src/pipeline/`)

- `ingest.rs` — Fast model analyzes raw notes, extracts concepts, creates source summary pages
- `compile.rs` — Default: concept-driven (one article per concept, incremental). Legacy: two-step LLM planning (`--legacy`). Manual-edit protection via content_hash comparison
- `query.rs` — Index-based page routing (no embeddings), fast model selects pages, heavy model answers
- `lint.rs` — Static health checks (orphans, broken links, stale articles, missing frontmatter)

## Conventions

- **Two LLM tiers:** fast model (gemma4:e4b, 8K ctx) for analysis/routing, heavy model (qwen2.5:14b, 16K ctx) for writing. For manual/smoke testing, use gemma4:e4b for both fast and heavy
- **Serde models for LLM output:** Keep schemas small and flat (no nested lists of objects) for 4B model reliability. JSON schema is injected into system prompts
- **Atomic writes:** `vault::atomic_write()` uses temp file + rename for crash safety
- **Content hashing:** SHA256 on note body (excluding frontmatter) for dedup and manual-edit detection
- **Concept normalization:** Case-insensitive matching against existing canonical names during ingest (`concept_key` / `match_key`)
- **Config loading order:** `--vault` flag → `SYNTO_VAULT` env var → global config default vault → error
- **Git safety:** All auto-operations use `[synto]`-prefixed commits; undo uses `git revert` (never destructive)
- **Error handling:** LLM failures log + mark note as "failed" + continue (no crash). Config loading returns None on failure (fail open)
- **Code comments:** Add comments only when the reason or invariant is non-obvious from the code itself. Prefer tests and commit messages for routine explanation; use comments for safeguards, provider quirks, heuristics, and tradeoffs that future agents or humans could otherwise misread.
- **Testing:** Unit tests live next to the modules; integration tests under `tests/` mock `LlmClient` via `MockClient`, use tempfile vaults and on-disk SQLite. No live provider is required for `cargo test`.
- **Rust 1.85+:** edition 2021, `from_parts` constructors for test injection, rustls for HTTPS (no OpenSSL-dev required)
- **rustfmt:** max_width 100
