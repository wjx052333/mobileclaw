//! Integration test: camera capture through the full agent loop.
//! Requires --features test-utils.

use mobileclaw_core::agent::loop_impl::AgentLoop;
use mobileclaw_core::llm::client::test_helpers::MockLlmClient;
use mobileclaw_core::memory::sqlite::SqliteMemory;
use mobileclaw_core::secrets::store::test_helpers::NullSecretStore;
use mobileclaw_core::skill::SkillManager;
use mobileclaw_core::tools::{
    PermissionChecker, ToolContext, ToolRegistry,
    builtin::{register_all_builtins, camera::{CameraFrameBuffer, FrameData}},
};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tempfile::TempDir;

async fn make_ctx(dir: &TempDir, buf: Option<Arc<CameraFrameBuffer>>) -> ToolContext {
    let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
    let authorized = buf.is_some();
    ToolContext {
        memory: mem,
        sandbox_dir: dir.path().to_path_buf(),
        http_allowlist: vec![],
        permissions: Arc::new(PermissionChecker::allow_all()),
        secrets: Arc::new(NullSecretStore),
        camera_frame_buffer: buf,
        camera_authorized: Arc::new(AtomicBool::new(authorized)),
        vision_supported: true,
    }
}

#[tokio::test]
async fn camera_capture_full_agent_loop() {
    let dir = TempDir::new().unwrap();

    let buf = Arc::new(CameraFrameBuffer::new(16));
    buf.push(FrameData {
        id: 1,
        timestamp_ms: 1000,
        jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0],
        width: 640,
        height: 360,
    });

    let mut registry = ToolRegistry::new();
    register_all_builtins(&mut registry);

    let ctx = make_ctx(&dir, Some(buf)).await;

    // Mock that triggers camera_capture then responds with text.
    let llm = MockLlmClient::new_native(
        "I can see the camera feed.",
        vec![("tu_1".to_string(), "camera_capture".to_string(), serde_json::json!({"frames": 1}))],
    );

    let mut agent = AgentLoop::new(llm, registry, ctx, SkillManager::new(vec![]));
    let events = agent.chat("check the camera", "").await.unwrap();

    // Should have ToolCall and ToolResult for camera_capture.
    let tool_calls: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                mobileclaw_core::agent::loop_impl::AgentEvent::ToolCall { name }
                    if name == "camera_capture"
            )
        })
        .collect();
    assert!(!tool_calls.is_empty(), "camera_capture tool should be called");

    // History should contain Image blocks in the tool result message.
    let has_image_in_history = agent.history().iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, mobileclaw_core::llm::types::ContentBlock::Image { .. }))
    });
    assert!(
        has_image_in_history,
        "history should contain Image blocks from camera_capture"
    );
}

#[tokio::test]
async fn camera_capture_no_buffer_returns_error() {
    let dir = TempDir::new().unwrap();
    let mut registry = ToolRegistry::new();
    register_all_builtins(&mut registry);

    let ctx = make_ctx(&dir, None).await;

    let llm = MockLlmClient::new_native(
        "No camera available.",
        vec![("tu_1".to_string(), "camera_capture".to_string(), serde_json::json!({}))],
    );

    let mut agent = AgentLoop::new(llm, registry, ctx, SkillManager::new(vec![]));
    let events = agent.chat("check the camera", "").await.unwrap();

    // Should have a ToolResult with success=false.
    let tool_results: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                mobileclaw_core::agent::loop_impl::AgentEvent::ToolResult { name, success }
                    if name == "camera_capture" && !success
            )
        })
        .collect();
    assert!(
        !tool_results.is_empty(),
        "camera_capture should return error when no buffer"
    );
}

#[tokio::test]
async fn camera_capture_tool_registered_as_builtin() {
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("camera_capture");
    assert!(tool.is_some(), "camera_capture should be registered as builtin");
}
