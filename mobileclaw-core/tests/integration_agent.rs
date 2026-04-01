// Run with: cargo test -p mobileclaw-core --features test-utils --test integration_agent
use mobileclaw_core::{
    agent::AgentLoop,
    llm::client::test_helpers::MockLlmClient,
    memory::sqlite::SqliteMemory,
    tools::{ToolContext, ToolRegistry, builtin::register_all_builtins, PermissionChecker},
    skill::SkillManager,
};
use std::sync::Arc;
use tempfile::TempDir;

async fn make_loop(llm_response: &str) -> (AgentLoop<MockLlmClient>, TempDir) {
    let dir = TempDir::new().unwrap();
    let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
    let mut registry = ToolRegistry::new();
    register_all_builtins(&mut registry);
    let ctx = ToolContext {
        memory: mem,
        sandbox_dir: dir.path().to_path_buf(),
        http_allowlist: vec![],
        permissions: Arc::new(PermissionChecker::allow_all()),
    };
    let llm = MockLlmClient { response: llm_response.to_string() };
    let agent = AgentLoop::new(llm, registry, ctx, SkillManager::new(vec![]));
    (agent, dir)
}

#[tokio::test]
async fn simple_conversation_returns_text() {
    let (mut agent, _dir) = make_loop("Hello, I'm Claude!").await;
    let events: Vec<_> = agent.chat("Hi there", "You are helpful.").await.unwrap();
    let text: String = events.iter().filter_map(|e| match e {
        mobileclaw_core::agent::AgentEvent::TextDelta { text } => Some(text.as_str()),
        _ => None,
    }).collect();
    assert!(text.contains("Claude"));
}

#[tokio::test]
async fn tool_call_in_response_is_executed() {
    let response = r#"I'll check the time.
<tool_call>{"name": "time", "args": {}}</tool_call>"#;
    let (mut agent, _dir) = make_loop(response).await;
    let events: Vec<_> = agent.chat("What time is it?", "You are helpful.").await.unwrap();
    let tool_events: Vec<_> = events.iter().filter(|e| matches!(e, mobileclaw_core::agent::AgentEvent::ToolCall { .. })).collect();
    assert!(!tool_events.is_empty(), "should have executed a tool call");
}

#[tokio::test]
async fn message_history_grows_with_turns() {
    let (mut agent, _dir) = make_loop("Reply 1").await;
    agent.chat("Turn 1", "").await.unwrap();
    agent.chat("Turn 2", "").await.unwrap();
    assert_eq!(agent.history().len(), 4); // user + assistant × 2
}
