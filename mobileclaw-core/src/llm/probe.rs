use crate::llm::provider::ProviderConfig;

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub degraded: bool,
    pub error: Option<String>,
}

pub async fn probe_provider(_config: &ProviderConfig, _api_key: Option<&str>) -> ProbeResult {
    unimplemented!("probe_provider not yet implemented")
}
