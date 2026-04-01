//! Integration: verify create_llm_client factory works with the agent loop.
//! Uses MockLlmClient (requires --features test-utils).

#[cfg(feature = "test-utils")]
mod tests {
    use mobileclaw_core::llm::provider::{ProviderConfig, ProviderProtocol, create_llm_client};

    #[test]
    fn test_factory_returns_arc_for_anthropic() {
        let cfg = ProviderConfig::new(
            "Claude".into(),
            ProviderProtocol::Anthropic,
            "https://api.anthropic.com".into(),
            "claude-opus-4-6".into(),
        );
        // Factory must succeed with a valid key
        let result = create_llm_client(&cfg, Some("sk-ant-test"));
        assert!(result.is_ok(), "factory failed: {:?}", result.err());
    }

    #[test]
    fn test_factory_errors_without_key_for_anthropic() {
        let cfg = ProviderConfig::new(
            "Claude".into(),
            ProviderProtocol::Anthropic,
            "https://api.anthropic.com".into(),
            "claude-opus-4-6".into(),
        );
        let result = create_llm_client(&cfg, None);
        assert!(result.is_err());
        let err = result.err().expect("expected Err");
        assert!(err.to_string().contains("api_key"));
    }

    #[test]
    fn test_factory_returns_arc_for_ollama_without_key() {
        let cfg = ProviderConfig::new(
            "Llama".into(),
            ProviderProtocol::Ollama,
            "http://localhost:11434".into(),
            "llama3".into(),
        );
        let result = create_llm_client(&cfg, None);
        assert!(result.is_ok(), "Ollama factory should not require api_key");
    }

    #[test]
    fn test_factory_returns_arc_for_openai_compat() {
        let cfg = ProviderConfig::new(
            "Groq".into(),
            ProviderProtocol::OpenAiCompat,
            "https://api.groq.com/openai".into(),
            "mixtral-8x7b".into(),
        );
        let result = create_llm_client(&cfg, Some("gsk_test"));
        assert!(result.is_ok(), "OpenAI compat factory failed: {:?}", result.err());
    }
}
