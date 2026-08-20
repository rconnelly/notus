use std::sync::Arc;

use synto::config::Config;
use synto::llm::{MockClient, ModelRouter, RoleEndpoint};
use synto::pipeline::ingest::ingest_note;
use synto::state::StateDb;

fn mock_router(fast_payload: &str, heavy_payload: &str) -> ModelRouter {
    let mk = |payload: &str| RoleEndpoint {
        client: Box::new(MockClient::new(vec![payload.to_string()])),
        model: "mock".into(),
        ctx: 2048,
        think: None,
        temperature: None,
    };
    ModelRouter::from_parts(mk(fast_payload), mk(heavy_payload), mk(fast_payload))
}

#[test]
fn ingest_note_writes_concepts_and_source_page() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path();
    for d in ["raw", "wiki", "wiki/.drafts", "wiki/sources", ".synto"] {
        std::fs::create_dir_all(vault.join(d)).unwrap();
    }
    std::fs::write(
        vault.join("synto.toml"),
        synto::config::default_wiki_toml(
            "mock",
            "mock",
            "http://127.0.0.1:9",
            "ollama",
            None,
            5.0,
            None,
            false,
        ),
    )
    .unwrap();
    std::fs::write(
        vault.join("raw/qubit.md"),
        "# Qubit\n\nA qubit is the basic unit of quantum information.\n",
    )
    .unwrap();

    let cfg = Config::from_vault(vault).unwrap();
    let db = Arc::new(StateDb::open(&cfg.state_db_path()).unwrap());
    let analysis_payload = r#"{
        "summary": "Introduces the qubit.",
        "concepts": [{"name": "Qubit", "aliases": ["quantum bit"]}],
        "suggested_topics": ["Quantum computing"],
        "named_references": [],
        "quality": "high",
        "language": "en"
    }"#;
    let article_payload = r#"{
        "title": "Qubit",
        "content": "A [[Qubit]] is the basic unit of quantum information.",
        "tags": ["quantum"]
    }"#;
    let router = mock_router(analysis_payload, article_payload);
    let analysis = ingest_note(&vault.join("raw/qubit.md"), &cfg, &router, &db, &[], false)
        .unwrap()
        .expect("analysis");
    assert_eq!(analysis.concepts[0].name, "Qubit");
    let rec = db.get_raw("raw/qubit.md").unwrap().expect("raw row");
    assert_eq!(rec.status, "ingested");
    assert!(vault.join("wiki/sources/qubit.md").is_file());

    let (drafted, failed, _) =
        synto::pipeline::compile::compile_concepts(&cfg, &router, &db, false, false, None).unwrap();
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(drafted.len(), 1);
    let published = synto::pipeline::compile::publish_drafts(&cfg, &db, None, "", 0.0).unwrap();
    assert_eq!(published.len(), 1);
    assert!(vault.join("wiki").join("Qubit.md").is_file() || published[0].exists());
}

#[test]
fn lint_reports_stale_lock_on_fresh_vault() {
    let tmp = tempfile::tempdir().unwrap();
    let vault = tmp.path();
    for d in ["raw", "wiki", "wiki/.drafts", ".synto"] {
        std::fs::create_dir_all(vault.join(d)).unwrap();
    }
    std::fs::write(
        vault.join("synto.toml"),
        synto::config::default_wiki_toml(
            "mock",
            "mock",
            "http://127.0.0.1:9",
            "ollama",
            None,
            5.0,
            None,
            false,
        ),
    )
    .unwrap();
    std::fs::write(vault.join(".synto/pipeline.lock"), "nope").unwrap();
    let cfg = Config::from_vault(vault).unwrap();
    let db = Arc::new(StateDb::open(&cfg.state_db_path()).unwrap());
    let lint = synto::pipeline::lint::run_lint(&cfg, &db, false).unwrap();
    assert!(lint.issues.iter().any(|i| i.issue_type == "stale_lock"));
}
