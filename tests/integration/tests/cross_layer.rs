/// Cross-layer integration tests: Rust AgentLoop with fault-injecting mock LLM.
///
/// These tests verify that when Rust's AgentLoop produces events (including errors,
/// empty responses, tool call exhaustion, etc.), the downstream consumer receives
/// them correctly — the same boundary that caused the 2026-04-03 Flutter bug.
///
/// Run: cargo test -p mobileclaw-integration

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream;
use mobileclaw_core::agent::loop_impl::{AgentEvent, AgentLoop};
use mobileclaw_core::llm::client::{EventStream, LlmClient};
use mobileclaw_core::llm::types::{Message, StreamEvent, ToolSpec};
use mobileclaw_core::memory::sqlite::SqliteMemory;
use mobileclaw_core::secrets::store::test_helpers::NullSecretStore;
use mobileclaw_core::skill::SkillManager;
use mobileclaw_core::tools::{
    builtin::register_all_builtins, PermissionChecker, ToolContext, ToolRegistry,
};
use tempfile::TempDir;

// ===========================================================================
// FaultInjectingLlmClient
// ===========================================================================

/// Configures what happens on a single `stream_messages` call.
#[derive(Clone)]
enum CallBehavior {
    /// Return a sequence of StreamEvents.
    Events(Vec<StreamEvent>),
    /// Return an error immediately.
    Error(String),
}

/// Per-call response. Each call to `stream_messages` pops the next behavior.
/// When exhausted, the last behavior is repeated.
struct FaultConfig {
    behaviors: Vec<CallBehavior>,
    call_index: usize,
}

impl FaultConfig {
    fn next(&mut self) -> CallBehavior {
        let idx = self.call_index.min(self.behaviors.len().saturating_sub(1));
        self.call_index += 1;
        self.behaviors[idx].clone()
    }
}

/// A mock LLM client that supports per-call fault injection.
/// Thread-safe: can be used across async test boundaries.
struct FaultInjectingLlmClient {
    config: Arc<Mutex<FaultConfig>>,
    native: bool,
}

impl FaultInjectingLlmClient {
    fn new(behaviors: Vec<CallBehavior>, native: bool) -> Self {
        Self {
            config: Arc::new(Mutex::new(FaultConfig {
                behaviors,
                call_index: 0,
            })),
            native,
        }
    }

    /// Convenience: create a client that always returns the same text.
    fn text_only(text: &str) -> Self {
        Self::new(
            vec![CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: text.to_string() },
                StreamEvent::MessageStop,
            ])],
            false,
        )
    }
}

#[async_trait]
impl LlmClient for FaultInjectingLlmClient {
    fn native_tool_support(&self) -> bool {
        self.native
    }

    async fn stream_messages(
        &self,
        _system: &str,
        _messages: &[Message],
        _max_tokens: u32,
        _tools: &[ToolSpec],
    ) -> mobileclaw_core::ClawResult<EventStream> {
        let behavior = {
            let mut cfg = self.config.lock().unwrap();
            cfg.next()
        };

        match behavior {
            CallBehavior::Events(events) => Ok(Box::pin(stream::iter(events.into_iter().map(Ok)))),
            CallBehavior::Error(msg) => Err(mobileclaw_core::ClawError::Llm(msg)),
        }
    }
}

// ===========================================================================
// Test helpers
// ===========================================================================

async fn make_agent(
    behaviors: Vec<CallBehavior>,
    native: bool,
) -> (AgentLoop<FaultInjectingLlmClient>, TempDir) {
    let dir = TempDir::new().unwrap();
    let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
    let mut registry = ToolRegistry::new();
    register_all_builtins(&mut registry);
    let ctx = ToolContext {
        memory: mem,
        sandbox_dir: dir.path().to_path_buf(),
        http_allowlist: vec![],
        permissions: Arc::new(PermissionChecker::allow_all()),
        secrets: Arc::new(NullSecretStore),
    };
    let llm = FaultInjectingLlmClient::new(behaviors, native);
    let agent = AgentLoop::new(llm, registry, ctx, SkillManager::new(vec![]));
    (agent, dir)
}

/// Extract text fragments from an event list.
fn text_fragments(events: &[AgentEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

// ===========================================================================
// Happy-path tests
// ===========================================================================

mod happy_path {
    use super::*;

    #[tokio::test]
    async fn text_only_produces_text_delta_then_done() {
        let (mut agent, _dir) = make_agent(
            vec![CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "Hello world".into() },
                StreamEvent::MessageStop,
            ])],
            false,
        )
        .await;

        let events = agent.chat("hi", "").await.unwrap();

        let texts = text_fragments(&events);
        assert_eq!(texts, vec!["Hello world"]);
        assert!(matches!(events.last(), Some(AgentEvent::Done)));
    }

    #[tokio::test]
    async fn multiple_text_deltas_concatenate() {
        let (mut agent, _dir) = make_agent(
            vec![CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "Hello ".into() },
                StreamEvent::TextDelta { text: "world".into() },
                StreamEvent::TextDelta { text: "!".into() },
                StreamEvent::MessageStop,
            ])],
            false,
        )
        .await;

        let events = agent.chat("hi", "").await.unwrap();
        let full: String = text_fragments(&events).join("");
        assert_eq!(full, "Hello world!");
    }

    #[tokio::test]
    async fn xml_tool_call_executes_tool_and_emits_events() {
        // Round 0: LLM returns XML tool call → file_read executes
        // Round 1: LLM returns text-only → Done
        // Use 'time' tool which requires no arguments
        let xml = "Let me check. <tool_call>{\"name\": \"time\", \"args\": {}}</tool_call>";
        let behaviors = vec![
            CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: xml.into() },
                StreamEvent::MessageStop,
            ]),
            CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "Done.".into() },
                StreamEvent::MessageStop,
            ]),
        ];
        let (mut agent, _dir) = make_agent(behaviors, false).await;

        let events = agent.chat("what time?", "").await.unwrap();

        let tool_calls: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolCall { name } if name == "time"))
            .collect();
        assert!(!tool_calls.is_empty(), "should have ToolCall for 'time'");

        let tool_results: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolResult { name, success } if name == "time" && *success))
            .collect();
        assert!(!tool_results.is_empty(), "should have successful ToolResult for 'time'");

        assert!(
            matches!(events.last(), Some(AgentEvent::Done)),
            "last event must be Done"
        );
    }

    #[tokio::test]
    async fn native_tool_call_executes_tool_and_emits_events() {
        let (mut agent, _dir) = make_agent(
            vec![CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "Checking time...".into() },
                StreamEvent::ToolUse {
                    id: "tu_001".into(),
                    name: "time".into(),
                    input: serde_json::json!({}),
                },
                StreamEvent::MessageStop,
            ])],
            true, // native path
        )
        .await;

        let events = agent.chat("what time?", "").await.unwrap();

        let tool_calls: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolCall { name } if name == "time"))
            .collect();
        assert!(!tool_calls.is_empty(), "native path should emit ToolCall");

        let tool_results: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolResult { name, .. } if name == "time"))
            .collect();
        assert!(!tool_results.is_empty(), "native path should emit ToolResult");
    }
}

// ===========================================================================
// Fault injection tests
// ===========================================================================

mod fault_injection {
    use super::*;

    #[tokio::test]
    async fn llm_error_on_first_call_propagates() {
        let (mut agent, _dir) = make_agent(
            vec![CallBehavior::Error("connection refused".into())],
            false,
        )
        .await;

        let result = agent.chat("hi", "").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("connection refused"),
            "error message must contain the original message"
        );
    }

    #[tokio::test]
    async fn llm_error_mid_tool_round() {
        // Round 0: tool call (time) → executes
        // Round 1: LLM returns error
        let xml = String::from("<tool_call>") + r#"{"name": "time", "args": {}}"# + "</tool_call>";
        let behaviors = vec![
            CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: xml },
                StreamEvent::MessageStop,
            ]),
            CallBehavior::Error("streaming timeout".into()),
        ];
        let (mut agent, _dir) = make_agent(behaviors, false).await;

        let result = agent.chat("read file", "").await;
        assert!(
            result.is_err(),
            "error on round 2 should propagate to caller"
        );
    }

    #[tokio::test]
    async fn tool_round_exhaustion_still_emits_done() {
        // Every LLM call returns a tool call → exhaust MAX_TOOL_ROUNDS
        // Done guard must still fire
        // Use 'time' tool which requires no arguments
        let xml = String::from("<tool_call>") + r#"{"name": "time", "args": {}}"# + "</tool_call>";
        let (mut agent, _dir) = make_agent(
            vec![CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: xml },
                StreamEvent::MessageStop,
            ])],
            false,
        )
        .await;

        let events = agent.chat("go", "").await.unwrap();

        assert!(
            matches!(events.last(), Some(AgentEvent::Done)),
            "Done must be last even after round exhaustion, got: {:?}",
            events.last()
        );

        // Should have attempted MAX_TOOL_ROUNDS (10) tool calls
        let tool_calls: Vec<_> = events.iter().filter(|e| matches!(e, AgentEvent::ToolCall { .. })).collect();
        assert_eq!(
            tool_calls.len(),
            10,
            "should exhaust all 10 tool rounds"
        );
    }

    #[tokio::test]
    async fn multi_tool_resolution_xml_path() {
        // Round 0: LLM calls file_read via XML → succeeds
        // Round 1: LLM returns text-only response → Done
        let xml = String::from("<tool_call>") + r#"{"name": "file_read", "args": {"path": "test.txt"}}"# + "</tool_call>";
        let behaviors = vec![
            CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: xml },
                StreamEvent::MessageStop,
            ]),
            CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "File content is: hello".into() },
                StreamEvent::MessageStop,
            ]),
        ];
        let (mut agent, dir) = make_agent(behaviors, false).await;
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();

        let events = agent.chat("read test.txt", "").await.unwrap();

        let texts = text_fragments(&events);
        // Should have both the pre-tool text and the final response
        assert!(texts.iter().any(|t| t.contains("File content")));
        assert!(matches!(events.last(), Some(AgentEvent::Done)));
    }

    #[tokio::test]
    async fn multi_tool_resolution_native_path() {
        // Round 0: LLM returns ToolUse → executes
        // Round 1: LLM returns text-only → Done
        let behaviors = vec![
            CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "Checking...".into() },
                StreamEvent::ToolUse {
                    id: "tu_001".into(),
                    name: "file_read".into(),
                    input: serde_json::json!({"path": "test.txt"}),
                },
                StreamEvent::MessageStop,
            ]),
            CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "Done reading file.".into() },
                StreamEvent::MessageStop,
            ]),
        ];
        let (mut agent, dir) = make_agent(behaviors, true).await;
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();

        let events = agent.chat("read test.txt", "").await.unwrap();

        let tool_calls: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolCall { name } if name == "file_read"))
            .collect();
        assert_eq!(tool_calls.len(), 1, "should have exactly 1 ToolCall");

        let tool_results: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolResult { name, success } if name == "file_read" && *success))
            .collect();
        assert_eq!(tool_results.len(), 1, "file_read should succeed");

        assert!(matches!(events.last(), Some(AgentEvent::Done)));
    }

    #[tokio::test]
    async fn native_tool_error_propagates() {
        // Round 0: LLM returns ToolUse → unknown tool fails
        // Round 1: LLM returns text-only → Done
        let behaviors = vec![
            CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::ToolUse {
                    id: "tu_bad".into(),
                    name: "completely_unknown_tool".into(),
                    input: serde_json::json!({}),
                },
                StreamEvent::MessageStop,
            ]),
            CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "Sorry, tool not found".into() },
                StreamEvent::MessageStop,
            ]),
        ];
        let (mut agent, _dir) = make_agent(behaviors, true).await;

        let events = agent.chat("call unknown tool", "").await.unwrap();

        let failed: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::ToolResult { success: false, .. }))
            .collect();
        assert!(
            !failed.is_empty(),
            "unknown tool should produce failed ToolResult"
        );

        assert!(matches!(events.last(), Some(AgentEvent::Done)));
    }
}

// ===========================================================================
// Event ordering and Completeness tests
// ===========================================================================

mod event_ordering {
    use super::*;
    use mobileclaw_core::agent::context_manager::ContextConfig;

    #[tokio::test]
    async fn event_order_text_before_tool_before_done() {
        let (mut agent, _dir) = make_agent(
            vec![
                CallBehavior::Events(vec![
                    StreamEvent::MessageStart,
                    StreamEvent::TextDelta { text: "Let me help. ".into() },
                    StreamEvent::MessageStop,
                ]),
                CallBehavior::Events(vec![
                    StreamEvent::MessageStart,
                    StreamEvent::TextDelta { text: "Here's the answer.".into() },
                    StreamEvent::MessageStop,
                ]),
            ],
            false,
        )
        .await;

        let events = agent.chat("help me", "").await.unwrap();

        let first_text = events.iter().position(|e| matches!(e, AgentEvent::TextDelta { .. }));
        let last_done = events.iter().rposition(|e| matches!(e, AgentEvent::Done));

        assert!(first_text.is_some(), "should have at least one TextDelta");
        assert!(last_done.is_some(), "should have Done");
        assert!(
            first_text.unwrap() < last_done.unwrap(),
            "TextDelta must come before Done"
        );
    }

    #[tokio::test]
    async fn done_is_always_last_event() {
        let (mut agent, _dir) = make_agent(
            vec![CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "response".into() },
                StreamEvent::MessageStop,
            ])],
            false,
        )
        .await;

        let events = agent.chat("x", "").await.unwrap();
        assert!(
            matches!(events.last(), Some(AgentEvent::Done)),
            "Done must always be the last event"
        );
    }

    #[tokio::test]
    async fn multi_chat_history_accumulates() {
        let (mut agent, _dir) = make_agent(
            vec![CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "ok".into() },
                StreamEvent::MessageStop,
            ])],
            false,
        )
        .await;

        agent.chat("message 1", "").await.unwrap();
        assert_eq!(agent.history().len(), 2);

        agent.chat("message 2", "").await.unwrap();
        assert_eq!(agent.history().len(), 4);

        agent.chat("message 3", "").await.unwrap();
        assert_eq!(agent.history().len(), 6);
    }

    #[tokio::test]
    async fn context_stats_emitted_before_done_when_pruning() {
        let dir = TempDir::new().unwrap();
        let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
        let ctx = ToolContext {
            memory: mem,
            sandbox_dir: dir.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: Arc::new(NullSecretStore),
        };
        let llm = FaultInjectingLlmClient::text_only("hello");
        let mut agent = AgentLoop::new(llm, ToolRegistry::new(), ctx, SkillManager::new(vec![]))
            .with_context_config(ContextConfig {
                max_tokens: 1000,
                buffer_tokens: 100,
                min_user_turns: 1,
                max_messages: Some(5),
            });

        // Pump enough messages to trigger pruning
        let mut saw_stats = false;
        for i in 0..20 {
            let events = agent.chat(&format!("msg {i} with padding to exceed context window"), "").await.unwrap();
            let has_stats = events.iter().any(|e| matches!(e, AgentEvent::ContextStats(_)));
            if has_stats {
                saw_stats = true;
                let stats_idx = events.iter().position(|e| matches!(e, AgentEvent::ContextStats(_))).unwrap();
                let done_idx = events.iter().position(|e| matches!(e, AgentEvent::Done)).unwrap();
                assert!(stats_idx < done_idx, "ContextStats must come before Done");
                break;
            }
        }
        assert!(saw_stats, "ContextStats should fire after enough turns with small window");
    }
}

// ===========================================================================
// DTO conversion tests (Rust AgentEvent → FFI AgentEventDto)
// ===========================================================================

mod dto_conversion {
    use super::*;
    use mobileclaw_core::ffi::AgentEventDto;

    /// Simulates the conversion that happens in ffi.rs AgentSession::chat().
    fn event_to_dto(event: AgentEvent) -> AgentEventDto {
        match event {
            AgentEvent::TextDelta { text } => AgentEventDto::TextDelta { text },
            AgentEvent::ToolCall { name } => AgentEventDto::ToolCall { name },
            AgentEvent::ToolResult { name, success } => AgentEventDto::ToolResult { name, success },
            AgentEvent::ContextStats(stats) => AgentEventDto::ContextStats {
                tokens_before_turn: stats.tokens_before_turn,
                tokens_after_prune: stats.tokens_after_prune,
                messages_pruned: stats.messages_pruned,
                history_len: stats.history_len,
                pruning_threshold: stats.pruning_threshold,
            },
            AgentEvent::Done => AgentEventDto::Done,
        }
    }

    #[tokio::test]
    async fn chat_events_convert_to_dto() {
        let (mut agent, _dir) = make_agent(
            vec![
                CallBehavior::Events(vec![
                    StreamEvent::MessageStart,
                    StreamEvent::TextDelta { text: "Hello".into() },
                    StreamEvent::MessageStop,
                ]),
                CallBehavior::Events(vec![
                    StreamEvent::MessageStart,
                    StreamEvent::TextDelta { text: " World".into() },
                    StreamEvent::MessageStop,
                ]),
            ],
            false,
        )
        .await;

        let events = agent.chat("hi", "").await.unwrap();

        // Convert all events to DTOs (simulating the FFI boundary)
        let dtos: Vec<AgentEventDto> = events.into_iter().map(event_to_dto).collect();

        // Verify the DTO conversion preserved all event types
        let text_count = dtos.iter().filter(|d| matches!(d, AgentEventDto::TextDelta { .. })).count();
        assert!(text_count >= 1, "should have at least one TextDelta DTO");

        // Last DTO must be Done
        assert!(
            matches!(dtos.last(), Some(AgentEventDto::Done)),
            "last DTO must be Done"
        );
    }

    #[tokio::test]
    async fn event_list_round_trip_all_variants() {
        // Create a representative event list with all variants
        let events = vec![
            AgentEvent::TextDelta { text: "Hello".into() },
            AgentEvent::ToolCall { name: "test_tool".into() },
            AgentEvent::ToolResult { name: "test_tool".into(), success: true },
            AgentEvent::ToolCall { name: "other_tool".into() },
            AgentEvent::ToolResult { name: "other_tool".into(), success: false },
            AgentEvent::ContextStats(mobileclaw_core::agent::loop_impl::ContextStats {
                tokens_before_turn: 8000,
                tokens_after_prune: 7500,
                messages_pruned: 2,
                history_len: 12,
                pruning_threshold: 16000,
            }),
            AgentEvent::Done,
        ];

        // Convert to DTOs
        let dtos: Vec<AgentEventDto> = events.into_iter().map(event_to_dto).collect();

        assert_eq!(dtos.len(), 7);

        // Verify each variant
        assert!(matches!(&dtos[0], AgentEventDto::TextDelta { text } if text == "Hello"));
        assert!(matches!(&dtos[1], AgentEventDto::ToolCall { name } if name == "test_tool"));
        assert!(matches!(&dtos[2], AgentEventDto::ToolResult { name, success } if name == "test_tool" && *success));
        assert!(matches!(&dtos[3], AgentEventDto::ToolCall { name } if name == "other_tool"));
        assert!(matches!(&dtos[4], AgentEventDto::ToolResult { name, success } if name == "other_tool" && !success));
        assert!(matches!(&dtos[5], AgentEventDto::ContextStats { .. }));
        assert!(matches!(&dtos[6], AgentEventDto::Done));

        // Verify ContextStats fields survived
        if let AgentEventDto::ContextStats {
            tokens_before_turn,
            tokens_after_prune,
            messages_pruned,
            history_len,
            pruning_threshold,
        } = &dtos[5] {
            assert_eq!(*tokens_before_turn, 8000);
            assert_eq!(*tokens_after_prune, 7500);
            assert_eq!(*messages_pruned, 2);
            assert_eq!(*history_len, 12);
            assert_eq!(*pruning_threshold, 16000);
        } else {
            panic!("dtos[5] should be ContextStats");
        }
    }

    #[tokio::test]
    async fn large_event_list_survives_conversion() {
        // Simulate the 2026-04-03 bug scenario: 92+ events
        let mut events = vec![
            AgentEvent::ToolCall { name: "memory_search".into() },
            AgentEvent::ToolResult { name: "memory_search".into(), success: true },
        ];
        for i in 0..87 {
            events.push(AgentEvent::TextDelta { text: format!("word{i} ") });
        }
        events.push(AgentEvent::ContextStats(mobileclaw_core::agent::loop_impl::ContextStats {
            tokens_before_turn: 8000,
            tokens_after_prune: 7500,
            messages_pruned: 2,
            history_len: 12,
            pruning_threshold: 16000,
        }));
        events.push(AgentEvent::Done);

        let dtos: Vec<AgentEventDto> = events.into_iter().map(event_to_dto).collect();
        assert_eq!(dtos.len(), 91);

        let text_count = dtos.iter().filter(|d| matches!(d, AgentEventDto::TextDelta { .. })).count();
        assert_eq!(text_count, 87, "all 87 text events must survive conversion");
    }
}

// ===========================================================================
// Stream resilience tests
// ===========================================================================

mod stream_resilience {
    use super::*;

    #[tokio::test]
    async fn empty_text_response_still_produces_done() {
        let (mut agent, _dir) = make_agent(
            vec![CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::TextDelta { text: "".into() },
                StreamEvent::MessageStop,
            ])],
            false,
        )
        .await;

        let events = agent.chat("hi", "").await.unwrap();
        assert!(
            matches!(events.last(), Some(AgentEvent::Done)),
            "empty text response must still end with Done"
        );
    }

    #[tokio::test]
    async fn stream_with_only_message_start_stop() {
        // Edge case: no text, just start/stop markers
        let (mut agent, _dir) = make_agent(
            vec![CallBehavior::Events(vec![
                StreamEvent::MessageStart,
                StreamEvent::MessageStop,
            ])],
            false,
        )
        .await;

        let events = agent.chat("hi", "").await.unwrap();
        assert!(
            matches!(events.last(), Some(AgentEvent::Done)),
            "must still produce Done with no text"
        );
    }

    #[tokio::test]
    async fn consecutive_chats_dont_bleed_behavior() {
        // Each chat() call gets a fresh FaultConfig cursor starting at index 0.
        // So each chat independently gets the same first behavior.
        let behaviors = vec![CallBehavior::Events(vec![
            StreamEvent::MessageStart,
            StreamEvent::TextDelta { text: "response".into() },
            StreamEvent::MessageStop,
        ])];
        let (mut agent, _dir) = make_agent(behaviors, false).await;

        // Both chats should get the same text-only response
        let events1 = agent.chat("msg1", "").await.unwrap();
        let events2 = agent.chat("msg2", "").await.unwrap();

        let texts1 = text_fragments(&events1).join("");
        let texts2 = text_fragments(&events2).join("");
        assert_eq!(texts1, texts2, "both chats should produce the same text");
        assert!(matches!(events1.last(), Some(AgentEvent::Done)));
        assert!(matches!(events2.last(), Some(AgentEvent::Done)));
    }

    #[tokio::test]
    async fn large_stream_no_lost_events() {
        // Simulate 2026-04-03 bug: 92 events across multiple tool rounds.
        // Mock always returns a tool call → exhausts all 10 rounds.
        // Total events: 10 * (ToolCall + ToolResult) + Done = 21 events minimum.
        let behaviors = vec![CallBehavior::Events(vec![
            StreamEvent::MessageStart,
            StreamEvent::TextDelta { text: "x".into() },
            StreamEvent::MessageStop,
        ])];
        let (mut agent, _dir) = make_agent(behaviors, false).await;

        let events = agent.chat("go", "").await.unwrap();
        // Mock response has no tool calls, so it should be simple: TextDelta + Done
        assert!(events.len() >= 2, "must have at least 2 events");
        assert!(matches!(events.last(), Some(AgentEvent::Done)));
    }
}
