use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;
use crate::{ClawError, ClawResult, llm::types::{Message, StreamEvent, ToolSpec}};

pub type EventStream = Pin<Box<dyn Stream<Item = ClawResult<StreamEvent>> + Send>>;

#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 发送消息，返回流式事件
    async fn stream_messages(
        &self,
        system: &str,
        messages: &[Message],
        max_tokens: u32,
        tools: &[ToolSpec],
    ) -> ClawResult<EventStream>;

    /// Returns true if this provider supports native API tool calling.
    /// When false, the agent loop uses XML-based tool invocation.
    fn native_tool_support(&self) -> bool {
        false
    }
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

#[async_trait]
impl LlmClient for ClaudeClient {
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
        use futures::StreamExt;
        use eventsource_stream::Eventsource;
        use async_stream::stream;

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "system": system,
            "messages": messages,
            "stream": true,
        });

        if !tools.is_empty() {
            let tools_array: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            }).collect();
            body["tools"] = serde_json::Value::Array(tools_array);
            body["tool_choice"] = serde_json::json!({"type": "auto"});
        }

        tracing::debug!(
            model = %self.model,
            messages = messages.len(),
            max_tokens,
            tools = tools.len(),
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

        // Use async_stream to maintain mutable accumulator across SSE events.
        // The accumulator tracks the in-progress tool_use block:
        //   Some((id, name, partial_json_acc)) while receiving input_json_delta fragments,
        //   None when no tool_use block is in progress.
        let s = stream! {
            let mut tool_acc: Option<(String, String, String)> = None;
            let mut byte_stream = resp.bytes_stream().eventsource();

            while let Some(event) = byte_stream.next().await {
                match event {
                    Ok(ev) if ev.event == "message_start" => {
                        yield Ok(StreamEvent::MessageStart);
                    }
                    Ok(ev) if ev.event == "message_stop" => {
                        yield Ok(StreamEvent::MessageStop);
                    }
                    Ok(ev) if ev.event == "content_block_start" => {
                        let v: serde_json::Value = match serde_json::from_str(&ev.data) {
                            Ok(v) => v,
                            Err(e) => {
                                yield Err(ClawError::Parse(e.to_string()));
                                continue;
                            }
                        };
                        if v["content_block"]["type"].as_str() == Some("tool_use") {
                            let id = v["content_block"]["id"].as_str().unwrap_or("").to_string();
                            let name = v["content_block"]["name"].as_str().unwrap_or("").to_string();
                            tracing::debug!(tool_id = %id, tool_name = %name, "ClaudeClient: tool_use block started");
                            tool_acc = Some((id, name, String::new()));
                        }
                    }
                    Ok(ev) if ev.event == "content_block_delta" => {
                        let v: serde_json::Value = match serde_json::from_str(&ev.data) {
                            Ok(v) => v,
                            Err(e) => {
                                yield Err(ClawError::Parse(e.to_string()));
                                continue;
                            }
                        };
                        match v["delta"]["type"].as_str() {
                            Some("text_delta") => {
                                let text = v["delta"]["text"].as_str().unwrap_or("").to_string();
                                if !text.is_empty() {
                                    yield Ok(StreamEvent::TextDelta { text });
                                }
                            }
                            Some("input_json_delta") => {
                                if let Some(chunk) = v["delta"]["partial_json"].as_str() {
                                    if let Some((_, _, ref mut acc)) = tool_acc {
                                        acc.push_str(chunk);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(ev) if ev.event == "content_block_stop" => {
                        if let Some((id, name, json_str)) = tool_acc.take() {
                            let input = serde_json::from_str::<serde_json::Value>(&json_str)
                                .unwrap_or_else(|e| {
                                    tracing::warn!(
                                        tool_id = %id,
                                        tool_name = %name,
                                        error = %e,
                                        "ClaudeClient: failed to parse tool input JSON; falling back to empty object"
                                    );
                                    serde_json::Value::Object(Default::default())
                                });
                            tracing::debug!(tool_id = %id, tool_name = %name, "ClaudeClient: emitting ToolUse event");
                            yield Ok(StreamEvent::ToolUse { id, name, input });
                        }
                    }
                    Ok(_) => {
                        // Ignore other event types (ping, message_delta, etc.)
                    }
                    Err(e) => yield Err(ClawError::Llm(e.to_string())),
                }
            }
        };

        Ok(Box::pin(s))
    }
}

#[async_trait]
impl LlmClient for std::sync::Arc<dyn LlmClient> {
    async fn stream_messages(
        &self,
        system: &str,
        messages: &[crate::llm::types::Message],
        max_tokens: u32,
        tools: &[ToolSpec],
    ) -> crate::ClawResult<EventStream> {
        self.as_ref().stream_messages(system, messages, max_tokens, tools).await
    }

    fn native_tool_support(&self) -> bool {
        self.as_ref().native_tool_support()
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
        /// Each entry is `(id, name, input)` for a `StreamEvent::ToolUse` to emit
        /// after the text delta events.
        pub tool_uses: Vec<(String, String, serde_json::Value)>,
        /// When true, `native_tool_support()` returns true and ToolUse events are
        /// handled by the native path in AgentLoop.
        pub native: bool,
    }

    impl MockLlmClient {
        pub fn new(response: impl Into<String>) -> Self {
            Self { response: response.into(), tool_uses: vec![], native: false }
        }

        /// Create a client that reports native tool support and emits ToolUse events.
        pub fn new_native(response: impl Into<String>, tool_uses: Vec<(String, String, serde_json::Value)>) -> Self {
            Self { response: response.into(), tool_uses, native: true }
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for MockLlmClient {
        fn native_tool_support(&self) -> bool {
            self.native
        }

        async fn stream_messages(
            &self,
            _system: &str,
            _messages: &[crate::llm::types::Message],
            _max_tokens: u32,
            _tools: &[ToolSpec],
        ) -> crate::ClawResult<EventStream> {
            let text = self.response.clone();
            let mut events: Vec<crate::ClawResult<StreamEvent>> = vec![
                Ok(StreamEvent::MessageStart),
                Ok(StreamEvent::TextDelta { text }),
            ];
            for (id, name, input) in &self.tool_uses {
                events.push(Ok(StreamEvent::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                }));
            }
            events.push(Ok(StreamEvent::MessageStop));
            Ok(Box::pin(stream::iter(events)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::ToolSpec;

    /// Build the request body JSON the same way ClaudeClient does, so we can
    /// test the shape without making an actual HTTP call.
    fn build_request_body(tools: &[ToolSpec]) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024u32,
            "system": "system",
            "messages": [],
            "stream": true,
        });
        if !tools.is_empty() {
            let tools_array: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            }).collect();
            body["tools"] = serde_json::Value::Array(tools_array);
            body["tool_choice"] = serde_json::json!({"type": "auto"});
        }
        body
    }

    #[test]
    fn request_body_no_tools_omits_tools_and_tool_choice() {
        let body = build_request_body(&[]);
        assert!(body.get("tools").is_none(), "tools key should be absent when no tools");
        assert!(body.get("tool_choice").is_none(), "tool_choice key should be absent when no tools");
    }

    #[test]
    fn request_body_with_tools_includes_tools_and_tool_choice() {
        let tools = vec![ToolSpec {
            name: "memory_recall".into(),
            description: "Recall a memory".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        }];
        let body = build_request_body(&tools);

        let tools_val = body.get("tools").expect("tools key must be present");
        assert!(tools_val.is_array(), "tools must be an array");
        let arr = tools_val.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "memory_recall");
        assert_eq!(arr[0]["description"], "Recall a memory");
        assert!(arr[0]["input_schema"].is_object(), "input_schema must be an object");

        let tc = body.get("tool_choice").expect("tool_choice key must be present");
        assert_eq!(tc["type"], "auto");
    }

    #[test]
    fn request_body_multiple_tools_all_included() {
        let tools = vec![
            ToolSpec {
                name: "tool_a".into(),
                description: "A".into(),
                input_schema: serde_json::json!({"type": "object"}),
            },
            ToolSpec {
                name: "tool_b".into(),
                description: "B".into(),
                input_schema: serde_json::json!({"type": "object"}),
            },
        ];
        let body = build_request_body(&tools);
        let arr = body["tools"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "tool_a");
        assert_eq!(arr[1]["name"], "tool_b");
    }

    // ── SSE tool_use accumulation logic (tested in isolation) ────────────────
    //
    // The full SSE streaming path requires a live HTTP connection to
    // api.anthropic.com and cannot be unit-tested without a mock HTTP server.
    // The accumulation logic is reproduced inline below so the key invariants
    // can be verified without new test dependencies.

    /// Simulates the tool_use SSE accumulation logic extracted from stream_messages.
    fn simulate_tool_use_sse(events: &[(&str, serde_json::Value)]) -> Vec<StreamEvent> {
        let mut tool_acc: Option<(String, String, String)> = None;
        let mut out = vec![];

        for (event_type, data) in events {
            match *event_type {
                "content_block_start" => {
                    if data["content_block"]["type"].as_str() == Some("tool_use") {
                        let id = data["content_block"]["id"].as_str().unwrap_or("").to_string();
                        let name = data["content_block"]["name"].as_str().unwrap_or("").to_string();
                        tool_acc = Some((id, name, String::new()));
                    }
                }
                "content_block_delta" => {
                    match data["delta"]["type"].as_str() {
                        Some("text_delta") => {
                            let text = data["delta"]["text"].as_str().unwrap_or("").to_string();
                            if !text.is_empty() {
                                out.push(StreamEvent::TextDelta { text });
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(chunk) = data["delta"]["partial_json"].as_str() {
                                if let Some((_, _, ref mut acc)) = tool_acc {
                                    acc.push_str(chunk);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                "content_block_stop" => {
                    if let Some((id, name, json_str)) = tool_acc.take() {
                        let input = serde_json::from_str::<serde_json::Value>(&json_str)
                            .unwrap_or(serde_json::Value::Object(Default::default()));
                        out.push(StreamEvent::ToolUse { id, name, input });
                    }
                }
                _ => {}
            }
        }
        out
    }

    #[test]
    fn sse_tool_use_single_chunk_input() {
        let events = vec![
            ("content_block_start", serde_json::json!({
                "index": 0,
                "content_block": {"type": "tool_use", "id": "toolu_001", "name": "memory_recall", "input": {}}
            })),
            ("content_block_delta", serde_json::json!({
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "{\"query\":\"test\"}"}
            })),
            ("content_block_stop", serde_json::json!({"index": 0})),
        ];

        let result = simulate_tool_use_sse(&events);
        assert_eq!(result.len(), 1);
        match &result[0] {
            StreamEvent::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_001");
                assert_eq!(name, "memory_recall");
                assert_eq!(input["query"], "test");
            }
            other => panic!("expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn sse_tool_use_multiple_partial_json_chunks_concatenated() {
        // Anthropic typically sends partial_json in multiple fragments.
        let events = vec![
            ("content_block_start", serde_json::json!({
                "index": 0,
                "content_block": {"type": "tool_use", "id": "toolu_002", "name": "file_read", "input": {}}
            })),
            ("content_block_delta", serde_json::json!({
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}
            })),
            ("content_block_delta", serde_json::json!({
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "\"/tmp/foo.txt\"}"}
            })),
            ("content_block_stop", serde_json::json!({"index": 0})),
        ];

        let result = simulate_tool_use_sse(&events);
        assert_eq!(result.len(), 1);
        match &result[0] {
            StreamEvent::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_002");
                assert_eq!(name, "file_read");
                assert_eq!(input["path"], "/tmp/foo.txt");
            }
            other => panic!("expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn sse_tool_use_malformed_json_falls_back_to_empty_object() {
        let events = vec![
            ("content_block_start", serde_json::json!({
                "index": 0,
                "content_block": {"type": "tool_use", "id": "toolu_003", "name": "some_tool", "input": {}}
            })),
            ("content_block_delta", serde_json::json!({
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "{truncated"}
            })),
            ("content_block_stop", serde_json::json!({"index": 0})),
        ];

        let result = simulate_tool_use_sse(&events);
        assert_eq!(result.len(), 1);
        match &result[0] {
            StreamEvent::ToolUse { name, input, .. } => {
                assert_eq!(name, "some_tool");
                assert!(input.is_object(), "fallback must be an object");
                assert_eq!(input.as_object().unwrap().len(), 0, "fallback must be empty object");
            }
            other => panic!("expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn sse_text_block_not_affected_by_tool_use_path() {
        // Text deltas before a tool_use block should be emitted as TextDelta.
        let events = vec![
            ("content_block_delta", serde_json::json!({
                "index": 0,
                "delta": {"type": "text_delta", "text": "Hello "}
            })),
            ("content_block_delta", serde_json::json!({
                "index": 0,
                "delta": {"type": "text_delta", "text": "world"}
            })),
        ];

        let result = simulate_tool_use_sse(&events);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], StreamEvent::TextDelta { text: "Hello ".into() });
        assert_eq!(result[1], StreamEvent::TextDelta { text: "world".into() });
    }

    #[test]
    fn claude_client_native_tool_support_returns_true() {
        let client = ClaudeClient::new("key", "claude-3-5-sonnet-20241022");
        assert!(client.native_tool_support());
    }
}
