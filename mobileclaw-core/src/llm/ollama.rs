// TODO: implement Ollama streaming client
use async_trait::async_trait;
use crate::{ClawResult, ClawError};
use crate::llm::client::EventStream;
use crate::llm::types::Message;

pub struct OllamaClient;

impl OllamaClient {
    pub fn new(_base_url: &str, _model: &str) -> ClawResult<Self> {
        Err(ClawError::Llm("OllamaClient not yet implemented".into()))
    }
}

#[async_trait]
impl crate::llm::client::LlmClient for OllamaClient {
    async fn stream_messages(
        &self,
        _system: &str,
        _messages: &[Message],
        _max_tokens: u32,
    ) -> ClawResult<EventStream> {
        Err(ClawError::Llm("OllamaClient not yet implemented".into()))
    }
}
