use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::config::Config;
use crate::Result;

pub fn watch(
    config: Config,
    debounce: Duration,
    mut on_event: impl FnMut(Vec<PathBuf>),
) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())?;
    watcher.watch(&config.raw_dir(), RecursiveMode::Recursive)?;
    let pending: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
    loop {
        match rx.recv_timeout(debounce) {
            Ok(Ok(event)) => {
                let mut paths: Vec<PathBuf> = event
                    .paths
                    .into_iter()
                    .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
                    .collect();
                if !paths.is_empty() {
                    pending.lock().unwrap().append(&mut paths);
                }
            }
            Ok(Err(e)) => tracing::warn!("watch error: {e}"),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let batch = {
                    let mut p = pending.lock().unwrap();
                    std::mem::take(&mut *p)
                };
                if !batch.is_empty() {
                    on_event(batch);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}
