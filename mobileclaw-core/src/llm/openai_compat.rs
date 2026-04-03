use async_trait::async_trait;
use futures::StreamExt;
use eventsource_stream::Eventsource;
use async_stream::stream;

use crate::{ClawError, ClawResult, llm::{client::{EventStream, LlmClient}, types::{ContentBlock, Message, Role, StreamEvent, ToolSpec}}};

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
/// Once `[DONE]` is received, call `drain_as_events()` to produce `StreamEvent::ToolUse`
/// events for each accumulated call.
#[derive(Default)]
pub(crate) struct ToolCallAcc {
    // BTreeMap preserves insertion order by index for deterministic output.
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

    /// Drain accumulated tool calls as `StreamEvent::ToolUse` events, then clear.
    ///
    /// Each call's arguments string is parsed as JSON; malformed JSON falls back to `{}`.
    pub(crate) fn drain_as_events(&mut self) -> Vec<StreamEvent> {
        let mut events = Vec::with_capacity(self.calls.len());
        for entry in self.calls.values() {
            let input: serde_json::Value = serde_json::from_str(&entry.arguments)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            events.push(StreamEvent::ToolUse {
                id: entry.id.clone(),
                name: entry.name.clone(),
                input,
            });
        }
        self.calls.clear();
        events
    }
}

// ─── Message serialization helpers ───────────────────────────────────────────

/// Serialize a slice of `Message` into the OpenAI `messages` array format,
/// handling `ContentBlock::ToolUse` (assistant tool_calls) and
/// `ContentBlock::ToolResult` (tool-role messages).
fn serialize_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = Vec::with_capacity(messages.len());
    for m in messages {
        let role_str = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
        };

        // Collect text blocks and tool_use blocks for the assistant message.
        let mut text_parts: Vec<&str> = Vec::new();
        let mut tool_calls: Vec<serde_json::Value> = Vec::new();
        let mut tool_results: Vec<serde_json::Value> = Vec::new();

        for block in &m.content {
            match block {
                ContentBlock::Text { text } => {
                    text_parts.push(text.as_str());
                }
                ContentBlock::ToolUse { id, name, input } => {
                    // arguments must be a JSON-encoded string, not an object.
                    let arguments = serde_json::to_string(input)
                        .unwrap_or_else(|_| "{}".to_string());
                    tool_calls.push(serde_json::json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments,
                        }
                    }));
                }
                ContentBlock::ToolResult { tool_use_id, content, .. } => {
                    tool_results.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content,
                    }));
                }
            }
        }

        // Emit the primary message (user / assistant / system / tool).
        if !tool_results.is_empty() {
            // Messages that only contain ToolResult blocks are expanded into
            // separate tool-role messages; no primary message is emitted.
            out.extend(tool_results);
        } else {
            let text_content = text_parts.join("");
            let mut msg = serde_json::json!({
                "role": role_str,
                "content": text_content,
            });
            if !tool_calls.is_empty() {
                msg["tool_calls"] = serde_json::Value::Array(tool_calls);
            }
            out.push(msg);
        }
    }
    out
}

// ─── LlmClient implementation ─────────────────────────────────────────────────

#[async_trait]
impl LlmClient for OpenAiCompatClient {
    /// OpenAI-compat providers support native function calling.
    fn native_tool_support(&self) -> bool {
        true
    }

    async fn stream_messages(
        &self,
        system: &str,
        messages: &[Message],
        max_tokens: u32,
        tools: &[ToolSpec],
    ) -> ClawResult<EventStream> {
        let mut msg_array = vec![serde_json::json!({"role":"system","content":system})];
        msg_array.extend(serialize_messages(messages));

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": msg_array,
            "stream": true,
        });

        // Inject tools into the request when the caller provides them.
        if !tools.is_empty() {
            let tools_json: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            }).collect();
            body["tools"] = serde_json::Value::Array(tools_json);
            body["tool_choice"] = serde_json::json!("auto");
        }

        let url = format!("{}/chat/completions", self.base_url);
        tracing::debug!(
            url = %url,
            model = %self.model,
            messages = msg_array.len(),
            max_tokens,
            tools = tools.len(),
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
        // Fix: use async_stream to iterate SSE events with a local ToolCallAcc.
        // Accumulated calls are emitted as StreamEvent::ToolUse on [DONE] **or**
        // when the byte stream ends without a [DONE] sentinel (some gateway
        // implementations close the connection without sending [DONE]).
        let url_for_log = url.clone();
        let s = stream! {
            yield Ok(StreamEvent::MessageStart);

            let mut acc = ToolCallAcc::default();
            let mut byte_stream = resp.bytes_stream().eventsource();

            while let Some(ev) = byte_stream.next().await {
                match ev {
                    Ok(e) => {
                        tracing::trace!(
                            data_preview = &e.data[..e.data.len().min(120)],
                            "OpenAiCompatClient: SSE event"
                        );
                        if e.data == "[DONE]" {
                            tracing::debug!(
                                url = %url_for_log,
                                tool_calls = acc.calls.len(),
                                "[DONE] received"
                            );
                            if acc.has_calls() {
                                tracing::debug!(count = acc.calls.len(), "emitting native tool calls as ToolUse events");
                                for event in acc.drain_as_events() {
                                    yield Ok(event);
                                }
                            }
                            yield Ok(StreamEvent::MessageStop);
                            return;
                        }
                        let v: serde_json::Value = match serde_json::from_str(&e.data) {
                            Ok(v) => v,
                            Err(e) => { yield Err(ClawError::Parse(e.to_string())); continue; }
                        };
                        let had_calls_before = acc.has_calls();
                        acc.feed(&v);
                        if !had_calls_before && acc.has_calls() {
                            tracing::debug!("OpenAiCompatClient: first tool_call chunk accumulated");
                        }
                        let text = v["choices"][0]["delta"]["content"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        if !text.is_empty() {
                            yield Ok(StreamEvent::TextDelta { text });
                        }
                    }
                    Err(e) => yield Err(ClawError::Llm(e.to_string())),
                }
            }

            // Stream ended without [DONE] — flush any accumulated tool calls.
            tracing::debug!(
                url = %url_for_log,
                tool_calls = acc.calls.len(),
                "OpenAiCompatClient: stream ended without [DONE], flushing accumulator"
            );
            if acc.has_calls() {
                tracing::debug!(count = acc.calls.len(), "emitting native tool calls as ToolUse events (no-DONE flush)");
                for event in acc.drain_as_events() {
                    yield Ok(event);
                }
            }
            yield Ok(StreamEvent::MessageStop);
        };

        Ok(Box::pin(s))
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
        let events = acc.drain_as_events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolUse { id, name, input } => {
                assert_eq!(id, "call_abc");
                assert_eq!(name, "memory_recall");
                assert_eq!(input["query"], "recent events");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
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

        let events = acc.drain_as_events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolUse { name, input, .. } => {
                assert_eq!(name, "file_read");
                assert_eq!(input["path"], "/tmp/test.txt");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
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

        let events = acc.drain_as_events();
        assert_eq!(events.len(), 2, "both tool calls must be emitted");
        // Verify order: index 0 first
        match &events[0] {
            StreamEvent::ToolUse { name, .. } => assert_eq!(name, "memory_store"),
            other => panic!("expected ToolUse, got {other:?}"),
        }
        match &events[1] {
            StreamEvent::ToolUse { name, .. } => assert_eq!(name, "memory_recall"),
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn test_acc_drain_clears_calls() {
        let mut acc = ToolCallAcc::default();
        acc.feed(&serde_json::json!({
            "choices": [{"delta": {"tool_calls": [{
                "index": 0, "id": "call_1", "type": "function",
                "function": {"name": "memory_recall", "arguments": "{\"query\":\"q\"}"}
            }]}}]
        }));
        assert!(acc.has_calls());
        let events = acc.drain_as_events();
        assert_eq!(events.len(), 1);
        // After drain, accumulator must be clear.
        assert!(!acc.has_calls());
        assert!(acc.drain_as_events().is_empty());
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
        let events = acc.drain_as_events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolUse { name, input, .. } => {
                assert_eq!(name, "some_tool");
                assert!(input.is_object());
                assert_eq!(input.as_object().unwrap().len(), 0);
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn test_acc_no_tool_calls_field_is_noop() {
        // Events with no tool_calls field must not affect the accumulator.
        let mut acc = ToolCallAcc::default();
        acc.feed(&serde_json::json!({
            "choices": [{"delta": {"content": "Hello"}}]
        }));
        assert!(!acc.has_calls());
        assert!(acc.drain_as_events().is_empty());
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

    // ── native_tool_support ───────────────────────────────────────────────────

    #[test]
    fn test_native_tool_support_is_true() {
        let client = OpenAiCompatClient {
            base_url: "https://example.com/v1".into(),
            api_key: "key".into(),
            model: "gpt-4".into(),
            http: reqwest::Client::new(),
        };
        assert!(client.native_tool_support());
    }

    // ── serialize_messages ────────────────────────────────────────────────────

    #[test]
    fn test_serialize_messages_text_only() {
        use crate::llm::types::{ContentBlock, Message, Role};
        let msgs = vec![
            Message { role: Role::User, content: vec![ContentBlock::Text { text: "hello".into() }] },
            Message { role: Role::Assistant, content: vec![ContentBlock::Text { text: "world".into() }] },
        ];
        let out = serialize_messages(&msgs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"], "hello");
        assert_eq!(out[1]["role"], "assistant");
        assert_eq!(out[1]["content"], "world");
    }

    #[test]
    fn test_serialize_messages_tool_use_in_assistant() {
        use crate::llm::types::{ContentBlock, Message, Role};
        let msgs = vec![Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text { text: "calling tool".into() },
                ContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "memory_recall".into(),
                    input: serde_json::json!({"query": "test"}),
                },
            ],
        }];
        let out = serialize_messages(&msgs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "calling tool");
        let tool_calls = out[0]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["id"], "call_1");
        assert_eq!(tool_calls[0]["type"], "function");
        assert_eq!(tool_calls[0]["function"]["name"], "memory_recall");
        // arguments must be a JSON-encoded string
        let args_str = tool_calls[0]["function"]["arguments"].as_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(args_str).unwrap();
        assert_eq!(parsed["query"], "test");
    }

    #[test]
    fn test_serialize_messages_tool_result_expands_to_tool_role() {
        use crate::llm::types::{ContentBlock, Message, Role};
        let msgs = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: "42".into(),
                is_error: false,
            }],
        }];
        let out = serialize_messages(&msgs);
        assert_eq!(out.len(), 1, "ToolResult expands to a single tool-role message");
        assert_eq!(out[0]["role"], "tool");
        assert_eq!(out[0]["tool_call_id"], "call_1");
        assert_eq!(out[0]["content"], "42");
    }

    #[test]
    fn test_serialize_messages_multiple_tool_results() {
        use crate::llm::types::{ContentBlock, Message, Role};
        let msgs = vec![Message {
            role: Role::User,
            content: vec![
                ContentBlock::ToolResult {
                    tool_use_id: "call_1".into(),
                    content: "result_a".into(),
                    is_error: false,
                },
                ContentBlock::ToolResult {
                    tool_use_id: "call_2".into(),
                    content: "result_b".into(),
                    is_error: false,
                },
            ],
        }];
        let out = serialize_messages(&msgs);
        assert_eq!(out.len(), 2, "two ToolResults expand to two tool-role messages");
        assert_eq!(out[0]["tool_call_id"], "call_1");
        assert_eq!(out[1]["tool_call_id"], "call_2");
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
