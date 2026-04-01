use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::{ClawError, ClawResult, llm::client::LlmClient};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    Anthropic,
    OpenAiCompat,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub protocol: ProviderProtocol,
    pub base_url: String,
    pub model: String,
    pub created_at: i64,
}

impl ProviderConfig {
    pub fn new(
        name: String,
        protocol: ProviderProtocol,
        base_url: String,
        model: String,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            protocol,
            base_url,
            model,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time before epoch")
                .as_secs() as i64,
        }
    }
}

/// Create an LlmClient from a ProviderConfig. Called once per session.
/// api_key is None only for Ollama.
pub fn create_llm_client(
    config: &ProviderConfig,
    api_key: Option<&str>,
) -> ClawResult<Arc<dyn LlmClient>> {
    match config.protocol {
        ProviderProtocol::Anthropic => {
            let key = api_key
                .ok_or_else(|| ClawError::Llm("Anthropic requires api_key".into()))?;
            use crate::llm::client::ClaudeClient;
            Ok(Arc::new(ClaudeClient::new(key, &config.model)))
        }
        ProviderProtocol::OpenAiCompat => {
            let key = api_key
                .ok_or_else(|| ClawError::Llm("OpenAI-compat requires api_key".into()))?;
            use crate::llm::openai_compat::OpenAiCompatClient;
            Ok(Arc::new(OpenAiCompatClient::new(&config.base_url, key, &config.model)?))
        }
        ProviderProtocol::Ollama => {
            use crate::llm::ollama::OllamaClient;
            Ok(Arc::new(OllamaClient::new(&config.base_url, &config.model)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_config_roundtrip() {
        let cfg = ProviderConfig::new(
            "DeepSeek".into(),
            ProviderProtocol::OpenAiCompat,
            "https://api.deepseek.com".into(),
            "deepseek-chat".into(),
        );
        assert_eq!(cfg.protocol, ProviderProtocol::OpenAiCompat);
        assert_eq!(cfg.model, "deepseek-chat");
        assert!(!cfg.id.is_empty());

        let json = serde_json::to_string(&cfg).unwrap();
        let restored: ProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, cfg.id);
        assert_eq!(restored.protocol, ProviderProtocol::OpenAiCompat);
        assert_eq!(restored.base_url, "https://api.deepseek.com");
    }
}
