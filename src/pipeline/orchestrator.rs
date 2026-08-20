use std::sync::Arc;

use crate::config::Config;
use crate::llm::ModelRouter;
use crate::pipeline::compile::{compile_concepts, publish_drafts};
use crate::pipeline::ingest::ingest_all;
use crate::pipeline::lint::run_lint;
use crate::state::StateDb;
use crate::Result;

#[derive(Debug, Default)]
pub struct PipelineReport {
    pub ingested: usize,
    pub compiled: usize,
    pub published: usize,
    pub health_score: f64,
    pub failed: Vec<String>,
}

pub struct PipelineOrchestrator {
    pub config: Config,
    pub router: ModelRouter,
    pub db: Arc<StateDb>,
}

impl PipelineOrchestrator {
    pub fn run(
        &self,
        paths: Option<&[std::path::PathBuf]>,
        auto_approve: bool,
        fix: bool,
        max_rounds: u32,
        dry_run: bool,
        min_confidence: f64,
    ) -> Result<PipelineReport> {
        let mut report = PipelineReport::default();
        let ingested = ingest_all(&self.config, &self.router, &self.db, false, paths)?;
        report.ingested = ingested.iter().filter(|(_, r)| r.is_some()).count();
        let mut rounds = 0;
        while rounds < max_rounds {
            rounds += 1;
            let (drafted, failed, _) =
                compile_concepts(&self.config, &self.router, &self.db, false, dry_run, None)?;
            report.compiled += drafted.len();
            report.failed.extend(failed);
            if drafted.is_empty() {
                break;
            }
        }
        let lint = run_lint(&self.config, &self.db, fix)?;
        report.health_score = lint.health_score;
        if (auto_approve || self.config.pipeline.auto_approve) && !dry_run {
            let published = publish_drafts(&self.config, &self.db, None, "", min_confidence)?;
            report.published = published.len();
        }
        if self.config.pipeline.auto_commit {
            crate::git_ops::git_commit(&self.config.vault, "pipeline run", None);
        }
        Ok(report)
    }
}
