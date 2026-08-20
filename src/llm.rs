use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::cache::LlmCache;
use crate::config::{Config, ResolvedModel};
use crate::hashing::sha256_hex;
use crate::models::{json_schema_template, AnalysisResult};
use crate::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct CallStats {
    pub latency_ms: u64,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GenerateRequest {
    pub prompt: String,
    pub model: String,
    pub system: String,
    pub format: Option<String>,
    pub num_ctx: u32,
    pub num_predict: i64,
    pub temperature: Option<f32>,
    pub think: Option<bool>,
    pub options: serde_json::Map<String, Value>,
}

pub trait LlmClient: Send + Sync {
    fn generate(&self, req: &GenerateRequest) -> Result<String>;
    fn embed_batch(&self, texts: &[String], model: &str) -> Result<Vec<Vec<f32>>>;
    fn healthcheck(&self) -> bool;
    fn require_healthy(&self) -> Result<()> {
        if self.healthcheck() {
            Ok(())
        } else {
            Err(Error::llm("provider is not reachable"))
        }
    }
    fn list_models(&self) -> Result<Vec<String>>;
    fn last_stats(&self) -> CallStats;
}

pub struct HttpLlmClient {
    kind: String,
    base_url: String,
    api_key: Option<String>,
    supports_json_mode: bool,
    azure: bool,
    azure_api_version: String,
    anthropic: bool,
    cache: Option<LlmCache>,
    cache_namespace: String,
    stats: Mutex<CallStats>,
    http: reqwest::blocking::Client,
}

impl HttpLlmClient {
    pub fn from_resolved(resolved: &ResolvedModel, cache: Option<LlmCache>) -> Result<Self> {
        let timeout = Duration::from_secs_f64(resolved.timeout.max(1.0));
        let mut headers = reqwest::header::HeaderMap::new();
        for (k, v) in &resolved.headers {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                headers.insert(name, val);
            }
        }
        let http = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .default_headers(headers.clone())
            .build()?;
        Ok(Self {
            kind: resolved.provider_kind.clone(),
            base_url: resolved.url.trim_end_matches('/').to_string(),
            api_key: resolved.api_key.clone(),
            supports_json_mode: resolved.supports_json_mode,
            azure: resolved.azure,
            azure_api_version: resolved.azure_api_version.clone(),
            anthropic: resolved.anthropic_compat,
            cache,
            cache_namespace: resolved.cache_namespace(),
            stats: Mutex::new(CallStats::default()),
            http,
        })
    }

    fn cache_key(&self, req: &GenerateRequest) -> String {
        let mut messages = Vec::new();
        if !req.system.is_empty() {
            messages.push(json!({"role":"system","content": req.system}));
        }
        messages.push(json!({"role":"user","content": req.prompt}));
        let data = format!(
            "{}\0{}{}",
            self.cache_namespace,
            req.model,
            serde_json::to_string(&messages).unwrap_or_default()
        );
        sha256_hex(data.as_bytes())
    }

    fn post_json(&self, url: &str, payload: &Value) -> Result<reqwest::blocking::Response> {
        let delays = [1u64, 2, 4, 8, 16];
        let mut last_err = None;
        for (i, delay) in delays.iter().enumerate() {
            let mut b = self.http.post(url).json(payload);
            if let Some(key) = &self.api_key {
                if self.azure {
                    b = b.header("api-key", key);
                } else if self.anthropic {
                    b = b
                        .header("x-api-key", key)
                        .header("anthropic-version", "2023-06-01");
                } else {
                    b = b.bearer_auth(key);
                }
            }
            match b.send() {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    last_err = Some(e);
                    if i + 1 < delays.len() {
                        std::thread::sleep(Duration::from_secs(*delay));
                    }
                }
            }
        }
        Err(Error::llm(format!(
            "connection error: {}",
            last_err.unwrap()
        )))
    }
}

impl LlmClient for HttpLlmClient {
    fn generate(&self, req: &GenerateRequest) -> Result<String> {
        if let Some(cache) = &self.cache {
            let key = self.cache_key(req);
            if let Ok(Some(hit)) = cache.get(&key) {
                *self.stats.lock().unwrap() = CallStats {
                    latency_ms: 0,
                    cache_hit: true,
                    ..Default::default()
                };
                return Ok(hit);
            }
        }
        let t0 = Instant::now();
        let text = if self.kind == "ollama" && !self.base_url.ends_with("/v1") {
            self.generate_ollama(req)?
        } else if self.anthropic {
            self.generate_anthropic(req)?
        } else {
            self.generate_openai(req)?
        };
        if let Some(cache) = &self.cache {
            let _ = cache.put(&self.cache_key(req), &req.model, &text);
        }
        self.stats.lock().unwrap().latency_ms = t0.elapsed().as_millis() as u64;
        Ok(text)
    }

    fn embed_batch(&self, texts: &[String], model: &str) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        if self.kind == "ollama" && !self.base_url.ends_with("/v1") {
            let resp = self.post_json(
                &format!("{}/api/embed", self.base_url),
                &json!({"model": model, "input": texts}),
            )?;
            let body: Value = resp.json()?;
            let embeddings = body["embeddings"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|v| {
                    v.as_array().map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_f64().map(|f| f as f32))
                            .collect()
                    })
                })
                .collect();
            return Ok(embeddings);
        }
        if self.anthropic {
            return Err(Error::llm("embeddings are not supported for this provider"));
        }
        let url = format!("{}/embeddings", self.base_url);
        let resp = self.post_json(&url, &json!({"model": model, "input": texts}))?;
        let body: Value = resp.json()?;
        let mut items: Vec<(i64, Vec<f32>)> = body["data"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|d| {
                let idx = d.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
                let emb = d
                    .get("embedding")?
                    .as_array()?
                    .iter()
                    .filter_map(|x| x.as_f64().map(|f| f as f32))
                    .collect();
                Some((idx, emb))
            })
            .collect();
        items.sort_by_key(|(i, _)| *i);
        Ok(items.into_iter().map(|(_, e)| e).collect())
    }

    fn healthcheck(&self) -> bool {
        let url = if self.kind == "ollama" && !self.base_url.ends_with("/v1") {
            format!("{}/api/tags", self.base_url)
        } else if self.anthropic {
            self.base_url.clone()
        } else if self.azure {
            format!(
                "{}/openai/models?api-version={}",
                self.base_url, self.azure_api_version
            )
        } else {
            format!("{}/models", self.base_url)
        };
        self.http
            .get(&url)
            .send()
            .map(|r| r.status().as_u16() < 500)
            .unwrap_or(false)
    }

    fn require_healthy(&self) -> Result<()> {
        if self.healthcheck() {
            Ok(())
        } else if self.kind == "ollama" {
            Err(Error::llm(
                "Ollama not running. Start it with:\n  ollama serve\nThen pull required models:\n  ollama pull gemma4:e4b\n  ollama pull qwen2.5:14b",
            ))
        } else {
            Err(Error::llm(format!(
                "{} is not reachable at {}",
                self.kind, self.base_url
            )))
        }
    }

    fn list_models(&self) -> Result<Vec<String>> {
        if self.kind == "ollama" && !self.base_url.ends_with("/v1") {
            let resp = self
                .http
                .get(format!("{}/api/tags", self.base_url))
                .send()?;
            let body: Value = resp.json()?;
            return Ok(body["models"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|m| {
                    m.get("name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect());
        }
        if self.anthropic {
            return Ok(Vec::new());
        }
        let resp = self.http.get(format!("{}/models", self.base_url)).send()?;
        let body: Value = resp.json().unwrap_or(json!({}));
        Ok(body["data"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect())
    }

    fn last_stats(&self) -> CallStats {
        self.stats.lock().unwrap().clone()
    }
}

impl HttpLlmClient {
    fn generate_ollama(&self, req: &GenerateRequest) -> Result<String> {
        let mut options = json!({"num_ctx": req.num_ctx, "num_predict": req.num_predict});
        if let Some(t) = req.temperature {
            options["temperature"] = json!(t);
        }
        for (k, v) in &req.options {
            options[k] = v.clone();
        }
        let mut payload = json!({
            "model": req.model,
            "prompt": req.prompt,
            "system": req.system,
            "stream": false,
            "options": options,
        });
        if let Some(t) = req.think {
            payload["think"] = json!(t);
        }
        if let Some(fmt) = &req.format {
            payload["format"] = json!(fmt);
        }
        let resp = self.post_json(&format!("{}/api/generate", self.base_url), &payload)?;
        if !resp.status().is_success() {
            return Err(Error::llm(format!(
                "Ollama HTTP error: {} {}",
                resp.status(),
                resp.text().unwrap_or_default()
            )));
        }
        let body: Value = resp.json()?;
        let text = body
            .get("response")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let done_reason = body
            .get("done_reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        self.stats.lock().unwrap().prompt_tokens =
            body.get("prompt_eval_count").and_then(|v| v.as_i64());
        self.stats.lock().unwrap().completion_tokens =
            body.get("eval_count").and_then(|v| v.as_i64());
        if done_reason.as_deref() == Some("length") || text.trim().is_empty() {
            return Err(Error::LlmTruncated {
                provider: "ollama".into(),
                max_tokens: if req.num_predict > 0 {
                    req.num_predict
                } else {
                    0
                },
                completion_tokens: body.get("eval_count").and_then(|v| v.as_i64()),
                finish_reason: done_reason.or_else(|| Some("empty_content".into())),
            });
        }
        Ok(text)
    }

    fn generate_openai(&self, req: &GenerateRequest) -> Result<String> {
        let mut messages = Vec::new();
        if !req.system.is_empty() {
            messages.push(json!({"role":"system","content": req.system}));
        }
        messages.push(json!({"role":"user","content": req.prompt}));
        let mut payload = json!({"model": req.model, "messages": messages});
        if let Some(t) = req.temperature {
            payload["temperature"] = json!(t);
        }
        if req.format.as_deref() == Some("json") && self.supports_json_mode {
            payload["response_format"] = json!({"type":"json_object"});
        }
        if req.num_predict > 0 {
            payload["max_tokens"] = json!(req.num_predict);
        }
        for (k, v) in &req.options {
            payload[k] = v.clone();
        }
        let url = if self.azure {
            format!(
                "{}/openai/deployments/{}/chat/completions?api-version={}",
                self.base_url, req.model, self.azure_api_version
            )
        } else {
            format!("{}/chat/completions", self.base_url)
        };
        let resp = self.post_json(&url, &payload)?;
        let status = resp.status();
        let body: Value = resp.json().unwrap_or(json!({}));
        if !status.is_success() {
            let msg = body["error"]["message"]
                .as_str()
                .unwrap_or("request failed");
            if status.as_u16() == 400 {
                return Err(Error::LlmBadRequest(msg.into()));
            }
            return Err(Error::llm(format!("{status} {msg}")));
        }
        let text = body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let finish = body["choices"][0]["finish_reason"]
            .as_str()
            .map(|s| s.to_string());
        self.stats.lock().unwrap().prompt_tokens = body["usage"]["prompt_tokens"].as_i64();
        self.stats.lock().unwrap().completion_tokens = body["usage"]["completion_tokens"].as_i64();
        if matches!(finish.as_deref(), Some("length") | Some("max_tokens"))
            || text.trim().is_empty()
        {
            return Err(Error::LlmTruncated {
                provider: self.kind.clone(),
                max_tokens: if req.num_predict > 0 {
                    req.num_predict
                } else {
                    0
                },
                completion_tokens: body["usage"]["completion_tokens"].as_i64(),
                finish_reason: finish.or_else(|| Some("empty_content".into())),
            });
        }
        Ok(text)
    }

    fn generate_anthropic(&self, req: &GenerateRequest) -> Result<String> {
        let max_tokens = if req.num_predict > 0 {
            req.num_predict
        } else {
            4096
        };
        let payload = json!({
            "model": req.model,
            "max_tokens": max_tokens,
            "system": req.system,
            "messages": [{"role":"user","content": req.prompt}],
        });
        let resp = self.post_json(&format!("{}/v1/messages", self.base_url), &payload)?;
        let body: Value = resp.json()?;
        let text = body["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let stop = body["stop_reason"].as_str().map(|s| s.to_string());
        self.stats.lock().unwrap().prompt_tokens = body["usage"]["input_tokens"].as_i64();
        self.stats.lock().unwrap().completion_tokens = body["usage"]["output_tokens"].as_i64();
        if stop.as_deref() == Some("max_tokens") || text.trim().is_empty() {
            return Err(Error::LlmTruncated {
                provider: self.kind.clone(),
                max_tokens,
                completion_tokens: body["usage"]["output_tokens"].as_i64(),
                finish_reason: stop.or_else(|| Some("empty_content".into())),
            });
        }
        Ok(text)
    }
}

pub struct RoleEndpoint {
    pub client: Box<dyn LlmClient>,
    pub model: String,
    pub ctx: u32,
    pub think: Option<bool>,
    pub temperature: Option<f32>,
}

pub struct ModelRouter {
    fast: RoleEndpoint,
    heavy: RoleEndpoint,
    embed: RoleEndpoint,
}

impl ModelRouter {
    pub fn build(config: &Config, cache: Option<LlmCache>) -> Result<Self> {
        let mk = |role: &str| -> Result<RoleEndpoint> {
            let resolved = config.resolve_role(role)?;
            let client = HttpLlmClient::from_resolved(&resolved, cache.clone())?;
            Ok(RoleEndpoint {
                client: Box::new(client),
                model: resolved.model,
                ctx: resolved.ctx,
                think: resolved.think,
                temperature: resolved.temperature,
            })
        };
        Ok(Self {
            fast: mk("fast")?,
            heavy: mk("heavy")?,
            embed: mk("embed")?,
        })
    }

    pub fn fast(&self) -> &RoleEndpoint {
        &self.fast
    }
    pub fn heavy(&self) -> &RoleEndpoint {
        &self.heavy
    }
    pub fn embed(&self) -> &RoleEndpoint {
        &self.embed
    }

    pub fn require_healthy(&self) -> Result<()> {
        self.fast.client.require_healthy()?;
        self.heavy.client.require_healthy()?;
        Ok(())
    }

    pub fn from_parts(fast: RoleEndpoint, heavy: RoleEndpoint, embed: RoleEndpoint) -> Self {
        Self { fast, heavy, embed }
    }
}

pub struct MockClient {
    pub responses: Mutex<Vec<String>>,
    pub stats: Mutex<CallStats>,
}

impl MockClient {
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(responses),
            stats: Mutex::new(CallStats::default()),
        }
    }
}

impl LlmClient for MockClient {
    fn generate(&self, _req: &GenerateRequest) -> Result<String> {
        let mut q = self.responses.lock().unwrap();
        if q.is_empty() {
            Err(Error::llm("mock client has no responses left"))
        } else {
            Ok(q.remove(0))
        }
    }
    fn embed_batch(&self, texts: &[String], _model: &str) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![0.0; 8]).collect())
    }
    fn healthcheck(&self) -> bool {
        true
    }
    fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["mock".into()])
    }
    fn last_stats(&self) -> CallStats {
        self.stats.lock().unwrap().clone()
    }
}

const SCHEMA_INSTRUCTION: &str =
    "You MUST respond with ONLY valid JSON. No prose before or after.\n\
Return the JSON object directly. Do NOT wrap it or add extra keys.\n\n\
Fill in this exact JSON structure with real content:\n\n\
{template}\n\n\
Replace each placeholder string with actual content. Keep the same keys and types.\n\
Respond with nothing but the completed JSON object.";

fn extract_json(text: &str) -> Option<String> {
    if let Some(m) = regex::Regex::new(r"```json\s*\n(.*?)\n\s*```")
        .unwrap()
        .captures(text)
    {
        return Some(m.get(1).unwrap().as_str().trim().to_string());
    }
    if let Some(m) = regex::Regex::new(r"```\s*\n(\{.*?\})\s*\n```")
        .unwrap()
        .captures(text)
    {
        return Some(m.get(1).unwrap().as_str().trim().to_string());
    }
    if let Some(m) = regex::Regex::new(r"\{[\s\S]*\}").unwrap().find(text) {
        return Some(m.as_str().trim().to_string());
    }
    None
}

fn unwrap_obj(mut data: Value) -> Value {
    if let Value::Object(map) = &data {
        if map.len() == 1 {
            let (_k, v) = map.iter().next().unwrap();
            if v.is_object() {
                return v.clone();
            }
            if let Some(s) = v.as_str() {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    if parsed.is_object() {
                        return parsed;
                    }
                }
            }
        }
        if map.contains_key("properties") {
            if let Some(props) = map.get("properties").and_then(|v| v.as_object()) {
                let mut out = serde_json::Map::new();
                for (k, v) in props {
                    if let Some(leaf) = v.get("default").or_else(|| v.get("example")).cloned() {
                        out.insert(k.clone(), leaf);
                    }
                }
                if !out.is_empty() {
                    return Value::Object(out);
                }
            }
        }
    }
    if let Value::Object(ref mut map) = data {
        if let Some(Value::Array(arr)) = map.get_mut("concepts") {
            for item in arr.iter_mut() {
                if let Value::String(s) = item {
                    *item = json!({"name": s, "aliases": []});
                }
            }
        }
    }
    data
}

pub fn request_structured<T: serde::de::DeserializeOwned>(
    client: &dyn LlmClient,
    prompt: &str,
    schema_kind: &str,
    model: &str,
    system: &str,
    num_ctx: u32,
    num_predict: i64,
    temperature: Option<f32>,
    think: Option<bool>,
    max_retries: usize,
) -> Result<T> {
    let template = json_schema_template(schema_kind);
    let schema_sys = SCHEMA_INSTRUCTION.replace("{template}", &template);
    let sys = if system.is_empty() {
        schema_sys
    } else {
        format!("{system}\n\n{schema_sys}")
    };
    let mut last_err = String::new();
    for attempt in 0..=max_retries {
        let extra = if attempt == 0 {
            String::new()
        } else {
            format!("\n\nYour previous response was invalid JSON: {last_err}\nReturn ONLY the corrected JSON object.")
        };
        let req = GenerateRequest {
            prompt: format!("{prompt}{extra}"),
            model: model.to_string(),
            system: sys.clone(),
            format: Some("json".into()),
            num_ctx,
            num_predict,
            temperature,
            think,
            options: serde_json::Map::new(),
        };
        let raw = client.generate(&req)?;
        let candidate = extract_json(&raw).unwrap_or(raw);
        match serde_json::from_str::<Value>(&candidate) {
            Ok(v) => {
                let mut v = unwrap_obj(v);
                if schema_kind == "AnalysisResult" {
                    v = AnalysisResult::coerce(v);
                }
                match serde_json::from_value::<T>(v) {
                    Ok(parsed) => return Ok(parsed),
                    Err(e) => last_err = e.to_string(),
                }
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(Error::StructuredOutput(last_err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AnalysisResult;

    #[test]
    fn extract_json_from_fence() {
        let raw = "here you go\n```json\n{\"ok\": true}\n```\n";
        assert_eq!(extract_json(raw).as_deref(), Some("{\"ok\": true}"));
    }

    #[test]
    fn request_structured_parses_mock_analysis() {
        let payload = r#"{
            "summary": "A note about qubits.",
            "concepts": [{"name": "Qubit", "aliases": ["quantum bit"]}],
            "suggested_topics": ["Quantum computing"],
            "named_references": [],
            "quality": "high",
            "language": "en"
        }"#;
        let client = MockClient::new(vec![payload.into()]);
        let parsed: AnalysisResult = request_structured(
            &client,
            "note",
            "AnalysisResult",
            "mock",
            "",
            2048,
            256,
            None,
            None,
            0,
        )
        .unwrap();
        assert_eq!(parsed.summary, "A note about qubits.");
        assert_eq!(parsed.concepts[0].name, "Qubit");
        assert_eq!(parsed.quality, "high");
    }
}
