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

// ─── Tool call accumulator ────────────────────────────────────────────────────

/// Accumulates OpenAI streaming tool_call chunks across multiple SSE events.
///
/// The OpenAI streaming format sends tool calls as fragments:
/// - First chunk: `tool_calls[i].id`, `.function.name`, empty `.function.arguments`
/// - Subsequent chunks: `.function.arguments` fragments (partial JSON)
/// - Final: `finish_reason: "tool_calls"` with null delta
///
/// Once `[DONE]` is received, call `to_xml()` to produce the agent-compatible
/// `<tool_call>{"name":"...","args":{...}}</tool_call>` XML string.
#[derive(Default)]
pub(crate) struct ToolCallAcc {
    // BTreeMap preserves insertion order by index for deterministic XML output.
    calls: std::collections::BTreeMap<usize, ToolCallEntry>,
}

#[derive(Default)]
struct ToolCallEntry {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallAcc {
    /// Feed one parsed SSE event JSON value into the accumulator.
    /// Extracts `choices[0].delta.tool_calls` fragments.
    pub(crate) fn feed(&mut self, v: &serde_json::Value) {
        let Some(arr) = v["choices"][0]["delta"]["tool_calls"].as_array() else {
            return;
        };
        for tc in arr {
            let idx = tc["index"].as_u64().unwrap_or(0) as usize;
            let entry = self.calls.entry(idx).or_default();
            if let Some(id) = tc["id"].as_str() {
                if !id.is_empty() {
                    entry.id = id.to_string();
                }
            }
            if let Some(name) = tc["function"]["name"].as_str() {
                if !name.is_empty() {
                    entry.name = name.to_string();
                }
            }
            if let Some(args_chunk) = tc["function"]["arguments"].as_str() {
                entry.arguments.push_str(args_chunk);
            }
        }
    }

    /// Returns true if at least one tool call has been accumulated.
    pub(crate) fn has_calls(&self) -> bool {
        !self.calls.is_empty()
    }

    /// Render accumulated tool calls as agent-compatible XML.
    ///
    /// Each call becomes `<tool_call>{"name":"...","args":{...}}</tool_call>`.
    /// Malformed accumulated JSON (truncated stream) falls back to `{}` for args.
    pub(crate) fn to_xml(&self) -> String {
        let mut out = String::new();
        for entry in self.calls.values() {
            let args: serde_json::Value = serde_json::from_str(&entry.arguments)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            let call_json = serde_json::json!({
                "name": entry.name,
                "args": args,
            });
            // to_string() on json! output is always valid; unwrap is safe here.
            out.push_str(&format!(
                "<tool_call>{}</tool_call>",
                serde_json::to_string(&call_json).unwrap_or_default()
            ));
        }
        out
    }
}

// ─── LlmClient implementation ─────────────────────────────────────────────────

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

        // ── Tool call accumulation ────────────────────────────────────────────
        // Some OpenAI-compat models respond with native function calling
        // (`choices[0].delta.tool_calls`) instead of embedding tool invocations
        // as XML text in `choices[0].delta.content`. Previously these chunks were
        // silently dropped, resulting in empty assistant messages.
        //
        // Fix: accumulate tool_call fragments across streaming chunks; on `[DONE]`
        // emit the accumulated calls as `<tool_call>…</tool_call>` XML text so the
        // agent's existing XML parser handles them transparently.
        let tool_acc = std::sync::Arc::new(std::sync::Mutex::new(ToolCallAcc::default()));

        let initial = futures::stream::once(async { Ok(StreamEvent::MessageStart) });
        let data_stream = resp.bytes_stream().eventsource().filter_map(move |ev| {
            let acc = tool_acc.clone();
            async move {
                match ev {
                    Ok(e) => {
                        if e.data == "[DONE]" {
                            // Emit accumulated tool calls as XML before stopping.
                            let locked = acc.lock().unwrap();
                            if locked.has_calls() {
                                let xml = locked.to_xml();
                                tracing::debug!(
                                    tool_calls = locked.calls.len(),
                                    xml_len = xml.len(),
                                    "OpenAiCompatClient: emitting native tool calls as XML"
                                );
                                return Some(Ok(StreamEvent::TextDelta { text: xml }));
                            }
                            return Some(Ok(StreamEvent::MessageStop));
                        }
                        // Parse JSON, feed tool_calls to accumulator, extract text.
                        let v: serde_json::Value = match serde_json::from_str(&e.data) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(ClawError::Parse(e.to_string()))),
                        };
                        acc.lock().unwrap().feed(&v);
                        let text = v["choices"][0]["delta"]["content"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        if text.is_empty() { None } else { Some(Ok(StreamEvent::TextDelta { text })) }
                    }
                    Err(e) => Some(Err(ClawError::Llm(e.to_string()))),
                }
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

    // ── Test-only SSE text parser (mirrors the inline logic in stream_messages) ──

    /// Parse one OpenAI SSE data string into a StreamEvent (text content only).
    /// Returns Some(TextDelta) for non-empty content, Some(MessageStop) for [DONE],
    /// None for role-only / null-content / tool_call-only chunks.
    fn parse_openai_event(data: &str) -> ClawResult<Option<StreamEvent>> {
        if data == "[DONE]" {
            return Ok(Some(StreamEvent::MessageStop));
        }
        let v: serde_json::Value =
            serde_json::from_str(data).map_err(|e| ClawError::Parse(e.to_string()))?;
        let text = v["choices"][0]["delta"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if text.is_empty() { Ok(None) } else { Ok(Some(StreamEvent::TextDelta { text })) }
    }

    // ── parse_openai_event ────────────────────────────────────────────────────

    #[test]
    fn test_parse_text_content() {
        let data = r#"{"id":"1","choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
        assert_eq!(
            parse_openai_event(data).unwrap(),
            Some(StreamEvent::TextDelta { text: "Hello".into() })
        );
    }

    #[test]
    fn test_parse_role_only_skipped() {
        let data = r#"{"id":"1","choices":[{"delta":{"role":"assistant"},"index":0}]}"#;
        assert_eq!(parse_openai_event(data).unwrap(), None);
    }

    #[test]
    fn test_parse_done_sentinel() {
        assert_eq!(
            parse_openai_event("[DONE]").unwrap(),
            Some(StreamEvent::MessageStop)
        );
    }

    #[test]
    fn test_parse_null_content_skipped() {
        let data = r#"{"id":"1","choices":[{"delta":{"content":null},"index":0}]}"#;
        assert_eq!(parse_openai_event(data).unwrap(), None);
    }

    #[test]
    fn test_parse_tool_calls_only_returns_none() {
        // tool_calls-only chunk: content is absent → should return None (not silently
        // crash). The accumulator (not parse_openai_event) handles the tool_calls field.
        let data = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"memory_recall","arguments":""}}]}}]}"#;
        assert_eq!(parse_openai_event(data).unwrap(), None);
    }

    proptest! {
        #[test]
        fn test_parse_never_panics(s in ".*") {
            let _ = parse_openai_event(&s);
        }
    }

    // ── ToolCallAcc ───────────────────────────────────────────────────────────

    #[test]
    fn test_acc_empty_initially() {
        let acc = ToolCallAcc::default();
        assert!(!acc.has_calls());
        assert_eq!(acc.to_xml(), "");
    }

    #[test]
    fn test_acc_single_tool_call_single_chunk() {
        let mut acc = ToolCallAcc::default();
        // Typical first (and only) chunk: name + full arguments
        let v = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "memory_recall",
                            "arguments": "{\"query\":\"recent events\"}"
                        }
                    }]
                }
            }]
        });
        acc.feed(&v);
        assert!(acc.has_calls());
        let xml = acc.to_xml();
        assert!(xml.contains("<tool_call>"));
        assert!(xml.contains("</tool_call>"));
        assert!(xml.contains("\"name\":\"memory_recall\""));
        assert!(xml.contains("\"recent events\""));
    }

    #[test]
    fn test_acc_arguments_assembled_from_chunks() {
        let mut acc = ToolCallAcc::default();
        // First chunk: name, empty arguments
        acc.feed(&serde_json::json!({
            "choices": [{"delta": {"tool_calls": [{
                "index": 0, "id": "call_1", "type": "function",
                "function": {"name": "file_read", "arguments": ""}
            }]}}]
        }));
        // Second chunk: first arguments fragment
        acc.feed(&serde_json::json!({
            "choices": [{"delta": {"tool_calls": [{
                "index": 0,
                "function": {"arguments": "{\"path\":"}
            }]}}]
        }));
        // Third chunk: remaining arguments
        acc.feed(&serde_json::json!({
            "choices": [{"delta": {"tool_calls": [{
                "index": 0,
                "function": {"arguments": "\"/tmp/test.txt\"}"}
            }]}}]
        }));

        let xml = acc.to_xml();
        assert!(xml.contains("file_read"));
        assert!(xml.contains("/tmp/test.txt"));
        // Arguments should be valid JSON inside the XML
        assert!(xml.contains("<tool_call>"));
        assert!(xml.contains("</tool_call>"));
    }

    #[test]
    fn test_acc_multiple_tool_calls() {
        let mut acc = ToolCallAcc::default();
        // Two tool calls in a single chunk
        acc.feed(&serde_json::json!({
            "choices": [{"delta": {"tool_calls": [
                {
                    "index": 0, "id": "call_1", "type": "function",
                    "function": {"name": "memory_store", "arguments": "{\"key\":\"k\",\"value\":\"v\"}"}
                },
                {
                    "index": 1, "id": "call_2", "type": "function",
                    "function": {"name": "memory_recall", "arguments": "{\"query\":\"q\"}"}
                }
            ]}}]
        }));

        let xml = acc.to_xml();
        // Both tool calls must appear in order
        let pos_store = xml.find("memory_store").unwrap();
        let pos_recall = xml.find("memory_recall").unwrap();
        assert!(pos_store < pos_recall, "tool calls should be ordered by index");
        // Each must be wrapped in its own <tool_call> block
        assert_eq!(xml.matches("<tool_call>").count(), 2);
        assert_eq!(xml.matches("</tool_call>").count(), 2);
    }

    #[test]
    fn test_acc_xml_is_parseable_by_agent() {
        // Verify that to_xml() output is accepted by the agent's extract_tool_calls parser.
        use crate::agent::parser::extract_tool_calls;
        let mut acc = ToolCallAcc::default();
        acc.feed(&serde_json::json!({
            "choices": [{"delta": {"tool_calls": [{
                "index": 0, "id": "call_1", "type": "function",
                "function": {
                    "name": "memory_recall",
                    "arguments": "{\"query\":\"Rust async patterns\"}"
                }
            }]}}]
        }));
        let xml = acc.to_xml();
        let calls = extract_tool_calls(&xml);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "memory_recall");
        assert_eq!(calls[0].args["query"], "Rust async patterns");
    }

    #[test]
    fn test_acc_malformed_arguments_fallback_to_empty_object() {
        // Truncated / invalid JSON in arguments must not panic; args fall back to {}.
        let mut acc = ToolCallAcc::default();
        acc.feed(&serde_json::json!({
            "choices": [{"delta": {"tool_calls": [{
                "index": 0, "id": "c", "type": "function",
                "function": {"name": "some_tool", "arguments": "{truncated"}
            }]}}]
        }));
        let xml = acc.to_xml();
        assert!(xml.contains("some_tool"));
        // Args fallback: {} → the XML should still be valid
        use crate::agent::parser::extract_tool_calls;
        let calls = extract_tool_calls(&xml);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "some_tool");
        assert!(calls[0].args.is_object());
    }

    #[test]
    fn test_acc_no_tool_calls_field_is_noop() {
        // Events with no tool_calls field must not affect the accumulator.
        let mut acc = ToolCallAcc::default();
        acc.feed(&serde_json::json!({
            "choices": [{"delta": {"content": "Hello"}}]
        }));
        assert!(!acc.has_calls());
        assert_eq!(acc.to_xml(), "");
    }

    #[test]
    fn test_acc_feed_ignores_missing_choices() {
        // Malformed / unexpected event shapes must not panic.
        let mut acc = ToolCallAcc::default();
        acc.feed(&serde_json::json!({}));
        acc.feed(&serde_json::json!({"choices": []}));
        acc.feed(&serde_json::json!({"choices": [{"delta": {}}]}));
        assert!(!acc.has_calls());
    }

    proptest! {
        #[test]
        fn test_acc_feed_never_panics(s in ".*") {
            let mut acc = ToolCallAcc::default();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                acc.feed(&v);
            }
        }
    }

    // ── normalise_base_url ────────────────────────────────────────────────────

    #[test]
    fn test_normalise_base_url_appends_v1() {
        assert_eq!(
            normalise_base_url("https://api.groq.com/openai"),
            "https://api.groq.com/openai/v1"
        );
        assert_eq!(
            normalise_base_url("https://api.groq.com/openai/v1"),
            "https://api.groq.com/openai/v1"
        );
        assert_eq!(
            normalise_base_url("https://api.groq.com/openai/"),
            "https://api.groq.com/openai/v1"
        );
    }
}
