# Core Black-Box Test Design

**Crate:** `tests/integration/` (`mobileclaw-integration`)  
**Run:** `cargo test -p mobileclaw-integration`  
**Feature flag:** `--features test-utils` (applied via `Cargo.toml` dependency on `mobileclaw-core`)

---

## 1. Purpose and Positioning

`tests/integration/` is the **black-box test layer for `mobileclaw-core`**. It imports
`mobileclaw-core` as an external dependency (not as a path-relative `#[cfg(test)]` module)
and exercises only the public API. Nothing internal — no `pub(crate)` types, no test
helpers that live inside `mobileclaw-core` — is accessible here.

### How it differs from `mobileclaw-core/tests/`

| Dimension | `mobileclaw-core/tests/` (white-box) | `tests/integration/` (black-box) |
|---|---|---|
| Dependency type | Crate-internal test binary, can access `pub(crate)` | External crate, public API only |
| LLM mock | `MockLlmClient` — always emits all configured tool_uses every round | `FaultInjectingLlmClient` — per-call configurable, pops next behavior each round |
| Focus | Feature correctness (camera auth flow, frame data, FFI contracts) | Event pipeline integrity, DTO boundary, fault tolerance |
| Multi-round control | Indirect (must create new `AgentLoop` per "turn") | Direct: each behavior slot controls one `stream_messages()` call |
| Origin of tests | Fine-grained feature scenarios | Cross-layer contract and regression tests |

**The name `tests/integration` is a misnomer.** This crate is better described as the
core's black-box test layer. "Integration" correctly implies that it tests the integration
between the Rust core and its consumer boundary — but it does NOT exercise a real Flutter
runtime, a real LLM API, or a real device. It is Rust-only. See §5 for the distinction
from true Flutter integration tests.

---

## 2. Key Infrastructure: `FaultInjectingLlmClient`

The central test tool in this crate is `FaultInjectingLlmClient`. It differs fundamentally
from `MockLlmClient`:

```
MockLlmClient:            call 1      call 2      call 3      ...
                          [fixed]     [fixed]     [fixed]     repeats forever

FaultInjectingLlmClient:  call 1      call 2      call 3      call 4+
                          behavior[0] behavior[1] behavior[2] behavior[last] (repeated)
```

`CallBehavior` variants:
- `Events(Vec<StreamEvent>)` — returns a well-formed SSE stream
- `Error(String)` — returns `Err(ClawError::Llm(...))` immediately

`FaultConfig::next()` increments a call index and clamps to `behaviors.len()-1`, so the
last behavior is repeated when exhausted.

This design enables:
- **Multi-round tests**: configure round 0 to trigger a tool call, round 1 to return text
- **LLM error injection**: configure round N to error, verify propagation
- **Round exhaustion**: single repeating behavior exhausts `MAX_TOOL_ROUNDS=10`

**Supported stream paths:**
- XML path (`native: false`): tool calls embedded in `TextDelta` as `<tool_call>...</tool_call>`
- Native path (`native: true`): tool calls via `StreamEvent::ToolUse { id, name, input }`

---

## 3. Test Modules

### `happy_path` — Baseline correctness

Verifies the normal flow (no errors, no faults) before any fault tests:

| Test | What it checks |
|---|---|
| `text_only_produces_text_delta_then_done` | Text response ends with Done |
| `multiple_text_deltas_concatenate` | Multiple TextDelta events all arrive |
| `xml_tool_call_executes_tool_and_emits_events` | XML tool call path: ToolCall + ToolResult events emitted |
| `native_tool_call_executes_tool_and_emits_events` | Native `StreamEvent::ToolUse` path: same guarantees |

---

### `fault_injection` — LLM and tool failures

Tests that faults in the LLM stream or tool execution are handled correctly:

| Test | What it checks |
|---|---|
| `llm_error_on_first_call_propagates` | `Err` on round 0 propagates to `chat()` caller |
| `llm_error_mid_tool_round` | `Err` on round 1 (after tool execution) propagates |
| `tool_round_exhaustion_still_emits_done` | Repeating tool call exhausts 10 rounds; Done still fires |
| `multi_tool_resolution_xml_path` | Round 0 tool + round 1 text: both TextDelta and ToolResult in events |
| `multi_tool_resolution_native_path` | Same as above on native path |
| `native_tool_error_propagates` | Tool execution failure (not LLM error) propagates correctly |

---

### `event_ordering` — Stream ordering guarantees

Tests invariants that the Flutter consumer depends on:

| Test | Invariant |
|---|---|
| `event_order_text_before_tool_before_done` | TextDelta < ToolCall < ToolResult < Done ordering |
| `done_is_always_last_event` | Done is the last event regardless of tool usage |
| `multi_chat_history_accumulates` | History grows across consecutive `chat()` calls |
| `context_stats_emitted_before_done_when_pruning` | ContextStats precedes Done when context window prunes |

---

### `dto_conversion` — `AgentEvent → AgentEventDto` boundary

Simulates the conversion performed by `AgentSession::chat()` inside `ffi.rs`. The Flutter
layer receives `Vec<AgentEventDto>`, not `Vec<AgentEvent>`. These tests verify the
conversion is lossless.

| Test | What it checks |
|---|---|
| `chat_events_convert_to_dto` | Live agent events convert to DTOs correctly |
| `event_list_round_trip_all_variants` | Every `AgentEvent` variant (including `CameraAuthRequired`) maps to the correct DTO variant with all fields preserved |
| `large_event_list_survives_conversion` | 91-event list (regression for 2026-04-03 Flutter bug) |

**`event_list_round_trip_all_variants` is the canonical variant-coverage test.** It must
be updated whenever a new `AgentEvent` variant is added to ensure the DTO conversion
handles it. Current variants covered:

```
TextDelta, ToolCall, ToolResult, CameraAuthRequired, ContextStats, Done
```

---

### `stream_resilience` — Edge cases in stream shape

| Test | What it checks |
|---|---|
| `empty_text_response_still_produces_done` | Empty TextDelta still ends with Done |
| `stream_with_only_message_start_stop` | No text at all — Done still fires |
| `consecutive_chats_dont_bleed_behavior` | Two `chat()` calls get independent behaviors |
| `large_stream_no_lost_events` | Regression for 2026-04-03 Flutter bug: large event list reaches consumer |

---

### `camera_pipeline` — Camera events at the FFI/DTO boundary

Black-box camera tests that are only possible with `FaultInjectingLlmClient`'s per-call
control. Complements `mobileclaw-core/tests/integration_camera.rs` which tests feature
correctness. These tests focus on what the Flutter consumer actually observes.

#### Key invariants under test

| # | Invariant |
|---|---|
| CP1 | `AgentEventDto::CameraAuthRequired` reaches the consumer when capture is unauthorized |
| CP2 | `CameraAuthRequired` DTO appears before `Done` DTO |
| CP3 | No `CameraAuthRequired` DTO when capture succeeds |
| CP4 | A LLM error arriving on the round *after* a camera auth failure propagates as `Err` |
| CP5 | 10 rounds of persistent camera auth failure: `Done` still fires, all `ToolResult` are `success=false` |
| CP6 | `CameraAuthRequired` in a mixed event list survives DTO conversion with correct count and position |

#### Tests

| Test | Invariant(s) | Unique aspect |
|---|---|---|
| `unauthorized_capture_emits_camera_auth_required_dto` | CP1, CP3 | Full end-to-end: Rust event → DTO |
| `camera_auth_required_before_done_in_dto_stream` | CP2 | Ordering in the converted DTO stream |
| `authorized_capture_has_no_camera_auth_required_dto` | CP3 | Negative assertion: no false CameraAuthRequired |
| `llm_error_after_camera_auth_failure_propagates` | CP4 | Only possible with `FaultInjectingLlmClient` (round-specific error) |
| `camera_auth_failure_round_exhaustion_still_emits_done` | CP5 | 10-round persistence, Done guarantee |
| `camera_auth_required_survives_mixed_event_list_conversion` | CP6 | Static DTO conversion, positional accuracy |

#### Helper: `make_camera_agent`

```rust
async fn make_camera_agent(
    behaviors: Vec<CallBehavior>,
    native: bool,
    authorized: bool,     // set independently of num_frames
    num_frames: usize,    // dummy JPEG frames pushed into the buffer
) -> (AgentLoop<FaultInjectingLlmClient>, Arc<CameraFrameBuffer>, Arc<AtomicBool>, TempDir)
```

Returns the `buf` and `auth` handles so tests can mutate camera state between turns if
needed (multi-turn revoke/restore tests).

---

## 4. Shared Helpers

```rust
// Top-level — accessible from all modules via `use super::*`

fn make_agent(behaviors, native) → (AgentLoop, TempDir)
    // Camera fields: None, authorized=false, vision=false

fn make_camera_agent(behaviors, native, authorized, num_frames)
    → (AgentLoop, Arc<CameraFrameBuffer>, Arc<AtomicBool>, TempDir)
    // Camera fields pre-configured; vision=true

fn event_to_dto(event: AgentEvent) → AgentEventDto
    // Mirrors ffi.rs AgentSession::chat() conversion; shared by dto_conversion and camera_pipeline

fn text_fragments(events: &[AgentEvent]) → Vec<&str>
    // Extract TextDelta text for assertion helpers
```

---

## 5. What This Crate Is NOT

This crate tests Rust-only behavior. It does not:

| What | Why not here |
|---|---|
| Real Flutter → Rust FFI calls | Requires Flutter runtime + generated Dart bindings |
| Camera frame injection from Dart | Dart calls `camera_push_frame` via flutter_rust_bridge; needs device/emulator |
| Real LLM API | External network dependency |
| UI event handling | Flutter widget tests or integration tests |
| `AgentSession` FFI API (Group F tests) | Tested in `mobileclaw-core/tests/integration_camera.rs` Group F |

True Flutter integration tests (Dart → Rust → event stream → Dart) are a separate
effort. See §6.

---

## 6. Flutter Integration Test Strategy (Future Work)

When this project reaches the Flutter integration test phase, the recommended approach is:

### Problem

The Rust `ClaudeClient` has `https://api.anthropic.com/v1/messages` hardcoded. There is
no way to substitute a mock LLM from Flutter without either:
(a) making the LLM URL configurable in `AgentConfig`, or  
(b) building a test `.so` with `--features test-utils` (which exposes `MockLlmClient`
    via FFI — not currently done).

### Recommended Approach

**Option A — Configurable LLM base URL (lowest coupling)**

Add `llm_base_url: Option<String>` to `AgentConfig`. In Flutter integration tests, pass
a local mock server URL (e.g., `http://localhost:9000`). The mock server speaks the
Anthropic SSE protocol. This keeps `MockLlmClient` out of the production binary and is
the most faithful test of the real code path.

**Option B — FFI-exposed `MockLlmClient` via feature flag**

Add a `ffi_test_helpers` feature that exposes `AgentSession::set_mock_llm(behaviors)`
from Rust to Dart. The `.so` for Flutter integration tests is built with this feature.
Less faithful to production but simpler infrastructure.

### What Flutter integration tests would cover

Once the LLM is mockable:

- Dart calls `AgentSession.create(config)` → FFI allocates `AgentSession`
- Dart calls `camera_push_frame(sessionPtr, jpegBytes, ...)` → frame pushed, authorized
- Dart calls `AgentSession.chat("check camera", "")` → events returned as `List<AgentEventDto>`
- Dart asserts `events.contains(AgentEventDto.cameraAuthRequired)` or `toolResult(success=true)`
- Multi-turn: push frame between turns, verify auth recovery sequence

These tests would run with `flutter drive` or `flutter test integration_test/` on an
Android emulator, exercising the real `flutter_rust_bridge` bindings end-to-end.
