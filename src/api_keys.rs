use crate::paths::API_KEY_ENV_VAR;
use crate::providers::get_provider;

const LOCAL_URL_PREFIXES: &[&str] = &["http://localhost", "http://127.0.0.1"];

pub fn resolve_api_key(
    provider_kind: &str,
    alias: Option<&str>,
    block_api_key_env: Option<&str>,
    api_key_env_override: Option<&str>,
    provider_keys: Option<&std::collections::HashMap<String, String>>,
    legacy_api_key: Option<&str>,
) -> Option<String> {
    for env_name in [api_key_env_override, block_api_key_env]
        .into_iter()
        .flatten()
    {
        if let Ok(val) = std::env::var(env_name) {
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    if let Some(prov) = get_provider(provider_kind) {
        if let Some(env_var) = prov.env_var {
            if let Ok(val) = std::env::var(env_var) {
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
    }
    if let (Some(alias), Some(keys)) = (alias, provider_keys) {
        if let Some(val) = keys.get(alias) {
            if !val.is_empty() {
                return Some(val.clone());
            }
        }
    }
    if let Ok(val) = std::env::var(API_KEY_ENV_VAR) {
        if !val.is_empty() {
            return Some(val);
        }
    }
    legacy_api_key
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

pub fn credential_gap(
    provider_kind: &str,
    api_key: Option<&str>,
    api_key_env: Option<&str>,
    url: &str,
    has_custom_headers: bool,
) -> Option<(String, Option<String>)> {
    if api_key.map(|s| !s.is_empty()).unwrap_or(false) {
        return None;
    }
    if has_custom_headers {
        return None;
    }
    let prov = get_provider(provider_kind)?;
    if !prov.requires_auth {
        return None;
    }
    if LOCAL_URL_PREFIXES.iter().any(|p| url.starts_with(p)) {
        return None;
    }
    if let Some(env) = api_key_env {
        return Some(("declared".into(), Some(env.to_string())));
    }
    Some(("missing".into(), prov.env_var.map(|s| s.to_string())))
}
