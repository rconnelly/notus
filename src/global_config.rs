use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{ModelProfile, ProviderBlock};
use crate::paths::APP_NAME;
use crate::vault::atomic_write;
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConfig {
    #[serde(default)]
    pub vault: Option<String>,
    #[serde(default)]
    pub ollama_url: Option<String>,
    #[serde(default)]
    pub fast_model: Option<String>,
    #[serde(default)]
    pub heavy_model: Option<String>,
    #[serde(default)]
    pub provider_name: Option<String>,
    #[serde(default)]
    pub provider_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub azure_api_version: Option<String>,
    #[serde(default)]
    pub experimental_inline_source_citations: Option<bool>,
    #[serde(default)]
    pub providers: std::collections::HashMap<String, ProviderBlock>,
    #[serde(default)]
    pub models: Option<std::collections::HashMap<String, ModelProfile>>,
    #[serde(default)]
    pub provider_keys: Option<std::collections::HashMap<String, String>>,
}

impl GlobalConfig {
    pub fn is_multi_provider(&self) -> bool {
        !self.providers.is_empty() && self.models.as_ref().map(|m| !m.is_empty()).unwrap_or(false)
    }
}

pub fn global_config_path() -> PathBuf {
    if cfg!(windows) {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| {
            directories::BaseDirs::new()
                .map(|b| b.data_dir().to_string_lossy().into_owned())
                .unwrap_or_else(|| ".".into())
        });
        PathBuf::from(appdata).join(APP_NAME).join("config.toml")
    } else {
        let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
            directories::BaseDirs::new()
                .map(|b| b.home_dir().join(".config").to_string_lossy().into_owned())
                .unwrap_or_else(|| ".".into())
        });
        PathBuf::from(xdg).join(APP_NAME).join("config.toml")
    }
}

pub fn load_global_config() -> Option<GlobalConfig> {
    let path = global_config_path();
    if !path.exists() {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&text).ok()
}

pub fn load_global_config_strict() -> Result<Option<GlobalConfig>> {
    let path = global_config_path();
    if !path.exists() {
        return Ok(None);
    }
    load_global_config()
        .ok_or_else(|| Error::config(format!("unreadable global config: {}", path.display())))
        .map(Some)
}

pub fn save_global_config(cfg: &GlobalConfig) -> Result<()> {
    let path = global_config_path();
    let text = toml::to_string_pretty(cfg).map_err(|e| Error::config(e.to_string()))?;
    atomic_write(&path, &text)
}

fn known_vaults_path() -> PathBuf {
    global_config_path().with_file_name("vaults.toml")
}

pub fn vault_key(vault: &Path) -> String {
    let resolved = std::fs::canonicalize(vault).unwrap_or_else(|_| vault.to_path_buf());
    if cfg!(windows) {
        resolved.to_string_lossy().to_lowercase()
    } else {
        resolved.to_string_lossy().into_owned()
    }
}

fn read_known_vaults() -> (Vec<String>, bool) {
    let path = known_vaults_path();
    if !path.exists() {
        return (Vec::new(), false);
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => match text.parse::<toml::Value>() {
            Ok(val) => {
                let vaults = val.get("vaults").and_then(|v| v.as_array());
                match vaults {
                    Some(arr) => (
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect(),
                        false,
                    ),
                    None => (Vec::new(), true),
                }
            }
            Err(_) => (Vec::new(), true),
        },
        Err(_) => (Vec::new(), true),
    }
}

pub fn load_known_vaults() -> Vec<String> {
    read_known_vaults().0
}

pub fn save_known_vaults(paths: &[String]) -> Result<()> {
    let mut table = toml::map::Map::new();
    table.insert(
        "vaults".into(),
        toml::Value::Array(paths.iter().cloned().map(toml::Value::String).collect()),
    );
    let text = toml::to_string_pretty(&toml::Value::Table(table))
        .map_err(|e| Error::msg(e.to_string()))?;
    atomic_write(&known_vaults_path(), &text)
}

pub fn register_known_vault(vault: &Path) {
    let resolved = std::fs::canonicalize(vault)
        .unwrap_or_else(|_| vault.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let (vaults, malformed) = read_known_vaults();
    if vaults
        .iter()
        .any(|v| vault_key(Path::new(v)) == vault_key(Path::new(&resolved)))
    {
        return;
    }
    if malformed {
        let reg = known_vaults_path();
        let corrupt = reg.with_file_name(format!(
            "{}.corrupt",
            reg.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::rename(&reg, corrupt);
    }
    let mut next = vaults;
    next.push(resolved);
    let _ = save_known_vaults(&next);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgetResult {
    Removed,
    Absent,
    Error,
}

pub fn forget_known_vault(vault: &Path) -> ForgetResult {
    let key = vault_key(vault);
    let vaults = load_known_vaults();
    let kept: Vec<_> = vaults
        .into_iter()
        .filter(|v| vault_key(Path::new(v)) != key)
        .collect();
    if kept.len() == read_known_vaults().0.len() {
        return ForgetResult::Absent;
    }
    match save_known_vaults(&kept) {
        Ok(()) => ForgetResult::Removed,
        Err(_) => ForgetResult::Error,
    }
}
