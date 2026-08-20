use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),

    #[error(transparent)]
    Toml(#[from] toml::de::Error),

    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),

    #[error(transparent)]
    Notify(#[from] notify::Error),

    #[error("{0}")]
    Msg(String),

    #[error("{0}")]
    Config(String),

    #[error("{0}")]
    Llm(String),

    #[error("LLM output truncated ({provider}, finish_reason={finish_reason:?})")]
    LlmTruncated {
        provider: String,
        max_tokens: i64,
        completion_tokens: Option<i64>,
        finish_reason: Option<String>,
    },

    #[error("LLM request was rejected and is not retryable: {0}")]
    LlmBadRequest(String),

    #[error("structured output failed: {0}")]
    StructuredOutput(String),

    #[error("vault not found: {0}")]
    VaultNotFound(PathBuf),

    #[error("{0}")]
    Git(String),

    #[error("{0}")]
    Pipeline(String),

    #[error("synthesis insert conflict: {0}")]
    SynthesisConflict(String),

    #[error("working tree has uncommitted changes — commit or discard them before running undo.")]
    DirtyWorktree,
}

impl Error {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Msg(s.into())
    }

    pub fn config(s: impl Into<String>) -> Self {
        Self::Config(s.into())
    }

    pub fn llm(s: impl Into<String>) -> Self {
        Self::Llm(s.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
