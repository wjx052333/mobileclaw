# Camera Monitoring Skill — Design Document

## 1. Overview

A camera monitoring capability that lets the agent:
- **One-shot capture**: grab N frames on demand and analyze them via the LLM
- **Continuous monitoring**: run a background loop that silently monitors camera frames and alerts the user only when anomalies are detected

### Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Frame acquisition | Flutter-side via `camera` package, pushes to Rust via FFI `camera_push_frame()` | FRB v2 cannot share `Arc<Mutex<...>>` with Dart; FFI functions are the bridge |
| Ring buffer | Internal Rust struct, private to `AgentSession` | Dart never touches the buffer directly — only pushes frames and reads via FFI |
| Authorization | Dialog on first camera_capture call per session, then no repeat prompts | Conversation-level auth; revoked on ChatPage dispose |
| Preview UI | Independent CameraPreviewPage, can be opened/closed without affecting agent frame access | Preview and authorization are separate lifecycles |
| Images NOT stored in memory | Frames are ephemeral; only text summaries go to memory | Avoids bloating memory DB and token context |
| Model must support vision | `vision_supported()` computed at session creation from model metadata; unsupported models return clear error | Prevents silent failures |
| Guard prompt for monitoring | Single-token yes/no prompt, followed by detailed analysis only on "yes" | Cost-effective, reduces false positives |
| Monitor pauses during chat | `AtomicBool` flag set during `chat()` and cleared after | Prevents API rate limit conflicts |
| Monitor auto-cleanup on drop | `CancellationToken` stored in `AgentSession`; cancelled on `Drop` | No leaked tasks if ChatPage closes without explicit stop |
| Error messages in English | `ClawError` variants use English; Flutter localizes | Consistent with existing codebase patterns |

## 2. Architecture

```
Flutter UI (CameraPreviewPage)
    │
    ├─ camera package → ImageStream → JPEG encode (360p downsample)
    ├─ 2fps call FFI camera_push_frame(jpeg, id, ts, w, h)
    │
    ▼
┌─────────────────────────────────────────────┐
│  Rust Core (AgentSession)                   │
│                                             │
│  RingBuffer (private, internal)             │
│    └── Mutex<VecDeque<FrameData>>           │
│    ├── push() via camera_push_frame() FFI   │
│    └── read_latest_n() by tools/monitor     │
│                                             │
│  camera_capture Tool                        │
│  ├── read_latest_n(N) from buffer           │
│  ├── construct ContentBlock::Image          │
│  ├── append to message history              │
│  └── LLM analyzes (multi-image)             │
│                                             │
│  Background Monitor Task                    │
│  ├── independent tokio task                 │
│  ├── guard prompt (yes/no, 1 token)         │
│  ├── on "yes": analysis prompt              │
│  ├── pauses during active chat()            │
│  └── alerts via FRB Stream<CameraAlert>     │
└─────────────────────────────────────────────┘
    │
    ▼  (alerts stream)
Flutter ChatPage: appends alert to chat
```

## 3. Interface Definitions

### 3.1 Ring Buffer (`mobileclaw-core/src/tools/builtin/camera.rs`)

Internal Rust-only struct. Not exposed via FFI.

```rust
pub struct FrameData {
    pub id: u64,
    pub timestamp_ms: u64,
    pub jpeg: Vec<u8>,   // 360p downsampled, ~30-50KB per frame
    pub width: u32,
    pub height: u32,
}

pub struct RingBuffer {
    buffer: Mutex<VecDeque<FrameData>>,
    capacity: usize,  // default 16
}

impl RingBuffer {
    pub fn push(&self, frame: FrameData);
    pub fn read_latest_n(&self, n: usize) -> Vec<FrameData>;
    pub fn is_empty(&self) -> bool;
    pub fn latest_timestamp_ms(&self) -> Option<u64>;
}
```

**Concurrency note**: `push()` allocates `FrameData` (including JPEG `Vec<u8>`) *before* acquiring the mutex. The lock is held only for `VecDeque::push_back` and `pop_front`, which are O(1) pointer operations. `read_latest_n()` clones the `VecDeque` under the lock, then releases. For the expected rate (2fps push, 0.2fps read) contention is negligible.

### 3.2 FFI Functions for Frame Push (free functions, not methods)

```rust
// In ffi.rs
/// Push a camera frame from Flutter into the ring buffer.
/// Returns false if no session has been initialized with a camera buffer.
pub fn camera_push_frame(
    session_handle: i64,  // opaque session pointer handle
    jpeg: Vec<u8>,
    frame_id: u64,
    timestamp_ms: u64,
    width: u32,
    height: u32,
) -> bool;
```

The `session_handle` is a numeric handle (pointer cast to i64) that Dart gets from `AgentSession.create()`. This allows Dart to push frames without holding a reference to the Rust `Arc<RingBuffer>`.

### 3.3 Camera Parameters (via AgentConfig extension)

```rust
pub struct AgentConfig {
    // ... existing fields ...
    pub camera_frames_per_capture: Option<u32>,  // default 5
    pub camera_max_frames_per_capture: Option<u32>,  // hard limit, default 16
    pub camera_ring_buffer_capacity: Option<usize>,  // default 16
    // Note: camera_frame_rate is NOT in AgentConfig — Flutter controls push rate locally
}
```

### 3.4 ContentBlock::Image Extension

```rust
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Image { mime_type: String, data: Vec<u8> }, // NEW
}
```

### 3.5 camera_capture Tool

```rust
pub struct CameraCapture;

impl Tool for CameraCapture {
    fn name(&self) -> &str { "camera_capture" }
    fn description(&self) -> &str { "Capture N recent camera frames for visual analysis." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "frames": {
                    "type": "integer",
                    "description": "Number of recent frames to capture (default 5, max 16)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult> {
        // 1. Check vision_supported (from ToolContext)
        // 2. Read latest N frames from RingBuffer (via ToolContext)
        // 3. If empty → Err(ClawError::CameraUnauthorized)
        // 4. Return success with frame metadata JSON
        //    (Image blocks are constructed by the CALLER in loop_impl, not by this tool)
    }
}
```

### 3.6 ClawError Variants (NEW)

```rust
pub enum ClawError {
    // ... existing variants ...
    #[error("camera unauthorized: user denied access")]
    CameraUnauthorized,
    #[error("camera model not supported: {0}")]
    CameraModelNotSupported(String),
    #[error("camera frame timeout: no new frame for {0}s")]
    CameraFrameTimeout(u64),
    #[error("camera capture failed: {0}")]
    CameraCaptureFailed(String),
}
```

Error messages are in English. Flutter localizes when displaying to the user.

### 3.7 AgentEventDto Extension

```rust
pub enum AgentEventDto {
    // ... existing variants ...
    CameraAuthRequired,  // NEW: triggers Flutter dialog
    CameraAlert {        // NEW: from background monitor, delivered via FRB Stream
        summary: String, // one-line description of anomaly
        frame_id: u64,
    },
}
```

**Note**: `CameraAlert` does NOT include `jpeg: Vec<u8>` to avoid bandwidth overhead. The summary text is sufficient for the chat UI. If the user wants to see the actual frame, they can open the CameraPreviewPage.

### 3.8 FFI Methods on AgentSession

```rust
impl AgentSession {
    /// Start the background camera monitor task.
    /// Returns a monitor_id that can be used to stop it.
    pub async fn camera_start_monitor(
        &mut self,
        scenario: String,          // e.g., "baby in crib"
        frames_per_check: u32,     // how many frames per guard check
        check_interval_ms: u32,    // interval between checks
        cooldown_after_alert_ms: u32,
    ) -> anyhow::Result<String>;

    /// Stop a running background monitor.
    pub fn camera_stop_monitor(&mut self, monitor_id: &str) -> bool;

    /// Set authorization state. Called by Flutter after user responds to dialog.
    pub fn camera_set_authorized(&mut self, authorized: bool);
}
```

### 3.9 FRB Stream for Camera Alerts

Use FRB's `#[frb(stream)]` attribute to expose an async stream from `AgentSession`:

```rust
impl AgentSession {
    /// Stream of camera monitoring alerts from the background task.
    #[frb(stream)]
    pub async fn camera_alert_stream(&self) -> impl Stream<Item = CameraAlert> {
        self.camera_alert_rx.clone()
    }
}

#[frb]
pub struct CameraAlert {
    pub summary: String,
    pub frame_id: u64,
    pub timestamp_ms: u64,
}
```

The background monitor task sends alerts through an `mpsc::Sender<CameraAlert>`; the stream reads from the corresponding `Receiver<CameraAlert>`.

## 4. Authorization Flow

```
1. LLM calls camera_capture → tool reads RingBuffer
2. Buffer empty → returns Err(CameraUnauthorized)
3. loop_impl catches error → produces ToolResult with error message
4. FFI chat() converts events to DTOs; additionally checks if ANY
   tool result contains CameraUnauthorized → emits CameraAuthRequired
5. Dart shows Dialog: "Agent wants to access the camera. Allow?"
6a. User confirms → Dart calls camera_set_authorized(true)
    → Dart starts camera stream → calls camera_push_frame() FFI at 2fps
    → Dart sends a follow-up chat message: "摄像头已就绪，请继续"
    → LLM calls camera_capture again → this time gets frames
6b. User declines → Dart calls camera_set_authorized(false)
    → LLM already received the error message in step 3's tool_result
7. Revocation: on ChatPage dispose, Dart calls camera_set_authorized(false)
   → Dart stops calling camera_push_frame() → buffer drains
```

**No retry mechanism needed.** The authorization flow is:
1. First camera_capture fails with error → LLM sees error → tells user to authorize
2. User authorizes → sends new chat message → new camera_capture succeeds

This is consistent with how other tool errors are handled (the LLM sees the error and adapts).

## 5. Background Monitor

### 5.1 Two-Stage Detection

**Stage 1 — Guard Prompt** (every `check_interval_ms`, default 5s):
```
System: "You are monitoring: {scenario}. These are {N} consecutive frames.
         Is there anything the user should be alerted about?
         Answer only: Yes or No."
Images: [frame_1, frame_2, ..., frame_N]
Max tokens: 1
```

Response normalization: `.trim().to_lowercase().starts_with("yes")`.

If response == "No" → silent, next check.
If response == "Yes" → Stage 2.

**Stage 2 — Analysis Prompt**:
```
System: "Analyze what happened in these frames. One sentence, under 50 words."
Images: [frame_1, frame_2, ..., frame_N]
Max tokens: 150
```

Result → send through `mpsc::Sender<CameraAlert>` → FRB stream → Dart appends to chat.
Then cooldown for `cooldown_after_alert_ms` (default 30s).

### 5.2 Monitor Task Lifecycle

```
camera_start_monitor() called
  → create CancellationToken
  → spawn tokio task:
      loop {
        tokio::select! {
          _ = token.cancelled() => break,
          _ = sleep(interval) => {
            if chat_in_progress.load(Ordering::Relaxed) { continue; }
            frames = ring_buffer.read_latest_n(frames_per_check);
            if frames.is_empty() { continue; }
            guard_result = llm.guard_prompt(frames);
            if parse_yes(&guard_result) {
              analysis = llm.analysis_prompt(frames);
              alert_tx.send(CameraAlert { summary: analysis, ... }).await;
              sleep(cooldown).await;
            }
          }
        }
      }
  → return monitor_id

camera_stop_monitor(monitor_id)
  → cancel token

AgentSession::Drop
  → cancel all active monitor tokens
```

**Concurrency with chat()**: The monitor sets `chat_in_progress` AtomicBool to true at the start of `chat()` and false after it completes. The monitor skips checks while this flag is set, preventing concurrent API calls and rate limit issues.

### 5.3 Vision Support Detection

`vision_supported` is computed from model metadata at session creation — no network probe needed:
- **Anthropic Claude**: `true` for all models with `claude-` prefix (all Claude 3.x+ support vision)
- **OpenAI-compatible**: `true` for model names matching `gpt-4o*`, `gpt-4-turbo*`; `false` otherwise
- **Ollama**: `false` by default (requires explicit user override; Ollama vision support varies by model)

This value is stored in `AgentLoop.vision_supported: bool` and checked by `camera_capture.execute()`.

### 5.4 Token Cost Estimate

Per check (5 frames × 360p, 640×360 = 230,400 pixels):
- Vision token formula: `pixels / 750 + 85` per image
- Per frame: 230400 / 750 + 85 ≈ **392 tokens**
- Guard: 5 × 392 + ~30 (prompt) + 1 (response) ≈ **1991 tokens**
- Analysis (rare, only on "yes"): 5 × 392 + ~30 + 150 ≈ **2140 tokens**

At 5s interval, guard-only: ~14,335 tokens/hour. With ~1 alert/minute: +128,400 tokens/hour worst case.
Typical: ~15,000-20,000 tokens/hour for normal monitoring.

## 6. Memory — What Changes and What Doesn't

### No Changes Needed:
- **SqliteMemory**: never stores images. `store()`, `recall()`, `get()`, `forget()` operate on text content only.
- **Phase C summary**: `build_interaction_text()` already extracts only text/tool events. Camera tool calls produce text tool_results (e.g., "captured 5 frames"), no image data.
- **Memory DB schema**: unchanged.

### Changes Required:
- **`ContentBlock::Image`** enum variant: new type in `llm/types.rs`
- **`Message::text_content()`**: skip Image blocks (return empty string for that block)
- **Token estimator**: `estimate_tokens()` must handle Image blocks using the vision formula: `pixels / 750 + 85`
- **`build_interaction_text()`**: no changes needed — already only processes `AgentEvent`s

### History Behavior:
- camera_capture's tool_result in history is **text-only** (e.g., `{"frames_captured": 5, "resolution": "640x360"}`)
- The actual Image blocks in message history exist only in the current `Vec<Message>` and are pruned when context pruning fires
- Images are never persisted to memory DB

## 7. LLM Client Changes

### 7.1 Multi-Image Content in API Calls

For Anthropic native API: multiple `ContentBlock::Image` in a single message is supported natively. The `stream_messages` implementation constructs `content` array with `{type: "image", source: {type: "base64", media_type: "image/jpeg", data: "<base64>"}}`.

For OpenAI-compatible: convert to `{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,..."}}`.

For XML path (Ollama): fall back to base64 embedded in text prompt (similar to current tool description pattern).

### 7.2 Unsupported Model Handling

When `camera_capture.execute()` detects `vision_supported == false`:
```
Return Err(ClawError::CameraModelNotSupported(
    "The current model does not support image analysis. \
     Please switch to a vision-capable model (Claude 3.x, GPT-4o, etc.).".into()
))
```

## 8. Error Handling

| Error | Trigger | LLM Tool Result (English) |
|-------|---------|---------------------------|
| `CameraUnauthorized` | RingBuffer empty | "Camera access denied. The user has not authorized camera access. Ask the user to enable camera access." |
| `CameraModelNotSupported` | vision_supported == false | "The current model does not support image analysis. Please switch to a vision-capable model." |
| `CameraFrameTimeout` | buffer empty for > 5s after auth | "Camera authorized but no frames received for 5 seconds. The camera stream may be paused." |
| `CameraCaptureFailed` | ring buffer read error | "Camera capture failed. Please try again." |
| `CameraMonitorFailed` | background task LLM call failed | Internal — task retries or stops, no message to LLM |

## 9. File Changes

### New Files
- `mobileclaw-core/src/tools/builtin/camera.rs` — `CameraCapture` tool + `RingBuffer` + `FrameData`
- `mobileclaw-flutter/.../camera_service.dart` — CameraController + frame push to FFI

### Modified Files
- `mobileclaw-core/src/error.rs` — add 4 camera `ClawError` variants
- `mobileclaw-core/src/llm/types.rs` — add `ContentBlock::Image`
- `mobileclaw-core/src/llm/client.rs` — add `vision_supported(&self) -> bool` to `LlmClient` trait with default impl
- `mobileclaw-core/src/llm/claude_client.rs` — impl `vision_supported()` returning `true`
- `mobileclaw-core/src/llm/openai_client.rs` — impl `vision_supported()` from model name whitelist
- `mobileclaw-core/src/llm/ollama_client.rs` — impl `vision_supported()` returning `false`
- `mobileclaw-core/src/agent/loop_impl.rs` — `AgentLoop` gets `vision_supported: bool`; construct Image blocks in tool result handling; `chat_in_progress` AtomicBool
- `mobileclaw-core/src/agent/token_counter.rs` — Image token calculation
- `mobileclaw-core/src/ffi.rs` — `AgentConfig` camera fields, `AgentEventDto::CameraAuthRequired`, `camera_push_frame()`, `camera_start_monitor`/`camera_stop_monitor`/`camera_set_authorized`, `CameraAlert` struct, FRB stream
- `mobileclaw-core/src/tools/traits.rs` — `ToolContext` gets `camera_frame_buffer: Option<Arc<RingBuffer>>` and `vision_supported: bool`
- `mobileclaw-core/src/tools/builtin/mod.rs` — register `camera_capture`
- `mobileclaw-core/src/agent/session.rs` — `Drop` impl for `AgentSession` cancels monitor tasks
- `mobileclaw-flutter/.../agent_impl.dart` — SDK camera service integration
- `mobileclaw-flutter/.../chat_page.dart` — CameraAuthRequired event handling, CameraAlert display
