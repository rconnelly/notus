#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub name: &'static str,
    pub display_name: &'static str,
    pub default_url: &'static str,
    pub requires_auth: bool,
    pub supports_json_mode: bool,
    pub supports_embeddings: bool,
    pub is_local: bool,
    pub default_timeout: f64,
    pub env_var: Option<&'static str>,
    pub azure: bool,
    pub anthropic_compat: bool,
}

const fn p(
    name: &'static str,
    display_name: &'static str,
    default_url: &'static str,
    requires_auth: bool,
    supports_json_mode: bool,
    supports_embeddings: bool,
    is_local: bool,
    default_timeout: f64,
    env_var: Option<&'static str>,
) -> ProviderInfo {
    ProviderInfo {
        name,
        display_name,
        default_url,
        requires_auth,
        supports_json_mode,
        supports_embeddings,
        is_local,
        default_timeout,
        env_var,
        azure: false,
        anthropic_compat: false,
    }
}

pub fn registry() -> Vec<ProviderInfo> {
    vec![
        p(
            "ollama",
            "Ollama",
            "http://localhost:11434",
            false,
            true,
            true,
            true,
            600.0,
            None,
        ),
        p(
            "lm_studio",
            "LM Studio",
            "http://localhost:1234/v1",
            false,
            true,
            true,
            true,
            600.0,
            None,
        ),
        p(
            "vllm",
            "vLLM",
            "http://localhost:8000/v1",
            false,
            true,
            true,
            true,
            600.0,
            None,
        ),
        p(
            "llama_cpp",
            "llama.cpp",
            "http://localhost:8080/v1",
            false,
            true,
            false,
            true,
            600.0,
            None,
        ),
        p(
            "localai",
            "LocalAI",
            "http://localhost:8080/v1",
            false,
            true,
            true,
            true,
            600.0,
            None,
        ),
        p(
            "tgi",
            "TGI",
            "http://localhost:3000/v1",
            false,
            true,
            false,
            true,
            600.0,
            None,
        ),
        p(
            "sglang",
            "SGLang",
            "http://localhost:30000/v1",
            false,
            true,
            true,
            true,
            600.0,
            None,
        ),
        p(
            "llamafile",
            "Llamafile",
            "http://localhost:8080/v1",
            false,
            true,
            false,
            true,
            600.0,
            None,
        ),
        p(
            "lemonade",
            "Lemonade",
            "http://localhost:8000/v1",
            false,
            true,
            false,
            true,
            600.0,
            None,
        ),
        p(
            "groq",
            "Groq",
            "https://api.groq.com/openai/v1",
            true,
            true,
            false,
            false,
            120.0,
            Some("GROQ_API_KEY"),
        ),
        p(
            "together",
            "Together AI",
            "https://api.together.xyz/v1",
            true,
            true,
            true,
            false,
            120.0,
            Some("TOGETHER_API_KEY"),
        ),
        p(
            "fireworks",
            "Fireworks AI",
            "https://api.fireworks.ai/inference/v1",
            true,
            true,
            true,
            false,
            120.0,
            Some("FIREWORKS_API_KEY"),
        ),
        p(
            "deepinfra",
            "DeepInfra",
            "https://api.deepinfra.com/v1/openai",
            true,
            true,
            true,
            false,
            120.0,
            Some("DEEPINFRA_API_KEY"),
        ),
        p(
            "openrouter",
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            true,
            true,
            false,
            false,
            120.0,
            Some("OPENROUTER_API_KEY"),
        ),
        p(
            "mistral",
            "Mistral AI",
            "https://api.mistral.ai/v1",
            true,
            true,
            true,
            false,
            120.0,
            Some("MISTRAL_API_KEY"),
        ),
        p(
            "deepseek",
            "DeepSeek",
            "https://api.deepseek.com/v1",
            true,
            true,
            false,
            false,
            120.0,
            Some("DEEPSEEK_API_KEY"),
        ),
        p(
            "siliconflow",
            "SiliconFlow",
            "https://api.siliconflow.cn/v1",
            true,
            true,
            true,
            false,
            120.0,
            Some("SILICONFLOW_API_KEY"),
        ),
        p(
            "perplexity",
            "Perplexity",
            "https://api.perplexity.ai",
            true,
            false,
            false,
            false,
            120.0,
            Some("PERPLEXITY_API_KEY"),
        ),
        p(
            "xai",
            "xAI (Grok)",
            "https://api.x.ai/v1",
            true,
            true,
            false,
            false,
            120.0,
            Some("XAI_API_KEY"),
        ),
        p(
            "nvidia",
            "NVIDIA NIM",
            "https://integrate.api.nvidia.com/v1",
            true,
            true,
            false,
            false,
            120.0,
            Some("NVIDIA_API_KEY"),
        ),
        ProviderInfo {
            name: "azure",
            display_name: "Azure OpenAI",
            default_url: "",
            requires_auth: true,
            supports_json_mode: true,
            supports_embeddings: true,
            is_local: false,
            default_timeout: 120.0,
            env_var: Some("AZURE_OPENAI_API_KEY"),
            azure: true,
            anthropic_compat: false,
        },
        ProviderInfo {
            name: "kimi",
            display_name: "Kimi",
            default_url: "https://api.kimi.com/coding",
            requires_auth: true,
            supports_json_mode: false,
            supports_embeddings: false,
            is_local: false,
            default_timeout: 120.0,
            env_var: Some("KIMI_API_KEY"),
            azure: false,
            anthropic_compat: true,
        },
        p(
            "custom", "Custom", "", false, true, false, false, 300.0, None,
        ),
    ]
}

pub fn get_provider(name: &str) -> Option<ProviderInfo> {
    registry().into_iter().find(|p| p.name == name)
}

pub fn list_local_providers() -> Vec<ProviderInfo> {
    registry()
        .into_iter()
        .filter(|p| p.is_local && p.name != "ollama")
        .collect()
}

pub fn list_cloud_providers() -> Vec<ProviderInfo> {
    registry()
        .into_iter()
        .filter(|p| !p.is_local && p.name != "custom")
        .collect()
}

pub fn list_all_providers() -> Vec<ProviderInfo> {
    let mut out = Vec::new();
    if let Some(o) = get_provider("ollama") {
        out.push(o);
    }
    out.extend(list_local_providers());
    out.extend(list_cloud_providers());
    if let Some(c) = get_provider("custom") {
        out.push(c);
    }
    out
}
