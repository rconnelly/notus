use std::sync::Arc;

use crate::config::Config;
use crate::state::StateDb;
use crate::Result;

#[derive(Debug, Clone, serde::Serialize)]
pub struct StatsReport {
    pub vault: serde_json::Value,
    pub metrics: serde_json::Value,
}

pub fn parse_since(s: &str) -> Result<String> {
    if let Some(days) = s.strip_suffix('d').and_then(|n| n.parse::<i64>().ok()) {
        let dt = chrono::Local::now() - chrono::Duration::days(days);
        return Ok(dt.format("%Y-%m-%d").to_string());
    }
    if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
        return Ok(s.to_string());
    }
    Err(crate::Error::msg(format!("invalid --since value: {s}")))
}

pub fn compute_stats(config: &Config, since: Option<&str>) -> Result<StatsReport> {
    let db = Arc::new(StateDb::open(&config.state_db_path())?);
    let since_s = since.map(parse_since).transpose()?;
    let vault = db.stats()?;
    let totals = db.metric_event_totals(since_s.as_deref())?;
    Ok(StatsReport {
        vault,
        metrics: serde_json::json!({
            "calls": totals.0,
            "prompt_tokens": totals.1,
            "completion_tokens": totals.2,
            "latency_ms": totals.3,
        }),
    })
}

pub fn render_text(r: &StatsReport) -> String {
    format!(
        "Vault\n  raw notes: {}\n  articles: {}\n  concepts: {}\n\nMetrics\n  calls: {}\n  prompt tokens: {}\n  completion tokens: {}\n",
        r.vault.get("raw_notes").and_then(|v| v.as_i64()).unwrap_or(0),
        r.vault.get("articles").and_then(|v| v.as_i64()).unwrap_or(0),
        r.vault.get("concepts").and_then(|v| v.as_i64()).unwrap_or(0),
        r.metrics.get("calls").and_then(|v| v.as_i64()).unwrap_or(0),
        r.metrics.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        r.metrics.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
    )
}

pub fn render_json(r: &StatsReport) -> String {
    serde_json::to_string_pretty(r).unwrap_or_else(|_| "{}".into())
}
