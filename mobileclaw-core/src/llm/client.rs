use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;
use crate::{ClawResult, llm::types::{Message, StreamEvent}};

pub type EventStream = Pin<Box<dyn Stream<Item = ClawResult<StreamEvent>> + Send>>;

#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 发送消息，返回流式事件
    async fn stream_messages(
        &self,
        system: &str,
        messages: &[Message],
        max_tokens: u32,
    ) -> ClawResult<EventStream>;
}

/// Claude API 实现（Messages API + SSE）
pub struct ClaudeClient {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl ClaudeClient {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .build()
            .expect("failed to build reqwest client");
        Self { api_key: api_key.into(), model: model.into(), http }
    }
}

// NOTE: stream_messages full implementation comes in Task 11
#[async_trait]
impl LlmClient for ClaudeClient {
    async fn stream_messages(
        &self,
        _system: &str,
        _messages: &[Message],
        _max_tokens: u32,
    ) -> ClawResult<EventStream> {
        Err(crate::ClawError::Llm("not yet implemented".into()))
    }
}
