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
#[allow(dead_code)]
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

#[async_trait]
impl LlmClient for ClaudeClient {
    async fn stream_messages(
        &self,
        system: &str,
        messages: &[Message],
        max_tokens: u32,
    ) -> ClawResult<EventStream> {
        use futures::StreamExt;
        use eventsource_stream::Eventsource;

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "system": system,
            "messages": messages,
            "stream": true,
        });

        tracing::debug!(
            model = %self.model,
            messages = messages.len(),
            max_tokens,
            "ClaudeClient: sending request"
        );

        let resp = self.http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "ClaudeClient: HTTP send failed");
                ClawError::Llm(e.to_string())
            })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            tracing::error!(status = %status, body = %text, "ClaudeClient: API error response");
            return Err(ClawError::Llm(format!("Claude API error {}: {}", status, text)));
        }
        tracing::debug!(status = %status, "ClaudeClient: streaming response started");

        let stream = resp.bytes_stream().eventsource().map(|event| {
            match event {
                Ok(ev) if ev.event == "message_start" => Ok(StreamEvent::MessageStart),
                Ok(ev) if ev.event == "message_stop" => Ok(StreamEvent::MessageStop),
                Ok(ev) if ev.event == "content_block_delta" => {
                    let v: serde_json::Value = serde_json::from_str(&ev.data)
                        .map_err(|e| ClawError::Parse(e.to_string()))?;
                    let text = v["delta"]["text"].as_str().unwrap_or("").to_string();
                    Ok(StreamEvent::TextDelta { text })
                }
                Ok(_) => Ok(StreamEvent::TextDelta { text: String::new() }),
                Err(e) => Err(ClawError::Llm(e.to_string())),
            }
        });
        Ok(Box::pin(stream))
    }
}

use crate::ClawError;

#[async_trait]
impl LlmClient for std::sync::Arc<dyn LlmClient> {
    async fn stream_messages(
        &self,
        system: &str,
        messages: &[crate::llm::types::Message],
        max_tokens: u32,
    ) -> crate::ClawResult<EventStream> {
        self.as_ref().stream_messages(system, messages, max_tokens).await
    }
}

#[cfg(feature = "test-utils")]
pub mod test_helpers {
    use super::*;
    use crate::llm::types::StreamEvent;
    use futures::stream;

    /// Fixed-response mock LLM client for integration tests
    pub struct MockLlmClient {
        pub response: String,
    }

    #[async_trait::async_trait]
    impl LlmClient for MockLlmClient {
        async fn stream_messages(
            &self,
            _system: &str,
            _messages: &[crate::llm::types::Message],
            _max_tokens: u32,
        ) -> crate::ClawResult<EventStream> {
            let text = self.response.clone();
            let events: Vec<crate::ClawResult<StreamEvent>> = vec![
                Ok(StreamEvent::MessageStart),
                Ok(StreamEvent::TextDelta { text }),
                Ok(StreamEvent::MessageStop),
            ];
            Ok(Box::pin(stream::iter(events)))
        }
    }
}
