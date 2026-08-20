use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::api_keys::resolve_api_key;
use crate::hashing::sha256_hex;
use crate::paths::{effective_config_path, APP_DIR_NAME, LEGACY_CONFIG_FILE_NAME};
use crate::providers::get_provider;
use crate::{Error, Result};

pub const ROLES: [&str; 3] = ["fast", "heavy", "embed"];
pub const HEALTHCHECK_ROLES: [&str; 2] = ["fast", "heavy"];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderBlock {
    #[serde(default = "default_ollama")]
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub timeout: Option<f64>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default = "default_azure_ver")]
    pub azure_api_version: String,
    #[serde(default)]
    pub options: HashMap<String, toml::Value>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

fn default_ollama() -> String {
    "ollama".into()
}
fn default_azure_ver() -> String {
    "2024-02-15-preview".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProfile {
    pub model: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub ctx: Option<u32>,
    #[serde(default)]
    pub think: Option<bool>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub options: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RoleSpec {
    Name(String),
    Profile(ModelProfile),
}

impl Default for RoleSpec {
    fn default() -> Self {
        Self::Name("gemma4:e4b".into())
    }
}

impl RoleSpec {
    pub fn as_profile(&self) -> ModelProfile {
        match self {
            Self::Name(m) => ModelProfile {
                model: m.clone(),
                provider: None,
                ctx: None,
                think: None,
                temperature: None,
                options: HashMap::new(),
            },
            Self::Profile(p) => p.clone(),
        }
    }
    pub fn model_name(&self) -> String {
        self.as_profile().model
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsConfig {
    #[serde(default = "default_fast")]
    pub fast: RoleSpec,
    #[serde(default = "default_heavy")]
    pub heavy: RoleSpec,
    #[serde(default = "default_embed")]
    pub embed: RoleSpec,
}

fn default_fast() -> RoleSpec {
    RoleSpec::Name("gemma4:e4b".into())
}
fn default_heavy() -> RoleSpec {
    RoleSpec::Name("qwen2.5:14b".into())
}
fn default_embed() -> RoleSpec {
    RoleSpec::Name("nomic-embed-text".into())
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            fast: default_fast(),
            heavy: default_heavy(),
            embed: default_embed(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_url")]
    pub url: String,
    #[serde(default = "default_timeout")]
    pub timeout: f64,
    #[serde(default = "default_fast_ctx")]
    pub fast_ctx: u32,
    #[serde(default = "default_heavy_ctx")]
    pub heavy_ctx: u32,
}

fn default_ollama_url() -> String {
    "http://localhost:11434".into()
}
fn default_timeout() -> f64 {
    600.0
}
fn default_fast_ctx() -> u32 {
    16384
}
fn default_heavy_ctx() -> u32 {
    32768
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            url: default_ollama_url(),
            timeout: default_timeout(),
            fast_ctx: default_fast_ctx(),
            heavy_ctx: default_heavy_ctx(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default = "default_ollama")]
    pub name: String,
    #[serde(default = "default_ollama_url")]
    pub url: String,
    #[serde(default = "default_timeout")]
    pub timeout: f64,
    #[serde(default = "default_fast_ctx")]
    pub fast_ctx: u32,
    #[serde(default = "default_heavy_ctx")]
    pub heavy_ctx: u32,
    #[serde(default = "default_azure_ver")]
    pub azure_api_version: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            name: default_ollama(),
            url: default_ollama_url(),
            timeout: default_timeout(),
            fast_ctx: default_fast_ctx(),
            heavy_ctx: default_heavy_ctx(),
            azure_api_version: default_azure_ver(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourceTypeOverride {
    pub max_concepts_per_source: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    #[serde(default)]
    pub auto_approve: bool,
    #[serde(default = "default_true")]
    pub auto_commit: bool,
    #[serde(default = "default_debounce")]
    pub watch_debounce: f64,
    #[serde(default = "default_max_concepts")]
    pub max_concepts_per_source: u32,
    #[serde(default)]
    pub source_overrides: HashMap<String, SourceTypeOverride>,
    #[serde(default)]
    pub auto_maintain: bool,
    #[serde(default)]
    pub ingest_parallel: bool,
    #[serde(default)]
    pub relation_extraction: bool,
    #[serde(default = "default_article_tokens")]
    pub article_max_tokens: u32,
    #[serde(default = "default_soft_cap")]
    pub concept_draft_soft_cap: toml::Value,
    #[serde(default)]
    pub inline_source_citations: bool,
    #[serde(default = "default_citation_style")]
    pub source_citation_style: String,
    #[serde(default = "default_draft_media")]
    pub draft_media: String,
    #[serde(default = "default_true")]
    pub graph_quality_checks: bool,
    #[serde(default)]
    pub language: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_debounce() -> f64 {
    3.0
}
fn default_max_concepts() -> u32 {
    8
}
fn default_article_tokens() -> u32 {
    16384
}
fn default_soft_cap() -> toml::Value {
    toml::Value::Integer(2400)
}
fn default_citation_style() -> String {
    "legend-only".into()
}
fn default_draft_media() -> String {
    "reference".into()
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            auto_approve: false,
            auto_commit: true,
            watch_debounce: 3.0,
            max_concepts_per_source: 8,
            source_overrides: HashMap::new(),
            auto_maintain: false,
            ingest_parallel: false,
            relation_extraction: false,
            article_max_tokens: 16384,
            concept_draft_soft_cap: toml::Value::Integer(2400),
            inline_source_citations: false,
            source_citation_style: "legend-only".into(),
            draft_media: "reference".into(),
            graph_quality_checks: true,
            language: None,
        }
    }
}

impl PipelineConfig {
    pub fn concept_soft_cap_tokens(&self) -> u32 {
        match &self.concept_draft_soft_cap {
            toml::Value::String(s) if s == "article_max_tokens" => self.article_max_tokens,
            toml::Value::Integer(n) => *n as u32,
            _ => 2400,
        }
    }

    pub fn max_concepts_for(&self, source_type: &str) -> u32 {
        if let Some(over) = self.source_overrides.get(source_type) {
            if let Some(n) = over.max_concepts_per_source {
                return n;
            }
        }
        match source_type {
            "textbook" => 25,
            "paper" => 15,
            _ => self.max_concepts_per_source,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagConfig {
    #[serde(default = "default_chunk")]
    pub chunk_size: u32,
    #[serde(default = "default_overlap")]
    pub chunk_overlap: u32,
    #[serde(default = "default_sim")]
    pub similarity_threshold: f64,
}
fn default_chunk() -> u32 {
    512
}
fn default_overlap() -> u32 {
    50
}
fn default_sim() -> f64 {
    0.7
}
impl Default for RagConfig {
    fn default() -> Self {
        Self {
            chunk_size: 512,
            chunk_overlap: 50,
            similarity_threshold: 0.7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    #[serde(default = "default_true")]
    pub persist: bool,
    #[serde(default)]
    pub detailed: bool,
    #[serde(default = "default_retention")]
    pub retention_days: u32,
    #[serde(default = "default_max_mb")]
    pub max_size_mb: u32,
    #[serde(default = "default_true")]
    pub hash_source_ids: bool,
}
fn default_retention() -> u32 {
    90
}
fn default_max_mb() -> u32 {
    100
}
impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            persist: true,
            detailed: false,
            retention_days: 90,
            max_size_mb: 100,
            hash_source_ids: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSourceAccessConfig {
    #[serde(default = "default_perm")]
    pub mode: String,
    #[serde(default = "default_licenses")]
    pub permissive_licenses: Vec<String>,
}
fn default_perm() -> String {
    "permissive_only".into()
}
fn default_licenses() -> Vec<String> {
    vec![
        "CC-BY".into(),
        "CC-BY-SA".into(),
        "MIT".into(),
        "Apache-2.0".into(),
        "BSD-3-Clause".into(),
        "public-domain".into(),
    ]
}
impl Default for McpSourceAccessConfig {
    fn default() -> Self {
        Self {
            mode: default_perm(),
            permissive_licenses: default_licenses(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_public")]
    pub default_visibility: String,
    #[serde(default)]
    pub exclude_tags: Vec<String>,
    #[serde(default)]
    pub audit: bool,
    #[serde(default)]
    pub audit_detailed: bool,
    #[serde(default)]
    pub source_access: McpSourceAccessConfig,
}
fn default_public() -> String {
    "public".into()
}
impl Default for McpConfig {
    fn default() -> Self {
        Self {
            default_visibility: default_public(),
            exclude_tags: Vec::new(),
            audit: false,
            audit_detailed: false,
            source_access: McpSourceAccessConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MaintainConfig {
    #[serde(default)]
    pub ack: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub provider_kind: String,
    pub url: String,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub timeout: f64,
    pub model: String,
    pub ctx: u32,
    pub think: Option<bool>,
    pub temperature: Option<f32>,
    pub supports_json_mode: bool,
    pub supports_embeddings: bool,
    pub azure: bool,
    pub azure_api_version: String,
    pub anthropic_compat: bool,
    pub options: HashMap<String, toml::Value>,
    pub headers: HashMap<String, String>,
}

impl ResolvedModel {
    pub fn connection_key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{:?}",
            self.provider_kind,
            self.url,
            self.api_key.as_deref().unwrap_or(""),
            self.timeout,
            self.azure,
            self.azure_api_version,
            {
                let mut h: Vec<_> = self.headers.iter().collect();
                h.sort_by(|a, b| a.0.cmp(b.0));
                h
            }
        )
    }

    pub fn cache_namespace(&self) -> String {
        let ident = serde_json::json!([
            &self.provider_kind,
            &self.url,
            self.api_key.as_deref().unwrap_or(""),
            &self.azure_api_version,
        ]);
        format!(
            "{}:{}",
            self.provider_kind,
            sha256_hex(ident.to_string().as_bytes())
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub models: ModelsConfig,
    #[serde(default)]
    pub ollama: OllamaConfig,
    #[serde(default)]
    pub provider: Option<ProviderConfig>,
    #[serde(default)]
    pub providers: HashMap<String, ProviderBlock>,
    #[serde(default)]
    pub pipeline: PipelineConfig,
    #[serde(default)]
    pub rag: RagConfig,
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub maintain: MaintainConfig,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub vault: PathBuf,
    pub models: ModelsConfig,
    pub ollama: OllamaConfig,
    pub provider: Option<ProviderConfig>,
    pub providers: HashMap<String, ProviderBlock>,
    pub provider_override: Option<String>,
    pub provider_override_url: Option<String>,
    pub pipeline: PipelineConfig,
    pub rag: RagConfig,
    pub metrics: MetricsConfig,
    pub mcp: McpConfig,
    pub cache: CacheConfig,
    pub maintain: MaintainConfig,
}

impl Config {
    pub fn raw_dir(&self) -> PathBuf {
        self.vault.join("raw")
    }
    pub fn wiki_dir(&self) -> PathBuf {
        self.vault.join("wiki")
    }
    pub fn drafts_dir(&self) -> PathBuf {
        self.vault.join("wiki").join(".drafts")
    }
    pub fn app_dir(&self) -> PathBuf {
        self.vault.join(APP_DIR_NAME)
    }
    pub fn state_db_path(&self) -> PathBuf {
        self.app_dir().join("state.db")
    }
    pub fn chroma_dir(&self) -> PathBuf {
        self.app_dir().join("chroma")
    }
    pub fn sources_dir(&self) -> PathBuf {
        self.vault.join("wiki").join("sources")
    }
    pub fn queries_dir(&self) -> PathBuf {
        self.vault.join("wiki").join("queries")
    }
    pub fn synthesis_dir(&self) -> PathBuf {
        self.vault.join("wiki").join("synthesis")
    }
    pub fn schema_path(&self) -> PathBuf {
        self.vault.join("vault-schema.md")
    }

    pub fn effective_provider(&self) -> ProviderConfig {
        if let Some(p) = &self.provider {
            return p.clone();
        }
        ProviderConfig {
            name: "ollama".into(),
            url: self.ollama.url.clone(),
            timeout: self.ollama.timeout,
            fast_ctx: self.ollama.fast_ctx,
            heavy_ctx: self.ollama.heavy_ctx,
            azure_api_version: default_azure_ver(),
        }
    }

    pub fn model_name(&self, role: &str) -> String {
        match role {
            "heavy" => self.models.heavy.model_name(),
            "embed" => self.models.embed.model_name(),
            _ => self.models.fast.model_name(),
        }
    }

    pub fn resolve_role(&self, role: &str) -> Result<ResolvedModel> {
        self.resolve_role_with_key(role, None)
    }

    pub fn resolve_role_with_key(
        &self,
        role: &str,
        api_key_env: Option<&str>,
    ) -> Result<ResolvedModel> {
        let profile = match role {
            "heavy" => self.models.heavy.as_profile(),
            "embed" => self.models.embed.as_profile(),
            _ => self.models.fast.as_profile(),
        };
        let mut alias = profile.provider.clone();
        let (
            mut kind,
            mut url,
            mut timeout,
            mut block_api_key_env,
            mut azure_api_version,
            mut options,
            mut headers,
        ) = if let Some(a) = alias.as_deref() {
            let block = self.providers.get(a).ok_or_else(|| {
                    Error::config(format!("[models.{role}] references provider '{a}', which is not defined under [providers.*]"))
                })?;
            (
                block.name.clone(),
                block.url.clone(),
                block.timeout,
                block.api_key_env.clone(),
                block.azure_api_version.clone(),
                {
                    let mut o = block.options.clone();
                    o.extend(profile.options.clone());
                    o
                },
                block.headers.clone(),
            )
        } else if let Some(block) = self.providers.get("default") {
            alias = Some("default".into());
            (
                block.name.clone(),
                block.url.clone(),
                block.timeout,
                block.api_key_env.clone(),
                block.azure_api_version.clone(),
                {
                    let mut o = block.options.clone();
                    o.extend(profile.options.clone());
                    o
                },
                block.headers.clone(),
            )
        } else {
            let legacy = self.effective_provider();
            (
                legacy.name,
                Some(legacy.url),
                Some(legacy.timeout),
                None,
                legacy.azure_api_version,
                profile.options.clone(),
                HashMap::new(),
            )
        };

        if let Some(over) = &self.provider_override {
            alias = None;
            kind = over.clone();
            url = self.provider_override_url.clone();
            timeout = None;
            block_api_key_env = None;
            azure_api_version = default_azure_ver();
            options = profile.options.clone();
            headers = HashMap::new();
        } else if let Some(u) = &self.provider_override_url {
            url = Some(u.clone());
        }

        let prov_info = get_provider(&kind);
        if url.as_deref().unwrap_or("").is_empty() {
            url = Some(
                prov_info
                    .as_ref()
                    .map(|p| p.default_url.to_string())
                    .unwrap_or_default(),
            );
        }
        let timeout = timeout.unwrap_or_else(|| {
            prov_info
                .as_ref()
                .map(|p| p.default_timeout)
                .unwrap_or(600.0)
        });
        let ctx = profile.ctx.unwrap_or_else(|| {
            let prov = self.effective_provider();
            if role == "heavy" {
                prov.heavy_ctx
            } else {
                prov.fast_ctx
            }
        });
        let think = if profile.think.is_none() && role == "fast" {
            Some(false)
        } else {
            profile.think
        };

        let gcfg = crate::global_config::load_global_config();
        let api_key = resolve_api_key(
            &kind,
            alias.as_deref(),
            block_api_key_env.as_deref(),
            api_key_env,
            gcfg.as_ref().and_then(|g| g.provider_keys.as_ref()),
            gcfg.as_ref().and_then(|g| g.api_key.as_deref()),
        );

        Ok(ResolvedModel {
            provider_kind: kind,
            url: url.unwrap_or_default(),
            api_key,
            api_key_env: block_api_key_env,
            timeout,
            model: profile.model,
            ctx,
            think,
            temperature: profile.temperature,
            supports_json_mode: prov_info
                .as_ref()
                .map(|p| p.supports_json_mode)
                .unwrap_or(true),
            supports_embeddings: prov_info
                .as_ref()
                .map(|p| p.supports_embeddings)
                .unwrap_or(false),
            azure: prov_info.as_ref().map(|p| p.azure).unwrap_or(false),
            azure_api_version,
            anthropic_compat: prov_info
                .as_ref()
                .map(|p| p.anthropic_compat)
                .unwrap_or(false),
            options,
            headers,
        })
    }

    pub fn from_vault(vault: &Path) -> Result<Self> {
        Self::from_vault_overrides(vault, None)
    }

    pub fn from_vault_overrides(vault: &Path, overrides: Option<toml::Value>) -> Result<Self> {
        let vault = dunce_canonicalize(vault);
        let config_file = effective_config_path(&vault);
        if !config_file.exists() && vault.join(LEGACY_CONFIG_FILE_NAME).exists() {
            return Err(Error::config(format!(
                "Legacy vault config found at {}; run `synto migrate-olw --vault {}` first.",
                vault.join(LEGACY_CONFIG_FILE_NAME).display(),
                vault.display()
            )));
        }
        let mut file_config = if config_file.exists() {
            let bytes = std::fs::read(&config_file)?;
            let text = String::from_utf8(bytes).map_err(|_| {
                Error::config(format!(
                    "{} is not valid UTF-8. Re-save the file as UTF-8, or delete it and re-run `synto init <vault> --existing`.",
                    config_file.display()
                ))
            })?;
            text.parse::<toml::Value>()
                .map_err(|e| Error::config(e.to_string()))?
        } else {
            toml::Value::Table(toml::map::Map::new())
        };
        if file_config.get("telemetry").is_some() {
            return Err(Error::config(format!(
                "Legacy [telemetry] config found in {}; rename it to [metrics] or run `synto migrate-olw`.",
                config_file.display()
            )));
        }
        if let Some(toml::Value::Table(over)) = overrides {
            if let toml::Value::Table(root) = &mut file_config {
                merge_toml(root, over);
            }
        }
        let parsed: ConfigFile = file_config
            .try_into()
            .map_err(|e: toml::de::Error| Error::config(e.to_string()))?;
        let cfg = Config {
            vault,
            models: parsed.models,
            ollama: parsed.ollama,
            provider: parsed.provider,
            providers: parsed.providers,
            provider_override: None,
            provider_override_url: None,
            pipeline: parsed.pipeline,
            rag: parsed.rag,
            metrics: parsed.metrics,
            mcp: parsed.mcp,
            cache: parsed.cache,
            maintain: parsed.maintain,
        };
        for role in ROLES {
            let prof = match role {
                "heavy" => cfg.models.heavy.as_profile(),
                "embed" => cfg.models.embed.as_profile(),
                _ => cfg.models.fast.as_profile(),
            };
            if let Some(p) = &prof.provider {
                if !cfg.providers.contains_key(p) {
                    return Err(Error::config(format!(
                        "[models.{role}] references provider '{p}', which is not defined under [providers.*]"
                    )));
                }
            }
        }
        for (alias, block) in &cfg.providers {
            if get_provider(&block.name).is_none() && block.url.as_deref().unwrap_or("").is_empty()
            {
                return Err(Error::config(format!(
                    "[providers.{alias}] has unknown provider name '{}' and no url",
                    block.name
                )));
            }
        }
        Ok(cfg)
    }
}

fn dunce_canonicalize(path: &Path) -> PathBuf {
    let expanded = if path.starts_with("~") {
        if let Some(home) = directories::BaseDirs::new() {
            home.home_dir().join(path.strip_prefix("~").unwrap_or(path))
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };
    std::fs::canonicalize(&expanded).unwrap_or_else(|_| {
        if expanded.is_absolute() {
            expanded
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(expanded)
        }
    })
}

fn merge_toml(
    dst: &mut toml::map::Map<String, toml::Value>,
    src: toml::map::Map<String, toml::Value>,
) {
    for (k, v) in src {
        match (dst.get_mut(&k), v) {
            (Some(toml::Value::Table(d)), toml::Value::Table(s)) => merge_toml(d, s),
            (_, v) => {
                dst.insert(k, v);
            }
        }
    }
}

fn toml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

pub fn vault_toml_tail(inline_source_citations: bool) -> String {
    let citation_line = if inline_source_citations {
        "inline_source_citations = true  # Experimental: add inline source links\n"
    } else {
        "# inline_source_citations = false  # Experimental: add inline source links\n"
    };
    format!(
        "[pipeline]\n\
auto_approve = false\n\
auto_commit = true\n\
auto_maintain = false\n\
watch_debounce = 3.0\n\
max_concepts_per_source = 8\n\
ingest_parallel = false   # true = parallel chunks\n\
# relation_extraction = false\n\
article_max_tokens = 16384\n\
concept_draft_soft_cap = 2400\n\
{citation_line}\
# source_citation_style = \"legend-only\"\n\
# draft_media = \"reference\"\n\
graph_quality_checks = true\n\
# language = \"en\"\n"
    )
}

pub fn default_wiki_toml(
    fast_model: &str,
    heavy_model: &str,
    ollama_url: &str,
    provider_name: &str,
    provider_url: Option<&str>,
    provider_timeout: f64,
    azure_api_version: Option<&str>,
    inline_source_citations: bool,
) -> String {
    let (url, fast_ctx, heavy_ctx, timeout_int) = if provider_name == "ollama" {
        (provider_url.unwrap_or(ollama_url), 16384, 32768, 600)
    } else {
        (
            provider_url.unwrap_or(""),
            8192,
            32768,
            provider_timeout as i64,
        )
    };
    let mut provider_lines = vec![
        "[providers.default]".into(),
        format!("name = {}", toml_quote(provider_name)),
        format!("url = {}", toml_quote(url)),
        format!("timeout = {timeout_int}"),
    ];
    if provider_name == "azure" {
        let api_ver = azure_api_version.unwrap_or("2024-02-15-preview");
        provider_lines.push(format!("azure_api_version = {}", toml_quote(api_ver)));
    }
    if provider_name != "ollama" {
        let env_hint = get_provider(provider_name)
            .and_then(|p| p.env_var)
            .unwrap_or("PROVIDER_API_KEY");
        provider_lines.push(format!(
            "# api_key_env = \"{env_hint}\"  # or set that env var / store the key in ~/.config/synto/config.toml"
        ));
    }
    let provider_section = provider_lines.join("\n") + "\n";
    let models_section = format!(
        "[models.fast]\nprovider = \"default\"\nmodel = {}\nctx = {fast_ctx}\n\n[models.heavy]\nprovider = \"default\"\nmodel = {}\nctx = {heavy_ctx}\n",
        toml_quote(fast_model),
        toml_quote(heavy_model)
    );
    format!(
        "{provider_section}\n{models_section}\n{}",
        vault_toml_tail(inline_source_citations)
    )
}
