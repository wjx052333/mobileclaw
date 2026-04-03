# Camera Integration Test Design

**File:** `mobileclaw-core/tests/integration_camera.rs`  
**Feature flag:** `--features test-utils`  
**Based on:** `docs/07-camera-monitoring-design.md`, `docs/superpowers/plans/2026-04-03-camera-monitoring.md`

---

## 1. Scope

These integration tests exercise the camera feature end-to-end through the Rust layer only. They sit below the Flutter/Dart FFI boundary and above unit tests.

**In scope:**
- `CameraCapture` tool executing inside `AgentLoop`
- `CameraAuthRequired` event propagation through the full event pipeline
- FFI API surface on `AgentSession` (camera_push_frame, camera_set_authorized, camera_is_authorized, camera_get_mmap_info)
- Background monitor stub lifecycle (camera_start_monitor, camera_stop_monitor, camera_alert_stream)
- Frame data integrity: frame IDs, resolution metadata, `ContentBlock::Image` blocks in history

**Out of scope (deferred):**
- Flutter→Rust 1000-frame throughput benchmark — requires real Flutter + mmap (Phase 2). Tracked separately.
- mmap zero-copy path — Phase 2, not yet implemented.
- Real LLM API calls — all tests use `MockLlmClient`.
- CameraAlert stream with real background monitor — Phase 2.

---

## 2. Key Invariants Under Test

These properties are derived from the design doc and must hold at all times:

| # | Invariant | Source |
|---|-----------|--------|
| I1 | `camera_capture` is registered as a builtin tool | design §9 |
| I2 | Unauthorized capture emits `CameraAuthRequired` before the turn ends | design §4 |
| I3 | Authorized capture with frames → `ContentBlock::Image` blocks appended to history | design §2.2 |
| I4 | `produces_images() == true` for CameraCapture; `false` for all other builtins | design §3.3 |
| I5 | Vision-unsupported model → `CameraModelNotSupported` error, no image blocks | design §7.3 |
| I6 | Authorized but empty buffer → `CameraFrameTimeout` error | design §3.6 |
| I7 | `camera_push_frame` auto-sets `camera_authorized = true` | design §3.9 |
| I8 | `camera_set_authorized(false)` revokes capture access mid-session | design §4 |
| I9 | `camera_start_monitor` returns a non-empty ID string (Phase 1 stub) | design §5.2 |
| I10 | `camera_stop_monitor` Phase 1 stub unconditionally returns `false` regardless of input | design §5.2 |
| I11 | `camera_alert_stream()` returns empty Vec in Phase 1 | design §3.10 |
| I12 | `read_latest_n(N)` returns the N most-recent frames, clamped to buffer occupancy | design §3.1 |

---

## 3. Test Scenarios

### Group A — Tool Registration

**A1 `camera_capture_registered_as_builtin`** *(already exists — keep)*

- Register all builtins via `register_all_builtins`.
- Assert `registry.get("camera_capture")` is `Some`.
- Assert `produces_images()` returns `true` for the retrieved tool.

> Note: A1 is an integration test that validates the full `register_all_builtins` path. The `camera_capture_not_in_protected_names_conflict` scenario is a unit test for `ToolRegistry` and belongs in `src/tools/registry.rs #[cfg(test)]`, not here.

---

### Group B — Authorization Flow

**B1 `unauthorized_capture_emits_auth_required_event`**

The most critical end-to-end path for authorization:
```
AgentLoop (no buffer set / camera_authorized=false)
  → MockLlmClient emits camera_capture tool_use
  → CameraCapture.execute() → Err(CameraUnauthorized)
  → loop_impl special-cases this error
  → emits AgentEvent::CameraAuthRequired
  → ToolResult with is_error=true
```

Setup:
- `ToolContext` with `camera_frame_buffer = Some(empty_buf)`, `camera_authorized = false`
- `MockLlmClient` emitting one `camera_capture` tool use

Assert:
- `events` contains `AgentEventDto::CameraAuthRequired`
- `events` contains `AgentEventDto::ToolResult { name: "camera_capture", success: false }`

**B2 `authorized_capture_succeeds_after_push_frame`**

Sequence:
1. Start with `camera_authorized = false`, empty buffer.
2. Call `camera_push_frame` → buffer gets one frame, authorized auto-set to `true`.
3. Run agent loop with `camera_capture` mock.
4. Assert success: ToolResult with `success = true`, Image block in history.

**B3 `camera_set_authorized_false_revokes_mid_session`**

Sequence:
1. Push one frame, camera authorized.
2. Run one successful `camera_capture` turn.
3. Call `camera_set_authorized(false)`.
4. Run another `camera_capture` turn.
5. Assert second turn produces `CameraAuthRequired` event.

---

### Group C — Frame Data Integrity

**C1 `single_frame_jpeg_appears_in_history`**

- Push one frame with known JPEG bytes `[0xFF, 0xD8, 0xFF, 0xE0]`.
- Run `camera_capture` with `frames=1`.
- Walk `agent.history()` to find the User-role message containing ToolResult + Image.
- Assert `ContentBlock::Image { data: [0xFF, 0xD8, 0xFF, 0xE0], mime_type: "image/jpeg" }` is present.

**C2 `multi_frame_capture_appends_n_image_blocks`**

- Push 5 frames with sequential IDs (1..=5) and distinct JPEG bytes.
- Run `camera_capture` with `frames=5`.
- Assert history contains 5 `ContentBlock::Image` blocks in the tool-result User message.

**C3 `capture_clamps_to_available_frames`**

- Push 2 frames into a buffer of capacity 16.
- Run `camera_capture` with `frames=10` (requesting more than available).
- Assert only 2 Image blocks in history (not 10, not panic).

**C4 `capture_uses_latest_frames_when_buffer_full`**

- Push 20 frames into a buffer of capacity 16 (frames 1..=20).
- Run `camera_capture` with `frames=3`.
- Assert Image blocks correspond to frames 18, 19, 20 (most recent 3).

**C5 `frame_resolution_metadata_in_tool_result`**

- Push one frame with `width=640, height=360`.
- Run `camera_capture`.
- Assert ToolResult JSON contains `"resolution": "640x360"`.

---

### Group D — Error Paths

**D1 `vision_not_supported_returns_model_error`**

- Set `vision_supported = false` in ToolContext.
- Push one frame, set authorized.
- Run `camera_capture`.
- Assert ToolResult `success=false`.
- Assert ToolResult content contains `"model"` (distinguishes `CameraModelNotSupported` from other errors).
- Assert no `ContentBlock::Image` blocks in history.
- Assert exactly zero `CameraAuthRequired` events (this is NOT an auth error; asserting the count guards against accidental reclassification).

**D2 `authorized_but_empty_buffer_returns_frame_timeout`**

- Set `camera_authorized = true` explicitly (via `Arc<AtomicBool::new(true)>`), leave buffer empty.
- Run `camera_capture`.
- Assert ToolResult `success=false`.
- Assert ToolResult content contains `"timeout"` (distinguishes `CameraFrameTimeout` from auth errors).
- Assert exactly zero `CameraAuthRequired` events.

**D3 `camera_capture_not_found_tool_result_error`**

- Create a registry without calling `register_all_builtins` (empty registry).
- MockLlmClient emits a `camera_capture` tool use.
- Assert ToolResult `success=false` with "tool not found" message.

---

### Group E — Async Notifications

**E1 `camera_auth_required_appears_exactly_once_per_unauthorized_turn`**

- Single turn with one `camera_capture` tool use, unauthorized.
- Count `CameraAuthRequired` events in the returned `Vec<AgentEventDto>`.
- Assert count == 1 (not 0, not 2+).

**E2 `camera_auth_required_not_emitted_when_authorized`**

- Camera authorized, frame pushed.
- Run `camera_capture`.
- Assert no `CameraAuthRequired` in events.

**E3 `camera_alert_stream_returns_empty_in_phase1`**

- On a freshly created `AgentSession` (via `AgentSession::create` with test config):
- Call `camera_alert_stream()`.
- Assert returned `Vec<CameraAlert>` is empty.

**E4 `camera_start_monitor_returns_non_empty_id`**

- Call `camera_start_monitor("test scenario", 3, 5000)`.
- Assert returned `Ok(id)` where `!id.is_empty()`.
- Note: Phase 1 stub returns `"monitor-id-todo"`. Test is forward-compatible with Phase 2 UUIDs.

**E5 `camera_stop_monitor_always_returns_false_in_phase1`**

- Call `camera_stop_monitor("any-id")`.
- Assert returns `false`.
- Note: Phase 1 stub unconditionally returns `false` regardless of input. This test documents the Phase 1 contract; Phase 2 will change this behavior for known IDs.

---

### Group F — FFI API Contracts

All tests in this group operate on `AgentSession` directly (via `AgentSession::create` with an in-memory test config), not through `AgentLoop`.

**F1 `camera_push_frame_auto_authorizes`**

- Create session. Assert `camera_is_authorized() == false`.
- Build a raw pointer to the session and call `camera_push_frame(ptr, jpeg, 1, 1000, 640, 360)`.
- Assert `camera_is_authorized() == true`.
- Assert `camera_buffer.is_empty() == false`.

> **Note on raw pointer safety:** `camera_push_frame` takes `i64` session_ptr and casts to `*const AgentSession`. In tests, we pass `session as *const _ as i64`. This is safe because the test holds the session in scope for the duration of the call.

**F2 `camera_set_authorized_toggles_flag`**

- Create session. Assert `camera_is_authorized() == false`.
- Call `camera_set_authorized(true)`. Assert `camera_is_authorized() == true`.
- Call `camera_set_authorized(false)`. Assert `camera_is_authorized() == false`.

**F3 `camera_get_mmap_info_returns_valid_tuple`**

- Create session, call `camera_get_mmap_info()`. Returns `(usize, usize, u64)`.
- Assert first element == 0 (Phase 1 hardcoded occupancy; will change in Phase 2).
- Assert second element (capacity) == 16 (matches `camera_ring_buffer_capacity`).
- Assert third element (latest_timestamp_ms) == 0 when buffer empty.
- Push one frame with `timestamp_ms=1234`. Assert third element == 1234.

**F4 `camera_push_frame_null_pointer_returns_false`**

- Call `camera_push_frame(0, vec![], 0, 0, 0, 0)`.
- Assert returns `false`.
- Assert no panic.

**F5 `camera_push_frame_multiple_frames_increments_buffer`**

- Create session, get pointer.
- Push 5 frames with sequential IDs.
- Assert `camera_is_authorized() == true`.
- Call `camera_get_mmap_info()` and verify latest_timestamp_ms equals the last pushed frame's timestamp.

---

### Group G — Agent Loop Integration

**G1 `camera_capture_full_agent_loop`** *(already exists — keep and extend)*

- Existing test: push 1 frame → MockLlmClient triggers camera_capture → verify ToolCall event + Image in history.
- Extend: also verify `ToolResult { success: true }` in events.

**G2 `camera_capture_no_buffer_returns_error`** *(already exists — keep)*

- No changes needed.

**G3 `camera_capture_tool_registered_as_builtin`** *(already exists — keep)*

- No changes needed.

**G4 `vision_false_loop_emits_error_tool_result`**

- ToolContext with `vision_supported=false`, frame pushed, authorized.
- MockLlmClient triggers camera_capture.
- Assert events contains `ToolResult { name: "camera_capture", success: false }`.
- Assert history contains no `ContentBlock::Image`.

**G5 `two_turn_auth_recovery_sequence`**

The primary real-world usage path for the authorization feature:

1. Turn 1: `camera_authorized=false`, empty buffer → `camera_capture` triggered → assert `CameraAuthRequired` emitted.
2. Between turns: call `camera_set_authorized(true)`, push one frame (simulates user granting permission + Dart starting camera service).
3. Turn 2: `camera_capture` triggered → assert `success=true`, Image block in history, no `CameraAuthRequired`.

**G6 `multiple_turns_camera_auth_revoke_and_restore`**

- Turn 1: camera authorized, frame pushed → success.
- Between turns: `camera_set_authorized(false)` (simulates ChatPage dispose).
- Turn 2: camera_capture triggered → `CameraAuthRequired` event.
- Between turns: push new frame (auto-authorizes via `camera_authorized=true`).
- Turn 3: camera_capture triggered → success, Image in history.

---

## 4. Test Infrastructure

### AgentSession Test Helper

Group F/E tests require `AgentSession::create`. The `TempDir` must be held alive in the **test body** (not inside the helper), because dropping it would delete the database files while `AgentSession` still holds paths to them.

```rust
// In each test body:
let dir = TempDir::new().unwrap();
let session = make_test_session(&dir).await;

// Helper:
async fn make_test_session(dir: &TempDir) -> AgentSession {
    AgentSession::create(AgentConfig {
        api_key: None,
        model: None,
        db_path: dir.path().join("mem.db").to_str().unwrap().into(),
        secrets_db_path: dir.path().join("sec.db").to_str().unwrap().into(),
        encryption_key: vec![0u8; 32],
        sandbox_dir: dir.path().to_str().unwrap().into(),
        http_allowlist: vec![],
        skills_dir: None,
        log_dir: None,
        session_dir: None,
        context_window: None,
        max_session_messages: None,
        camera_frames_per_capture: None,
        camera_max_frames_per_capture: None,
        camera_ring_buffer_capacity: Some(16u32),  // field type is Option<u32>
    }).await.unwrap()
}
```

> Note: `AgentSession::create` with no provider still allocates the camera buffer and sets up all camera state. Group F tests do not need the LLM client.

### AgentLoop Test Helper

Group B/C/D/E/G tests that drive through `AgentLoop` use an updated `make_ctx` helper that accepts an explicit `authorized: bool` parameter (separate from whether a buffer is provided), and a small factory for MockLlmClient:

```rust
async fn make_ctx(dir: &TempDir, buf: Option<Arc<CameraFrameBuffer>>, authorized: bool) -> ToolContext {
    let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
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

fn camera_mock(tool_use_id: &str, frames: u64) -> MockLlmClient {
    MockLlmClient::new_native(
        "Camera analysis complete.",
        vec![(
            tool_use_id.to_string(),
            "camera_capture".to_string(),
            serde_json::json!({"frames": frames}),
        )],
    )
}
```

> **Critical:** The old `make_ctx` helper set `authorized = buf.is_some()`, which conflated buffer presence with authorization. The new signature requires explicit `authorized` so that Group B tests can set `Some(empty_buf)` with `authorized=false` (the B1 scenario).

---

## 5. What Is NOT Tested Here

| What | Why | Where |
|------|-----|-------|
| Flutter→Rust 1000-frame FFI throughput | Requires real Flutter + mmap (Phase 2) | Flutter integration test suite |
| mmap slot layout / FrameHeader repr(C) | Phase 2 implementation not yet written | Phase 2 integration tests |
| Real LLM vision API round-trip | External network dependency | Manual / E2E tests |
| CameraAlert from live background monitor | Phase 2 (monitor task not implemented) | Phase 2 integration tests |
| Base64 image serialization in API request body | Unit-tested in `llm/client.rs` tests | `src/llm/client.rs #[cfg(test)]` |
| Token estimation for Image blocks | Unit-tested in `token_counter.rs` tests | `src/agent/token_counter.rs #[cfg(test)]` |
