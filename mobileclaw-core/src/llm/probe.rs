use std::time::Instant;
use crate::{ClawError, ClawResult, llm::{provider::{ProviderConfig, ProviderProtocol, create_llm_client}, types::Message}};
use futures::StreamExt;

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub ok: bool,
    pub latency_ms: u64,
    /// true = completions failed but models endpoint responded
    pub degraded: bool,
    pub error: Option<String>,
}

pub async fn probe_provider(config: &ProviderConfig, api_key: Option<&str>) -> ProbeResult {
    let start = Instant::now();

    match probe_with_request(config, api_key).await {
        Ok(()) => ProbeResult {
            ok: true,
            latency_ms: start.elapsed().as_millis() as u64,
            degraded: false,
            error: None,
        },
        Err(completion_err) => {
            let err_msg = completion_err.to_string();
            match probe_models_endpoint(config, api_key).await {
                Ok(()) => ProbeResult {
                    ok: true,
                    latency_ms: start.elapsed().as_millis() as u64,
                    degraded: true,
                    error: None,
                },
                Err(_) => ProbeResult {
                    ok: false,
                    latency_ms: start.elapsed().as_millis() as u64,
                    degraded: false,
                    error: Some(err_msg),
                },
            }
        }
    }
}

async fn probe_with_request(config: &ProviderConfig, api_key: Option<&str>) -> ClawResult<()> {
    // Wrap in timeout so probe never blocks for OS TCP timeout (~30s on refused connections)
    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        probe_with_request_inner(config, api_key),
    )
    .await
    .unwrap_or_else(|_| Err(ClawError::Llm("probe timed out after 15s".into())))
}

async fn probe_with_request_inner(config: &ProviderConfig, api_key: Option<&str>) -> ClawResult<()> {
    let client = create_llm_client(config, api_key)?;
    let mut stream = client
        .stream_messages(".", &[Message::user("Hi")], 16, &[])
        .await?;
    // Consume first event to verify the stream is working
    match stream.next().await {
        Some(Ok(_)) => Ok(()),
        Some(Err(e)) => Err(e),
        None => Err(ClawError::Llm("empty response stream".into())),
    }
}

async fn probe_models_endpoint(config: &ProviderConfig, api_key: Option<&str>) -> ClawResult<()> {
    let http = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ClawError::Llm(e.to_string()))?;

    match config.protocol {
        ProviderProtocol::OpenAiCompat => {
            let base = config.base_url.trim_end_matches('/');
            let url = if base.ends_with("/v1") {
                format!("{base}/models")
            } else {
                format!("{base}/v1/models")
            };
            let resp = http
                .get(&url)
                .header("Authorization", format!("Bearer {}", api_key.unwrap_or("")))
                .send()
                .await
                .map_err(|e| ClawError::Llm(e.to_string()))?;
            if resp.status().is_success() { Ok(()) }
            else { Err(ClawError::Llm(format!("models endpoint: {}", resp.status()))) }
        }
        ProviderProtocol::Ollama => {
            let base = config.base_url.trim_end_matches('/');
            let url = format!("{base}/api/tags");
            let resp = http.get(&url).send().await
                .map_err(|e| ClawError::Llm(e.to_string()))?;
            if resp.status().is_success() { Ok(()) }
            else { Err(ClawError::Llm(format!("tags endpoint: {}", resp.status()))) }
        }
        ProviderProtocol::Anthropic => {
            // Anthropic has no public models endpoint — skip fallback
            Err(ClawError::Llm("no models endpoint for Anthropic".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_result_fields() {
        let r = ProbeResult { ok: true, latency_ms: 150, degraded: false, error: None };
        assert!(r.ok);
        assert_eq!(r.latency_ms, 150);
        assert!(!r.degraded);
    }

    #[tokio::test]
    async fn test_probe_unreachable_host_returns_fail() {
        // Port 1 is reserved and should refuse connections quickly
        use crate::llm::provider::{ProviderConfig, ProviderProtocol};
        let cfg = ProviderConfig::new(
            "test".into(),
            ProviderProtocol::OpenAiCompat,
            "http://localhost:1".into(),
            "unused".into(),
        );
        let result = probe_provider(&cfg, Some("key")).await;
        assert!(!result.ok);
        assert!(result.error.is_some());
    }
}
