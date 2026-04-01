use async_trait::async_trait;
use async_stream::try_stream;
use futures::StreamExt;

use crate::{ClawError, ClawResult, llm::{client::{EventStream, LlmClient}, types::{Message, StreamEvent}}};

pub struct OllamaClient {
    /// Always ends with "/" (normalised in constructor)
    base_url: String,
    model: String,
    http: reqwest::Client,
}

impl OllamaClient {
    pub fn new(base_url: &str, model: &str) -> ClawResult<Self> {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .build()
            .map_err(|e| ClawError::Llm(format!("HTTP client build failed: {e}")))?;
        Ok(Self { base_url: normalise_base_url(base_url), model: model.into(), http })
    }
}

fn normalise_base_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    format!("{trimmed}/")
}

/// Parse one NDJSON line from Ollama's /api/chat response.
/// Returns (Option<StreamEvent>, is_done).
/// is_done=true means the stream is finished.
pub(crate) fn parse_ollama_line(line: &str) -> ClawResult<(Option<StreamEvent>, bool)> {
    let v: serde_json::Value =
        serde_json::from_str(line).map_err(|e| ClawError::Parse(e.to_string()))?;
    let text = v["message"]["content"].as_str().unwrap_or("").to_string();
    let done = v["done"].as_bool().unwrap_or(false);
    let event = if text.is_empty() {
        None
    } else {
        Some(StreamEvent::TextDelta { text })
    };
    Ok((event, done))
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn stream_messages(
        &self,
        system: &str,
        messages: &[Message],
        _max_tokens: u32,  // Ollama /api/chat does not accept max_tokens
    ) -> ClawResult<EventStream> {
        let mut msg_array = vec![serde_json::json!({"role":"system","content":system})];
        for m in messages {
            let role = match m.role {
                crate::llm::types::Role::User => "user",
                crate::llm::types::Role::Assistant => "assistant",
                crate::llm::types::Role::System => "system",
            };
            msg_array.push(serde_json::json!({
                "role": role,
                "content": m.text_content()
            }));
        }
        let body = serde_json::json!({
            "model": self.model,
            "messages": msg_array,
            "stream": true,
        });

        let resp = self.http
            .post(format!("{}api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| ClawError::Llm(format!("Ollama request: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ClawError::Llm(format!("Ollama {status}: {body}")));
        }

        let mut bytes_stream = resp.bytes_stream();
        let stream = try_stream! {
            yield StreamEvent::MessageStart;
            let mut buf: Vec<u8> = Vec::new();
            loop {
                if let Some(nl) = buf.iter().position(|&b| b == b'\n') {
                    // Extract the line up to (not including) the newline
                    let line = String::from_utf8_lossy(&buf[..nl]).into_owned();
                    buf.drain(..=nl);
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }
                    let (event, done) = parse_ollama_line(trimmed)?;
                    if let Some(ev) = event {
                        yield ev;
                    }
                    if done {
                        yield StreamEvent::MessageStop;
                        return;
                    }
                } else {
                    match bytes_stream.next().await {
                        Some(Ok(chunk)) => buf.extend_from_slice(&chunk),
                        Some(Err(e)) => Err(ClawError::Llm(e.to_string()))?,
                        None => {
                            // HTTP body ended without done:true
                            yield StreamEvent::MessageStop;
                            return;
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::StreamEvent;
    use proptest::prelude::*;

    #[test]
    fn test_parse_ollama_line_content() {
        let line = r#"{"model":"llama3","message":{"role":"assistant","content":"Hi"},"done":false}"#;
        let (event, done) = parse_ollama_line(line).unwrap();
        assert_eq!(event, Some(StreamEvent::TextDelta { text: "Hi".into() }));
        assert!(!done);
    }

    #[test]
    fn test_parse_ollama_line_done_empty_content() {
        let line = r#"{"model":"llama3","message":{"role":"assistant","content":""},"done":true}"#;
        let (event, done) = parse_ollama_line(line).unwrap();
        assert_eq!(event, None);
        assert!(done);
    }

    #[test]
    fn test_parse_ollama_line_done_with_content() {
        let line = r#"{"model":"llama3","message":{"role":"assistant","content":"!"},"done":true}"#;
        let (event, done) = parse_ollama_line(line).unwrap();
        assert_eq!(event, Some(StreamEvent::TextDelta { text: "!".into() }));
        assert!(done);
    }

    #[test]
    fn test_normalise_ollama_url_adds_slash() {
        assert_eq!(normalise_base_url("http://localhost:11434"), "http://localhost:11434/");
        assert_eq!(normalise_base_url("http://localhost:11434/"), "http://localhost:11434/");
    }

    proptest! {
        #[test]
        fn test_parse_ollama_line_never_panics(s in ".*") {
            let _ = parse_ollama_line(&s);
        }
    }
}
