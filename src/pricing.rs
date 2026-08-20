#[derive(Debug, Clone, Copy)]
pub struct Price {
    pub input_per_mtok: Option<f64>,
    pub output_per_mtok: Option<f64>,
    pub is_cloud: bool,
}

pub fn lookup(model_name: &str) -> Option<Price> {
    match model_name {
        "gemma4:e4b" | "google/gemma-4-e4b" | "qwen2.5:14b" => Some(Price {
            input_per_mtok: Some(0.0),
            output_per_mtok: Some(0.0),
            is_cloud: false,
        }),
        "claude-opus-4-7" => Some(Price {
            input_per_mtok: Some(15.0),
            output_per_mtok: Some(75.0),
            is_cloud: true,
        }),
        "claude-sonnet-4-6" => Some(Price {
            input_per_mtok: Some(3.0),
            output_per_mtok: Some(15.0),
            is_cloud: true,
        }),
        "claude-haiku-4-5-20251001" => Some(Price {
            input_per_mtok: Some(0.8),
            output_per_mtok: Some(4.0),
            is_cloud: true,
        }),
        _ => None,
    }
}

pub fn is_cloud_provider(provider_name: &str) -> bool {
    matches!(
        provider_name.to_ascii_lowercase().as_str(),
        "anthropic" | "azure-openai" | "groq" | "openai" | "together"
    )
}

pub fn estimate_cost_usd(model: &str, input_tokens: i64, output_tokens: i64) -> Option<f64> {
    let price = lookup(model)?;
    Some(
        (input_tokens as f64 * price.input_per_mtok? / 1_000_000.0)
            + (output_tokens as f64 * price.output_per_mtok? / 1_000_000.0),
    )
}
