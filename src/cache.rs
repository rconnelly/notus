use crate::hashing::sha256_hex;
use crate::state::StateDb;
use crate::Result;

#[derive(Clone)]
pub struct LlmCache {
    db: std::sync::Arc<StateDb>,
}

impl LlmCache {
    pub fn new(db: std::sync::Arc<StateDb>) -> Self {
        Self { db }
    }

    pub fn key(namespace: &str, model: &str, messages: &serde_json::Value) -> String {
        let data = format!(
            "{namespace}\0{model}{}",
            serde_json::to_string(messages).unwrap_or_default()
        );
        sha256_hex(data.as_bytes())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        self.db.cache_get(key)
    }

    pub fn put(&self, key: &str, model: &str, response: &str) -> Result<()> {
        self.db.cache_put(key, model, response)
    }

    pub fn clear(&self, older_than_days: Option<i64>) -> Result<usize> {
        self.db.cache_clear(older_than_days)
    }

    pub fn stats(&self) -> Result<(i64, i64, f64)> {
        self.db.cache_stats()
    }
}
