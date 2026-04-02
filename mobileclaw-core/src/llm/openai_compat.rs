use async_trait::async_trait;
use futures::StreamExt;
use eventsource_stream::Eventsource;

use crate::{ClawError, ClawResult, llm::{client::{EventStream, LlmClient}, types::{Message, StreamEvent}}};

pub struct OpenAiCompatClient {
    /// Always ends with "/v1" (normalised in constructor)
    base_url: String,
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl OpenAiCompatClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> ClawResult<Self> {
        let base_url = normalise_base_url(base_url);
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .build()
            .map_err(|e| ClawError::Llm(format!("HTTP client build failed: {e}")))?;
        Ok(Self { base_url, api_key: api_key.into(), model: model.into(), http })
    }
}

fn normalise_base_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

/// Parse one OpenAI SSE data string into a StreamEvent.
/// Returns None for empty/role-only deltas (should be skipped by caller).
pub(crate) fn parse_openai_event(data: &str) -> ClawResult<Option<StreamEvent>> {
    if data == "[DONE]" {
        return Ok(Some(StreamEvent::MessageStop));
    }
    let v: serde_json::Value =
        serde_json::from_str(data).map_err(|e| ClawError::Parse(e.to_string()))?;
    let text = v["choices"][0]["delta"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(StreamEvent::TextDelta { text }))
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatClient {
    async fn stream_messages(
        &self,
        system: &str,
        messages: &[Message],
        max_tokens: u32,
    ) -> ClawResult<EventStream> {
        let mut msg_array = vec![serde_json::json!({"role":"system","content":system})];
        for m in messages {
            use crate::llm::types::Role;
            let role_str = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => "system",
            };
            msg_array.push(serde_json::json!({
                "role": role_str,
                "content": m.text_content()
            }));
        }
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": msg_array,
            "stream": true,
        });

        let url = format!("{}/chat/completions", self.base_url);
        tracing::debug!(
            url = %url,
            model = %self.model,
            messages = msg_array.len(),
            max_tokens,
            "OpenAiCompatClient: sending request"
        );

        let resp = self.http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(url = %url, error = %e, "OpenAiCompatClient: HTTP send failed");
                ClawError::Llm(format!("OpenAI-compat request: {e}"))
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::error!(url = %url, status = %status, body = %body, "OpenAiCompatClient: API error response");
            return Err(ClawError::Llm(format!("OpenAI-compat {status}: {body}")));
        }
        tracing::debug!(url = %url, status = %status, "OpenAiCompatClient: streaming response started");

        // Synthetic MessageStart, then map SSE chunks
        let initial = futures::stream::once(async { Ok(StreamEvent::MessageStart) });
        let data_stream = resp.bytes_stream().eventsource().filter_map(|ev| async {
            match ev {
                Ok(e) => match parse_openai_event(&e.data) {
                    Ok(Some(event)) => Some(Ok(event)),
                    Ok(None) => None,
                    Err(e) => Some(Err(e)),
                },
                Err(e) => Some(Err(ClawError::Llm(e.to_string()))),
            }
        });

        Ok(Box::pin(initial.chain(data_stream)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::StreamEvent;
    use proptest::prelude::*;

    #[test]
    fn test_parse_openai_event_content() {
        let data = r#"{"id":"1","choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
        assert_eq!(parse_openai_event(data).unwrap(), Some(StreamEvent::TextDelta { text: "Hello".into() }));
    }

    #[test]
    fn test_parse_openai_event_role_only_skipped() {
        let data = r#"{"id":"1","choices":[{"delta":{"role":"assistant"},"index":0}]}"#;
        assert_eq!(parse_openai_event(data).unwrap(), None);
    }

    #[test]
    fn test_parse_openai_event_done() {
        assert_eq!(parse_openai_event("[DONE]").unwrap(), Some(StreamEvent::MessageStop));
    }

    #[test]
    fn test_parse_openai_event_null_content_skipped() {
        let data = r#"{"id":"1","choices":[{"delta":{"content":null},"index":0}]}"#;
        assert_eq!(parse_openai_event(data).unwrap(), None);
    }

    #[test]
    fn test_normalise_base_url_appends_v1() {
        assert_eq!(normalise_base_url("https://api.groq.com/openai"), "https://api.groq.com/openai/v1");
        assert_eq!(normalise_base_url("https://api.groq.com/openai/v1"), "https://api.groq.com/openai/v1");
        assert_eq!(normalise_base_url("https://api.groq.com/openai/"), "https://api.groq.com/openai/v1");
    }

    proptest! {
        #[test]
        fn test_parse_openai_event_never_panics(s in ".*") {
            // Function must not panic on any input — it may return Ok or Err
            let _ = parse_openai_event(&s);
        }
    }
}
