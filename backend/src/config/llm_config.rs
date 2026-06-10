// serde derives unused here — credentials are runtime-only

/// LLM credentials from env vars or config
#[derive(Debug, Clone)]
pub struct LlmCredentials {
    pub base_url: String,
    pub api_key: String,
}

impl LlmCredentials {
    pub fn from_env_or_config(base_url: Option<&str>, api_key: Option<&str>) -> Self {
        let base_url = std::env::var("LLM_BASE_URL")
            .ok()
            .or_else(|| base_url.map(String::from))
            .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());

        let api_key = std::env::var("LLM_API_KEY")
            .or_else(|_| std::env::var("OPENROUTER_API_KEY"))
            .ok()
            .or_else(|| api_key.map(String::from))
            .unwrap_or_default();

        Self { base_url, api_key }
    }
}
