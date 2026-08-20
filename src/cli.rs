use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
#[cfg(test)]
use clap::CommandFactory;
use clap::{Parser, Subcommand};

use crate::config::{default_wiki_toml, Config};
use crate::global_config::{
    forget_known_vault, load_global_config, load_global_config_strict, load_known_vaults,
    register_known_vault, save_global_config, vault_key, ForgetResult,
};
use crate::paths::{
    CLI_NAME, CONFIG_FILE_NAME, PROJECT_DISCUSSIONS_URL, PROJECT_ISSUES_URL, PROJECT_REPO_URL,
    VAULT_ENV_VAR, VERSION,
};
use crate::state::StateDb;

#[derive(Parser)]
#[command(name = CLI_NAME, version = VERSION, about = "Synto — local knowledge packs and synthesized wiki pipeline.")]
pub struct Cli {
    #[arg(long, global = true, env = VAULT_ENV_VAR)]
    vault: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create vault structure and initialise Synto
    Init {
        vault_path: PathBuf,
        #[arg(long)]
        existing: bool,
        #[arg(long)]
        non_interactive: bool,
        #[arg(long)]
        default: bool,
    },
    /// Copy an old olw vault layout into the Synto layout
    MigrateOlw,
    /// Interactive provider/model/vault wizard
    Setup {
        #[arg(long)]
        non_interactive: bool,
        #[arg(long)]
        reset: bool,
        #[arg(long)]
        provider: Option<String>,
    },
    /// Analyze raw notes
    Ingest {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        force: bool,
        paths: Vec<PathBuf>,
        #[arg(long)]
        fast_model: Option<String>,
        #[arg(long)]
        heavy_model: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_url: Option<String>,
    },
    /// Synthesize notes into wiki articles (writes to .drafts/)
    Compile {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        auto_approve: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        legacy: bool,
        #[arg(long)]
        concept: Vec<String>,
        #[arg(long)]
        retry_failed: bool,
        #[arg(long)]
        fast_model: Option<String>,
        #[arg(long)]
        heavy_model: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_url: Option<String>,
    },
    /// Publish drafts to wiki/
    Approve {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        min_confidence: Option<f64>,
        files: Vec<PathBuf>,
    },
    /// Mark drafts verified (not published)
    Verify {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        min_confidence: Option<f64>,
        files: Vec<PathBuf>,
    },
    /// Discard a draft
    Reject {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        feedback: Option<String>,
        files: Vec<PathBuf>,
    },
    /// Show vault health and pending drafts
    Status {
        #[arg(long)]
        failed: bool,
    },
    /// Offline structural eval harness
    Eval {
        #[arg(long)]
        queries: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Revert last N synto auto-commits
    Undo {
        #[arg(long, default_value_t = 1)]
        steps: usize,
        #[arg(long)]
        force: bool,
    },
    /// Wipe state.db + wiki/ (keeps raw/)
    Clean {
        #[arg(long)]
        yes: bool,
    },
    /// Print issues/discussions/repo URLs
    Support,
    /// Vault structure, providers, models
    Doctor {
        #[arg(long)]
        backlog: bool,
        #[arg(long)]
        reconcile: bool,
    },
    /// Index-routed Q&A
    Query {
        question: String,
        #[arg(long)]
        save: bool,
        #[arg(long)]
        synthesize: bool,
    },
    /// Debounced raw/ watcher
    Watch {
        #[arg(long)]
        auto_approve: bool,
    },
    /// Read-only MCP server
    Serve {
        #[arg(long, default_value = "stdio")]
        transport: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8000)]
        port: u16,
    },
    /// Full ingest→compile→lint→[approve]
    Run {
        #[arg(long)]
        auto_approve: bool,
        #[arg(long)]
        fix: bool,
        #[arg(long, default_value_t = 2)]
        max_rounds: u32,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        min_confidence: Option<f64>,
        #[arg(long)]
        fast_model: Option<String>,
        #[arg(long)]
        heavy_model: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_url: Option<String>,
    },
    /// Interactive draft review
    Review,
    /// Lint health, stubs, alias normalize
    Maintain {
        #[arg(long)]
        fix: bool,
        #[arg(long)]
        stubs_only: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        clear_cache: bool,
        #[arg(long)]
        older_than: Option<i64>,
    },
    /// Clear rejection block on a concept
    Unblock { concept: String },
    /// Import PDF/md/text
    Add {
        source: PathBuf,
        #[arg(long, default_value = "notes")]
        r#type: String,
        #[arg(long)]
        force: bool,
    },
    /// Find published articles
    Find { query: String },
    /// Side-by-side challenger vs current
    Compare {
        #[arg(long)]
        fast_model: Option<String>,
        #[arg(long)]
        heavy_model: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_url: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    #[command(subcommand)]
    Pack(PackCmd),
    #[command(subcommand)]
    Report(ReportCmd),
    #[command(subcommand)]
    Vault(VaultCmd),
    #[command(subcommand)]
    Config(ConfigCmd),
    #[command(subcommand)]
    Items(ItemsCmd),
    #[command(subcommand)]
    Trace(TraceCmd),
    #[command(subcommand)]
    Concept(ConceptCmd),
}

#[derive(Subcommand)]
enum PackCmd {
    Export {
        #[arg(long)]
        target: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ReportCmd {
    /// Show vault analytics
    Show {
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Clear {
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum VaultCmd {
    /// List known vaults
    List,
    Use {
        path: PathBuf,
    },
    Forget {
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    InlineSourceCitations { value: String },
}

#[derive(Subcommand)]
enum ItemsCmd {
    Audit {
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Show {
        name: String,
    },
}

#[derive(Subcommand)]
enum TraceCmd {
    Article { name: String },
    Term { name: String },
    Relation { relation_id: String },
    Citation { segment_id: String },
}

#[derive(Subcommand)]
enum ConceptCmd {
    Rename {
        old: String,
        new: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value_t = true)]
        keep_old_alias: bool,
    },
    Merge {
        loser: String,
        winner: String,
        #[arg(long)]
        dry_run: bool,
    },
    Split {
        name: String,
    },
    Unmerge {
        name: String,
    },
    Inspect {
        name: String,
    },
    Keep {
        surface: String,
        entity: String,
    },
    #[command(subcommand)]
    Alias(AliasCmd),
}

#[derive(Subcommand)]
enum AliasCmd {
    Add {
        entity: String,
        alias: String,
    },
    Remove {
        entity: String,
        alias: String,
    },
    Move {
        from: String,
        to: String,
        alias: String,
    },
}

fn resolve_vault(cli_vault: Option<&Path>) -> Result<PathBuf> {
    if let Some(v) = cli_vault {
        return Ok(dunce(v));
    }
    if let Ok(v) = std::env::var(VAULT_ENV_VAR) {
        return Ok(dunce(Path::new(&v)));
    }
    if let Some(g) = load_global_config() {
        if let Some(v) = g.vault {
            return Ok(dunce(Path::new(&v)));
        }
    }
    let cwd = std::env::current_dir()?;
    let mut cur = Some(cwd.as_path());
    while let Some(p) = cur {
        if crate::paths::config_path(p).exists() || crate::paths::legacy_config_path(p).exists() {
            return Ok(p.to_path_buf());
        }
        cur = p.parent();
    }
    anyhow::bail!(
        "no vault specified. Use --vault, set {VAULT_ENV_VAR}, run `{CLI_NAME} setup`, or cd into a vault directory."
    )
}

fn dunce(p: &Path) -> PathBuf {
    let p = if p.starts_with("~") {
        directories::BaseDirs::new()
            .map(|b| b.home_dir().join(p.strip_prefix("~").unwrap_or(p)))
            .unwrap_or_else(|| p.to_path_buf())
    } else {
        p.to_path_buf()
    };
    std::fs::canonicalize(&p).unwrap_or(p)
}

fn load_cfg(cli_vault: Option<&Path>) -> Result<Config> {
    let vault = resolve_vault(cli_vault)?;
    if crate::paths::is_legacy_vault(&vault) {
        anyhow::bail!("{}", crate::paths::migration_message(&vault));
    }
    Config::from_vault(&vault).map_err(Into::into)
}

fn apply_overrides(
    cfg: &mut Config,
    fast: &Option<String>,
    heavy: &Option<String>,
    provider: &Option<String>,
    url: &Option<String>,
) {
    if let Some(f) = fast {
        let mut p = cfg.models.fast.as_profile();
        p.model = f.clone();
        cfg.models.fast = crate::config::RoleSpec::Profile(p);
    }
    if let Some(h) = heavy {
        let mut p = cfg.models.heavy.as_profile();
        p.model = h.clone();
        cfg.models.heavy = crate::config::RoleSpec::Profile(p);
    }
    cfg.provider_override = provider.clone();
    cfg.provider_override_url = url.clone();
}

fn open_db(cfg: &Config) -> Result<Arc<StateDb>> {
    Ok(Arc::new(StateDb::open(&cfg.state_db_path())?))
}

fn router(cfg: &Config, db: &Arc<StateDb>) -> Result<crate::llm::ModelRouter> {
    let cache = if cfg.cache.enabled {
        Some(crate::cache::LlmCache::new(db.clone()))
    } else {
        None
    };
    Ok(crate::llm::ModelRouter::build(cfg, cache)?)
}

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("synto=info".parse().unwrap()),
        )
        .with_target(false)
        .without_time()
        .init();
    match cli.command {
        Commands::Init {
            vault_path,
            existing,
            non_interactive,
            default,
        } => cmd_init(&vault_path, existing, non_interactive, default),
        Commands::MigrateOlw => cmd_migrate(cli.vault.as_deref()),
        Commands::Setup {
            non_interactive,
            reset,
            provider,
        } => cmd_setup(non_interactive, reset, provider),
        Commands::Ingest {
            all,
            force,
            paths,
            fast_model,
            heavy_model,
            provider,
            provider_url,
        } => {
            let mut cfg = load_cfg(cli.vault.as_deref())?;
            apply_overrides(
                &mut cfg,
                &fast_model,
                &heavy_model,
                &provider,
                &provider_url,
            );
            let db = open_db(&cfg)?;
            let router = router(&cfg, &db)?;
            router.require_healthy()?;
            let sel = if all || paths.is_empty() {
                None
            } else {
                Some(paths)
            };
            let results =
                crate::pipeline::ingest::ingest_all(&cfg, &router, &db, force, sel.as_deref())?;
            let n = results.iter().filter(|(_, r)| r.is_some()).count();
            println!("Ingested {n} notes");
            let _ = crate::indexer::generate_index(&cfg, &db);
            if cfg.pipeline.auto_commit {
                crate::git_ops::git_commit(&cfg.vault, "ingest", None);
            }
            Ok(())
        }
        Commands::Compile {
            dry_run,
            auto_approve,
            force,
            legacy,
            concept,
            retry_failed: _,
            fast_model,
            heavy_model,
            provider,
            provider_url,
        } => {
            let mut cfg = load_cfg(cli.vault.as_deref())?;
            apply_overrides(
                &mut cfg,
                &fast_model,
                &heavy_model,
                &provider,
                &provider_url,
            );
            let db = open_db(&cfg)?;
            let router = router(&cfg, &db)?;
            router.require_healthy()?;
            let only = if concept.is_empty() {
                None
            } else {
                Some(concept)
            };
            let (drafted, failed, _) = if legacy {
                let (d, f) = crate::pipeline::compile::compile_notes(&cfg, &router, &db, dry_run)?;
                (d, f, Default::default())
            } else {
                crate::pipeline::compile::compile_concepts(
                    &cfg,
                    &router,
                    &db,
                    force,
                    dry_run,
                    only.as_deref(),
                )?
            };
            println!(
                "Drafted {} articles ({} failed)",
                drafted.len(),
                failed.len()
            );
            if auto_approve || cfg.pipeline.auto_approve {
                crate::pipeline::compile::publish_drafts(&cfg, &db, None, "", 0.0)?;
            }
            Ok(())
        }
        Commands::Approve {
            all,
            min_confidence,
            files,
        } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let db = open_db(&cfg)?;
            let _lock = crate::lock::try_pipeline_lock(&cfg.vault)?;
            let paths = if all || files.is_empty() {
                None
            } else {
                Some(files)
            };
            let n = crate::pipeline::compile::publish_drafts(
                &cfg,
                &db,
                paths.as_deref(),
                "",
                min_confidence.unwrap_or(0.0),
            )?;
            println!("Published {} drafts", n.len());
            Ok(())
        }
        Commands::Verify {
            all,
            min_confidence,
            files,
        } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let db = open_db(&cfg)?;
            let paths = if all || files.is_empty() {
                None
            } else {
                Some(files)
            };
            let n = crate::pipeline::compile::verify_drafts(
                &cfg,
                &db,
                paths.as_deref(),
                min_confidence.unwrap_or(0.0),
            )?;
            println!("Verified {} drafts", n.len());
            Ok(())
        }
        Commands::Reject {
            all,
            feedback,
            files,
        } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let db = open_db(&cfg)?;
            let drafts = if all || files.is_empty() {
                crate::pipeline::compile::list_draft_paths(&cfg)?
            } else {
                files
            };
            let fb = feedback.unwrap_or_default();
            for d in &drafts {
                crate::pipeline::compile::reject_draft(d, &cfg, &db, &fb)?;
                println!("Rejected {}", d.display());
            }
            Ok(())
        }
        Commands::Status { failed } => cmd_status(cli.vault.as_deref(), failed),
        Commands::Eval { queries: _, json } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let db = open_db(&cfg)?;
            let lint = crate::pipeline::lint::run_lint(&cfg, &db, false)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&lint)?);
            } else {
                println!("health {:.0} — {}", lint.health_score, lint.summary);
            }
            Ok(())
        }
        Commands::Undo { steps, force: _ } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let reverted = crate::git_ops::git_undo(&cfg.vault, steps)?;
            if reverted.is_empty() {
                println!("Nothing to undo");
            } else {
                for m in reverted {
                    println!("Reverted {m}");
                }
            }
            Ok(())
        }
        Commands::Clean { yes } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            if !yes {
                anyhow::bail!("refusing to clean without --yes");
            }
            let _ = std::fs::remove_file(cfg.state_db_path());
            if cfg.wiki_dir().exists() {
                for e in std::fs::read_dir(cfg.wiki_dir())? {
                    let p = e?.path();
                    if p.file_name().and_then(|s| s.to_str()) == Some("index.md") {
                        continue;
                    }
                    let _ = std::fs::remove_dir_all(&p);
                    let _ = std::fs::remove_file(&p);
                }
            }
            println!("Cleaned wiki/ and state.db (raw/ preserved)");
            Ok(())
        }
        Commands::Support => {
            println!("Issues: {PROJECT_ISSUES_URL}");
            println!("Discussions: {PROJECT_DISCUSSIONS_URL}");
            println!("Repo: {PROJECT_REPO_URL}");
            Ok(())
        }
        Commands::Doctor {
            backlog: _,
            reconcile: _,
        } => cmd_doctor(cli.vault.as_deref()),
        Commands::Query {
            question,
            save,
            synthesize,
        } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let db = open_db(&cfg)?;
            let router = router(&cfg, &db)?;
            let res =
                crate::pipeline::query::run_query(&cfg, &router, &db, &question, save, synthesize)?;
            println!("{}", res.answer);
            if let Some(s) = res.save {
                println!("Saved {}", s.path.display());
            }
            Ok(())
        }
        Commands::Watch { auto_approve } => {
            let mut cfg = load_cfg(cli.vault.as_deref())?;
            if auto_approve {
                cfg.pipeline.auto_approve = true;
            }
            let debounce = std::time::Duration::from_secs_f64(cfg.pipeline.watch_debounce);
            println!("Watching {} …", cfg.raw_dir().display());
            crate::watcher::watch(cfg.clone(), debounce, move |paths| {
                let _lock = match crate::lock::try_pipeline_lock(&cfg.vault) {
                    Ok(Some(lock)) => lock,
                    Ok(None) => {
                        eprintln!("pipeline already running");
                        return;
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        return;
                    }
                };
                let db = match StateDb::open(&cfg.state_db_path()) {
                    Ok(d) => Arc::new(d),
                    Err(e) => {
                        eprintln!("{e}");
                        return;
                    }
                };
                let router = match crate::llm::ModelRouter::build(&cfg, None) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("{e}");
                        return;
                    }
                };
                let orch = crate::pipeline::orchestrator::PipelineOrchestrator {
                    config: cfg.clone(),
                    router,
                    db,
                };
                match orch.run(
                    Some(&paths),
                    cfg.pipeline.auto_approve,
                    false,
                    2,
                    false,
                    0.0,
                ) {
                    Ok(r) => println!(
                        "watch: ingested {} compiled {} published {}",
                        r.ingested, r.compiled, r.published
                    ),
                    Err(e) => eprintln!("{e}"),
                }
            })?;
            Ok(())
        }
        Commands::Serve {
            transport,
            name,
            host,
            port,
        } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            crate::serve::run_server(cfg, &transport, name.as_deref(), &host, port)
                .map_err(Into::into)
        }
        Commands::Run {
            auto_approve,
            fix,
            max_rounds,
            dry_run,
            min_confidence,
            fast_model,
            heavy_model,
            provider,
            provider_url,
        } => {
            let mut cfg = load_cfg(cli.vault.as_deref())?;
            apply_overrides(
                &mut cfg,
                &fast_model,
                &heavy_model,
                &provider,
                &provider_url,
            );
            let Some(_lock) = crate::lock::try_pipeline_lock(&cfg.vault)? else {
                anyhow::bail!("pipeline already running");
            };
            let db = open_db(&cfg)?;
            let router = router(&cfg, &db)?;
            router.require_healthy()?;
            let orch = crate::pipeline::orchestrator::PipelineOrchestrator {
                config: cfg,
                router,
                db,
            };
            let r = orch.run(
                None,
                auto_approve,
                fix,
                max_rounds,
                dry_run,
                min_confidence.unwrap_or(0.0),
            )?;
            println!(
                "ingested {}  compiled {}  published {}  health {:.0}",
                r.ingested, r.compiled, r.published, r.health_score
            );
            Ok(())
        }
        Commands::Review => cmd_review(cli.vault.as_deref()),
        Commands::Maintain {
            fix,
            stubs_only,
            dry_run,
            clear_cache,
            older_than,
        } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let db = open_db(&cfg)?;
            if clear_cache {
                let n = db.cache_clear(older_than)?;
                println!("Cleared {n} cache entries");
                return Ok(());
            }
            let lint = crate::pipeline::lint::run_lint(&cfg, &db, fix)?;
            let (visible, acked) =
                crate::pipeline::lint::partition_acked(lint.issues, &cfg.maintain.ack);
            println!(
                "health {:.0} — {} issues ({} acked)",
                lint.health_score,
                visible.len(),
                acked.len()
            );
            for i in &visible {
                println!("  [{}] {}: {}", i.issue_type, i.path, i.description);
            }
            if stubs_only || fix {
                let broken: Vec<_> = visible
                    .iter()
                    .filter(|i| i.issue_type == "broken_link")
                    .cloned()
                    .collect();
                let stubs = crate::pipeline::maintain::create_stubs(&cfg, &db, &broken, 5)?;
                println!("Created {} stubs", stubs.len());
            }
            let _ = dry_run;
            Ok(())
        }
        Commands::Unblock { concept } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let db = open_db(&cfg)?;
            if db.unblock_concept(&concept)? {
                println!("Unblocked {concept}");
            } else {
                println!("{concept} was not blocked");
            }
            Ok(())
        }
        Commands::Add {
            source,
            r#type,
            force,
        } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let raw = crate::extractors::import_source(&cfg, &source, &r#type, force)?;
            println!("Imported {} → {}", source.display(), raw.display());
            Ok(())
        }
        Commands::Find { query } => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let db = open_db(&cfg)?;
            let q = query.to_lowercase();
            for a in db.list_articles()? {
                if a.status == "published"
                    && (a.title.to_lowercase().contains(&q) || a.path.to_lowercase().contains(&q))
                {
                    println!("{}\t{}", a.title, a.path);
                }
            }
            Ok(())
        }
        Commands::Compare { .. } => {
            anyhow::bail!("compare: run two vault configs side-by-side via `synto run` on ephemeral copies; full advisor report is available in library crate::compare (coming with fixture suites)")
        }
        Commands::Pack(PackCmd::Export { target, out }) => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let r = crate::pack_export::export_pack(&cfg, &target, out)?;
            println!(
                "Exported {} articles to {}",
                r.n_articles,
                r.out_dir.display()
            );
            println!("Capabilities: {}", r.capabilities.join(", "));
            Ok(())
        }
        Commands::Report(cmd) => match cmd {
            ReportCmd::Show { since, json } => {
                let cfg = load_cfg(cli.vault.as_deref())?;
                let r = crate::stats::compute_stats(&cfg, since.as_deref())?;
                if json {
                    println!("{}", crate::stats::render_json(&r));
                } else {
                    print!("{}", crate::stats::render_text(&r));
                }
                Ok(())
            }
            ReportCmd::Clear { yes } => {
                let cfg = load_cfg(cli.vault.as_deref())?;
                if !cfg.state_db_path().exists() {
                    println!("No metrics data found (state.db does not exist).");
                    return Ok(());
                }
                if !yes {
                    anyhow::bail!("refusing to clear metrics without --yes");
                }
                let db = StateDb::open(&cfg.state_db_path())?;
                let n = db.clear_metrics()?;
                println!("Metrics cleared ({n} rows deleted).");
                Ok(())
            }
        },
        Commands::Vault(cmd) => {
            match cmd {
                VaultCmd::List => {
                    let known = load_known_vaults();
                    let default = load_global_config().and_then(|g| g.vault);
                    if known.is_empty() {
                        println!("No known vaults. Run `{CLI_NAME} vault use <path>` or `{CLI_NAME} init`.");
                    }
                    for v in known {
                        let mark = if default
                            .as_ref()
                            .map(|d| vault_key(Path::new(d)) == vault_key(Path::new(&v)))
                            .unwrap_or(false)
                        {
                            "*"
                        } else {
                            " "
                        };
                        println!("{mark} {v}");
                    }
                    Ok(())
                }
                VaultCmd::Use { path } => {
                    let p = dunce(&path);
                    register_known_vault(&p);
                    let mut g = load_global_config_strict()?.unwrap_or_default();
                    g.vault = Some(p.display().to_string());
                    save_global_config(&g)?;
                    println!("Default vault: {}", p.display());
                    Ok(())
                }
                VaultCmd::Forget { path } => {
                    match forget_known_vault(&path) {
                        ForgetResult::Removed => println!("Forgot {}", path.display()),
                        ForgetResult::Absent => println!("Not registered: {}", path.display()),
                        ForgetResult::Error => anyhow::bail!("failed to update vault registry"),
                    }
                    Ok(())
                }
            }
        }
        Commands::Config(ConfigCmd::InlineSourceCitations { value }) => {
            cmd_citations(cli.vault.as_deref(), &value)
        }
        Commands::Items(cmd) => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let db = open_db(&cfg)?;
            match cmd {
                ItemsCmd::Audit { limit } => {
                    for item in db
                        .list_items()?
                        .into_iter()
                        .filter(|i| i.kind != "concept")
                        .take(limit)
                    {
                        println!("{}\t{}\t{}", item.name, item.kind, item.status);
                    }
                    Ok(())
                }
                ItemsCmd::Show { name } => {
                    match db.get_item(&name)? {
                        Some(i) => {
                            println!(
                                "{} ({}) status={} confidence={:.2}",
                                i.name, i.kind, i.status, i.confidence
                            );
                            for m in db.get_item_mentions(&name)? {
                                println!("  {} @ {}", m.mention_text, m.source_path);
                            }
                        }
                        None => println!("not found"),
                    }
                    Ok(())
                }
            }
        }
        Commands::Trace(cmd) => {
            let cfg = load_cfg(cli.vault.as_deref())?;
            let db = open_db(&cfg)?;
            match cmd {
                TraceCmd::Article { name } => {
                    for a in db.list_articles()? {
                        if a.title == name || a.path.contains(&name) {
                            println!("{a:?}");
                        }
                    }
                    Ok(())
                }
                TraceCmd::Term { name } => {
                    println!("entity: {:?}", db.entity_id_for_name(&name)?);
                    println!("sources: {:?}", db.get_sources_for_concept(&name)?);
                    Ok(())
                }
                TraceCmd::Relation { relation_id } => {
                    println!("{:?}", db.get_relation(&relation_id)?);
                    println!("{:?}", db.list_relation_evidence(&relation_id)?);
                    Ok(())
                }
                TraceCmd::Citation { segment_id } => {
                    println!("{:?}", db.fetch_segment_by_id(&segment_id)?);
                    Ok(())
                }
            }
        }
        Commands::Concept(cmd) => cmd_concept(cli.vault.as_deref(), cmd),
    }
}

fn cmd_init(
    vault_path: &Path,
    existing: bool,
    non_interactive: bool,
    set_default: bool,
) -> Result<()> {
    let vault = dunce(vault_path);
    std::fs::create_dir_all(&vault)?;
    for d in [
        "raw",
        "wiki",
        "wiki/.drafts",
        "wiki/sources",
        ".synto",
        ".synto/chroma",
    ] {
        std::fs::create_dir_all(vault.join(d))?;
    }
    let schema = vault.join("vault-schema.md");
    if !schema.exists() {
        std::fs::write(
            &schema,
            "# Vault Schema\n\n## Folder Structure\n- `raw/` — input notes (immutable, never edited by synto)\n- `wiki/` — AI-synthesised articles (managed by synto)\n- `wiki/.drafts/` — pending human review\n\n## Note Format\nEvery wiki note has YAML frontmatter with: title, tags, sources, confidence, status, created, updated.\n\n## Links\nUse `[[Article Title]]` wikilinks between notes.\n",
        )?;
    }
    let index = vault.join("wiki").join("index.md");
    if !index.exists() {
        std::fs::write(&index, "---\ntitle: Index\ntags: [index]\nstatus: published\n---\n\n# Wiki Index\n\n_Updated automatically by synto._\n")?;
    }
    if existing && !non_interactive {
        println!("Adopted existing notes in {}", vault.display());
    }
    let toml_path = vault.join(CONFIG_FILE_NAME);
    let gcfg = load_global_config();
    if !toml_path.exists() {
        let provider_name = gcfg
            .as_ref()
            .and_then(|g| g.provider_name.clone())
            .unwrap_or_else(|| "ollama".into());
        let fast = gcfg
            .as_ref()
            .and_then(|g| g.fast_model.clone())
            .unwrap_or_else(|| "gemma4:e4b".into());
        let heavy = gcfg
            .as_ref()
            .and_then(|g| g.heavy_model.clone())
            .unwrap_or_else(|| "qwen2.5:14b".into());
        let ollama_url = gcfg
            .as_ref()
            .and_then(|g| g.ollama_url.clone())
            .unwrap_or_else(|| "http://localhost:11434".into());
        let provider_url = gcfg.as_ref().and_then(|g| g.provider_url.clone());
        std::fs::write(
            &toml_path,
            default_wiki_toml(
                &fast,
                &heavy,
                &ollama_url,
                &provider_name,
                provider_url.as_deref(),
                600.0,
                gcfg.as_ref().and_then(|g| g.azure_api_version.as_deref()),
                gcfg.as_ref()
                    .and_then(|g| g.experimental_inline_source_citations)
                    .unwrap_or(false),
            ),
        )?;
        if gcfg.is_none() {
            println!("No global config found — using Ollama defaults ({fast} @ {ollama_url}).");
            println!(
                "  Run {CLI_NAME} setup to configure your provider, or edit {CONFIG_FILE_NAME}."
            );
        }
    }
    crate::git_ops::git_init(&vault)?;
    let gi = vault.join(".gitignore");
    if !gi.exists() {
        std::fs::write(
            gi,
            ".DS_Store\n.synto/chroma/\n.synto/state.db\n.synto/compare/\n.synto/pipeline.lock\n.synto/exports/\n.obsidian/workspace.json\n*.log\n",
        )?;
    }
    register_known_vault(&vault);
    if set_default {
        let mut g = load_global_config_strict()?.unwrap_or_default();
        g.vault = Some(vault.display().to_string());
        save_global_config(&g)?;
        println!("Set as default vault — no --vault flag needed.");
    }
    println!("Vault initialised: {}", vault.display());
    println!("Next steps:");
    println!("  1. Drop .md notes into raw/");
    println!("  2. Run {CLI_NAME} run");
    println!("  3. Review drafts: {CLI_NAME} review");
    println!("  4. Publish all drafts: {CLI_NAME} approve --all");
    Ok(())
}

fn cmd_migrate(vault: Option<&Path>) -> Result<()> {
    let vault = resolve_vault(vault)?;
    let old_cfg = crate::paths::legacy_config_path(&vault);
    let new_cfg = crate::paths::config_path(&vault);
    if old_cfg.exists() && !new_cfg.exists() {
        std::fs::copy(&old_cfg, &new_cfg)?;
        let text = std::fs::read_to_string(&new_cfg)?.replace("[telemetry]", "[metrics]");
        std::fs::write(&new_cfg, text)?;
    }
    let old_dir = crate::paths::legacy_app_dir(&vault);
    let new_dir = crate::paths::app_dir(&vault);
    if old_dir.exists() && !new_dir.exists() {
        std::fs::rename(&old_dir, &new_dir)?;
    }
    println!("Migrated olw layout at {}", vault.display());
    Ok(())
}

fn cmd_setup(non_interactive: bool, reset: bool, provider: Option<String>) -> Result<()> {
    if reset {
        let path = crate::global_config::global_config_path();
        let _ = std::fs::remove_file(&path);
        println!("Removed {}", path.display());
    }
    let mut g = load_global_config().unwrap_or_default();
    if non_interactive {
        if let Some(p) = provider {
            g.provider_name = Some(p);
        }
        g.fast_model = g.fast_model.or(Some("gemma4:e4b".into()));
        g.heavy_model = g.heavy_model.or(Some("qwen2.5:14b".into()));
        save_global_config(&g)?;
        println!(
            "Wrote {}",
            crate::global_config::global_config_path().display()
        );
        return Ok(());
    }
    let providers = crate::providers::list_all_providers();
    let names: Vec<_> = providers.iter().map(|p| p.name.to_string()).collect();
    let idx = dialoguer::Select::new()
        .with_prompt("Provider")
        .items(&names)
        .default(0)
        .interact()?;
    let chosen = &providers[idx];
    g.provider_name = Some(chosen.name.to_string());
    g.provider_url = Some(chosen.default_url.to_string());
    g.fast_model = Some(
        dialoguer::Input::new()
            .with_prompt("Fast model")
            .default("gemma4:e4b".into())
            .interact()?,
    );
    g.heavy_model = Some(
        dialoguer::Input::new()
            .with_prompt("Heavy model")
            .default("qwen2.5:14b".into())
            .interact()?,
    );
    let vault: String = dialoguer::Input::new()
        .with_prompt("Default vault path (optional)")
        .allow_empty(true)
        .interact()?;
    if !vault.is_empty() {
        g.vault = Some(vault);
    }
    save_global_config(&g)?;
    println!(
        "Saved {}",
        crate::global_config::global_config_path().display()
    );
    Ok(())
}

fn cmd_status(vault: Option<&Path>, failed: bool) -> Result<()> {
    let cfg = load_cfg(vault)?;
    let db = open_db(&cfg)?;
    let raw = db.list_raw()?;
    let (drafts, verified, published) = db.count_articles_by_status()?;
    println!("Vault: {}", cfg.vault.display());
    println!(
        "  raw notes: {}  ingested: {}  failed: {}",
        raw.len(),
        raw.iter().filter(|r| r.status == "ingested").count(),
        raw.iter().filter(|r| r.status == "failed").count()
    );
    println!("  drafts: {drafts}  verified: {verified}  published: {published}");
    let blocked = db.list_blocked_concepts()?;
    if !blocked.is_empty() {
        println!("  blocked: {}", blocked.join(", "));
    }
    if let Some(pid) = crate::lock::lock_holder_pid(&cfg.vault) {
        println!("  pipeline lock held by pid {pid}");
    }
    if failed {
        for r in raw.into_iter().filter(|r| r.status == "failed") {
            println!("  FAIL {} — {}", r.path, r.error.unwrap_or_default());
        }
    }
    Ok(())
}

fn cmd_doctor(vault: Option<&Path>) -> Result<()> {
    let cfg = load_cfg(vault)?;
    println!("vault: {}", cfg.vault.display());
    println!(
        "config: {}",
        crate::paths::effective_config_path(&cfg.vault).display()
    );
    println!(
        "state db: {} ({})",
        cfg.state_db_path().display(),
        if cfg.state_db_path().exists() {
            "ok"
        } else {
            "missing"
        }
    );
    for role in ["fast", "heavy"] {
        match cfg.resolve_role(role) {
            Ok(r) => {
                let gap = crate::api_keys::credential_gap(
                    &r.provider_kind,
                    r.api_key.as_deref(),
                    r.api_key_env.as_deref(),
                    &r.url,
                    !r.headers.is_empty(),
                );
                println!(
                    "  {role}: {} @ {} {}",
                    r.model,
                    r.provider_kind,
                    if gap.is_some() {
                        "(missing credentials)"
                    } else {
                        ""
                    }
                );
            }
            Err(e) => println!("  {role}: {e}"),
        }
    }
    Ok(())
}

fn cmd_review(vault: Option<&Path>) -> Result<()> {
    let cfg = load_cfg(vault)?;
    let db = open_db(&cfg)?;
    let drafts = crate::pipeline::review::list_drafts(&cfg);
    if drafts.is_empty() {
        println!("No drafts");
        return Ok(());
    }
    for d in &drafts {
        println!(
            "[{:.2}] {} ({}) {}",
            d.confidence,
            d.title,
            d.status,
            d.path.display()
        );
        let choice = dialoguer::Select::new()
            .items(&["skip", "approve", "verify", "reject"])
            .default(0)
            .interact()?;
        match choice {
            1 => {
                crate::pipeline::compile::publish_drafts(
                    &cfg,
                    &db,
                    Some(std::slice::from_ref(&d.path)),
                    "",
                    0.0,
                )?;
            }
            2 => {
                crate::pipeline::compile::verify_drafts(
                    &cfg,
                    &db,
                    Some(std::slice::from_ref(&d.path)),
                    0.0,
                )?;
            }
            3 => {
                let fb: String = dialoguer::Input::new()
                    .with_prompt("feedback")
                    .allow_empty(true)
                    .interact()?;
                crate::pipeline::compile::reject_draft(&d.path, &cfg, &db, &fb)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn cmd_citations(vault: Option<&Path>, value: &str) -> Result<()> {
    let cfg = load_cfg(vault)?;
    let path = crate::paths::config_path(&cfg.vault);
    match value {
        "status" => {
            println!(
                "{}",
                if cfg.pipeline.inline_source_citations {
                    "on"
                } else {
                    "off"
                }
            );
        }
        "on" | "off" => {
            let enabled = value == "on";
            let mut text = std::fs::read_to_string(&path)?;
            if text.contains("inline_source_citations") {
                let re =
                    regex::Regex::new(r"(?m)^(\s*)#?\s*inline_source_citations\s*=.*$").unwrap();
                text = re
                    .replace(&text, format!("${{1}}inline_source_citations = {enabled}"))
                    .into_owned();
            } else if text.contains("[pipeline]") {
                text = text.replace(
                    "[pipeline]",
                    &format!("[pipeline]\ninline_source_citations = {enabled}"),
                );
            } else {
                text.push_str(&format!(
                    "\n[pipeline]\ninline_source_citations = {enabled}\n"
                ));
            }
            crate::vault::atomic_write(&path, &text)?;
            println!("inline_source_citations = {enabled}");
        }
        _ => anyhow::bail!("expected on|off|status"),
    }
    Ok(())
}

fn cmd_concept(vault: Option<&Path>, cmd: ConceptCmd) -> Result<()> {
    let cfg = load_cfg(vault)?;
    let db = open_db(&cfg)?;
    match cmd {
        ConceptCmd::Rename {
            old,
            new,
            dry_run,
            keep_old_alias,
        } => {
            let r = crate::pipeline::maintain::rename_concept(
                &cfg,
                &db,
                &old,
                &new,
                keep_old_alias,
                dry_run,
            )?;
            println!(
                "Renamed {} → {} ({} files)",
                r.old_name, r.new_name, r.files_rewritten
            );
        }
        ConceptCmd::Merge {
            loser,
            winner,
            dry_run,
        } => {
            crate::pipeline::maintain::merge_concepts(&cfg, &db, &loser, &winner, dry_run)?;
            println!("Merged {loser} → {winner}");
        }
        ConceptCmd::Split { name } => {
            println!("split is interactive in the Python release; create new entities with ingest + `concept keep` for {name}");
        }
        ConceptCmd::Unmerge { name } => {
            println!("unmerge of {name}: see identity log");
            for row in db.list_identity_log()? {
                println!("{row:?}");
            }
        }
        ConceptCmd::Inspect { name } => {
            let id = db.entity_id_for_name(&name)?;
            println!("entity_id: {id:?}");
            if let Some(id) = &id {
                println!("preferred: {:?}", db.preferred_label_for_entity(id)?);
                println!("aliases: {:?}", db.get_aliases(id)?);
                println!("sources: {:?}", db.get_sources_for_entity(id)?);
            }
        }
        ConceptCmd::Keep { surface, entity } => {
            println!("keep {surface} → {entity} (occurrences resolved on next ingest)");
        }
        ConceptCmd::Alias(AliasCmd::Add { entity, alias }) => {
            let id = db
                .entity_id_for_name(&entity)?
                .ok_or_else(|| anyhow::anyhow!("unknown entity {entity}"))?;
            db.add_alias(&id, &alias)?;
            println!("Added alias {alias} → {entity}");
        }
        ConceptCmd::Alias(AliasCmd::Remove { entity, alias }) => {
            let id = db
                .entity_id_for_name(&entity)?
                .ok_or_else(|| anyhow::anyhow!("unknown entity {entity}"))?;
            db.remove_alias(&id, &alias)?;
            crate::pipeline::maintain::unlink_alias_links(&cfg, &db, &alias, &entity)?;
            println!("Removed alias {alias}");
        }
        ConceptCmd::Alias(AliasCmd::Move { from, to, alias }) => {
            if let Some(id) = db.entity_id_for_name(&from)? {
                db.remove_alias(&id, &alias)?;
            }
            if let Some(id) = db.entity_id_for_name(&to)? {
                db.add_alias(&id, &alias)?;
            }
            println!("Moved alias {alias}: {from} → {to}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_debug_assert() {
        Cli::command().debug_assert();
    }
}
