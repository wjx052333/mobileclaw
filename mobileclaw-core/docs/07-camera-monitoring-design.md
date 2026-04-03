# Camera Monitoring Skill — Design Document

## 1. Overview

A camera monitoring capability that lets the agent:
- **One-shot capture**: grab N frames on demand and analyze them via the LLM
- **Continuous monitoring**: run a background loop that silently monitors camera frames and alerts the user only when anomalies are detected

### Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Frame acquisition | Flutter-side `camera` package → mmap shared memory → zero-copy Rust read | Performance-critical; avoids FFI call overhead per frame |
| Ring buffer | Fixed-slot mmap ring buffer, 16 frames × 64KB = 1MB | Lock-free read, Dart writes header atomically |
| Authorization | Dialog on first camera_capture call per session, then no repeat prompts | Conversation-level auth; revoked on ChatPage dispose |
| Preview UI | Independent CameraPreviewPage, can be opened/closed without affecting agent frame access | Preview and authorization are separate lifecycles |
| Images NOT stored in memory | Frames are ephemeral; only text summaries go to memory | Avoids bloating memory DB and token context |
| Model must support vision | `vision_supported()` computed at session creation from model metadata; unsupported models return clear error | Prevents silent failures |
| Guard prompt for monitoring | Single-token yes/no prompt, followed by detailed analysis only on "yes" | Cost-effective, reduces false positives |
| Alert frequency | No severity grading; every "yes" from guard triggers alert immediately. If continuous anomalies, alerts fire on every check (minus cooldown). | Simpler logic; user sees urgency from alert frequency itself |
| Monitor pauses during chat | `AtomicBool` flag set during `chat()` and cleared after | Prevents API rate limit conflicts |
| Monitor auto-cleanup on drop | `CancellationToken` stored in `AgentSession`; cancelled on `Drop` | No leaked tasks if ChatPage closes without explicit stop |
| Error messages in English | `ClawError` variants use English; Flutter localizes | Consistent with existing codebase patterns |
| Monitor has independent LLM client | Monitor builds its own client via `create_llm_client()` — independent instance, independent state, no context pollution | Monitor is an independent agent; must not share state with main loop |
| No tool name hardcoding | `Tool::produces_images() -> bool` trait method; `loop_impl` checks interface, not string name | Decouples tool identity from loop behavior |
| Single-turn LLM for monitor | `LlmClient::chat_text()` — non-streaming, returns `String` | Avoids SSE stream overhead for 1-token guard response |

## 2. Architecture

### 2.1 mmap Zero-Copy Frame Buffer

```
Flutter (Dart)                          Rust
    │                                     │
    │  dart:ffi → mmap(1MB)               │
    │  allocates 16 slots of 64KB each    │
    │                                     │
    │  header layout per slot:            │
    │  [id: u64, ts: u64, size: u32,      │
    │   w: u16, h: u16] = 20B             │
    │  [jpeg data ... up to ~64KB]         │
    │                                     │
    │  camera encodes 360p JPEG           │
    │  writes jpeg directly into slot     │
    │  updates header.size atomically     │
    │                                     │
    ├─────────────────────────────────────►│
    │  mmap write (no FFI call per frame)  │
    │                                     │
    │                           CameraCapture tool
    │                           reads slot headers
    │                           sees size > 0 → valid frame
    │                           copies jpeg data (only at read time)
    │                           constructs ContentBlock::Image
```

**Why mmap:**
- Dart writes JPEG bytes directly into Rust-allocated memory — no FFI function call per frame
- Rust reads mmap region — no lock, no cross-language boundary
- 16 slots = 1MB pre-allocated; no runtime allocation for push
- Header `size` field acts as a seqlock: Rust checks `size > 0` to know frame is valid

**Slot layout (fixed, no alignment issues):**
```rust
#[repr(C)]
pub struct FrameHeader {
    pub id: u64,         // 8 bytes — monotonically increasing
    pub timestamp_ms: u64, // 8 bytes — unix millis
    pub size: u32,        // 4 bytes — 0 = empty, >0 = jpeg byte count
    pub width: u16,       // 2 bytes
    pub height: u16,      // 2 bytes
}  // total: 24 bytes (padded to 32 for alignment)

const SLOT_SIZE: usize = 64 * 1024; // 64KB per slot
const HEADER_SIZE: usize = 32;
const MAX_FRAME_SIZE: usize = SLOT_SIZE - HEADER_SIZE; // ~64KB minus header
```

### 2.2 Component Diagram

```
Flutter UI (CameraPreviewPage)
    │
    ├─ camera package → ImageStream → JPEG encode (360p downsample)
    ├─ 2fps write JPEG directly into mmap ring buffer slot
    │  (atomic header update: size = jpeg_len)
    │
    ▼
┌─────────────────────────────────────────────────────────┐
│  Rust Core (AgentSession)                               │
│                                                         │
│  MmapRingBuffer (1MB, mmap-backed, lock-free read)      │
│    └── push via Dart mmap write (zero FFI calls)        │
│    └── read_latest_n() copies valid frames from mmap    │
│                                                         │
│  camera_capture Tool                                    │
│  ├── read_latest_n(N) from mmap buffer                  │
│  ├── loop_impl constructs ContentBlock::Image           │
│  │   (via Tool::produces_images() — no string matching) │
│  └── LLM analyzes (multi-image)                         │
│                                                         │
│  Background Monitor Task (independent LLM client)       │
│  ├── built via create_llm_client() — independent state  │
│  ├── chat_text() for guard prompt (non-streaming)       │
│  ├── guard prompt (yes/no, 1 token)                     │
│  ├── on "yes": analysis prompt → alert via mpsc         │
│  ├── pauses during active chat()                        │
│  └── alerts via FRB Stream<CameraAlert>                 │
└─────────────────────────────────────────────────────────┘
    │
    ▼  (alerts stream)
Flutter ChatPage: appends alert to chat
```

## 3. Interface Definitions

### 3.1 mmap Ring Buffer (`mobileclaw-core/src/tools/builtin/camera.rs`)

```rust
/// Fixed-size header for each frame slot. Must be repr(C) for cross-FFI.
#[repr(C)]
pub struct FrameHeader {
    pub id: u64,
    pub timestamp_ms: u64,
    pub size: u32,    // 0 = empty slot, >0 = valid frame
    pub width: u16,
    pub height: u16,
}

/// mmap-backed ring buffer. Dart writes frames directly into the mmap region.
/// Rust reads frames by scanning headers.
pub struct MmapRingBuffer {
    mmap: Mmap,           // memmap2 crate, 1MB pre-allocated
    capacity: usize,      // number of slots (default 16)
    write_index: Mutex<usize>,  // next slot to write (monotonic counter for Dart)
}

const SLOT_SIZE: usize = 64 * 1024; // 64KB per slot
const FRAME_HEADER_SIZE: usize = 32; // padded

impl MmapRingBuffer {
    /// Allocate mmap with `capacity` slots.
    pub fn new(capacity: usize) -> Result<Self, Box<dyn Error>>;

    /// Read latest N valid frames. Copies jpeg data from mmap into Vec<FrameData>.
    /// Lock-free: only reads mmap, never writes.
    pub fn read_latest_n(&self, n: usize) -> Vec<FrameData>;

    pub fn is_empty(&self) -> bool;

    /// Get the mmap address and capacity as (ptr, len) for Dart to map.
    pub fn mmap_info(&self) -> (u64, usize);  // (address, total_length)

    /// Get the write_index slot pointer for Dart to write into.
    /// Dart advances write_index modulo capacity after each write.
    pub fn get_write_slot(&self) -> (u64, usize);  // (ptr_to_slot, slot_size)
}

/// Rust-side frame data (copied from mmap for LLM use).
#[derive(Debug, Clone)]
pub struct FrameData {
    pub id: u64,
    pub timestamp_ms: u64,
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub type CameraFrameBuffer = MmapRingBuffer;
```

**FFI exposure — Dart gets mmap address:**

```rust
// In ffi.rs
/// Get the mmap address and total size for the camera ring buffer.
/// Dart uses this to map the memory region and write frames directly.
/// Returns (address, total_bytes) or (0, 0) if not available.
pub fn camera_get_mmap_info(session_ptr: *const AgentSession) -> (u64, usize);

/// Get the current write slot address for Dart to write into.
/// Dart writes jpeg into this slot, then calls camera_advance_write_index().
pub fn camera_get_write_slot(session_ptr: *const AgentSession) -> (u64, usize);

/// Advance the write index after Dart finishes writing a frame.
pub fn camera_advance_write_index(session_ptr: *const AgentSession);
```

These use `*const AgentSession` raw pointers, NOT `i64`. Dart gets these once at session creation via an FFI call that returns `(mmap_addr, mmap_size)`, then uses them for zero-copy writes.

### 3.2 Camera Parameters (via AgentConfig extension)

```rust
pub struct AgentConfig {
    // ... existing fields ...
    pub camera_frames_per_capture: Option<u32>,  // default 5
    pub camera_max_frames_per_capture: Option<u32>,  // hard limit, default 16
    pub camera_ring_buffer_capacity: Option<usize>,  // default 16
}
```

### 3.3 Tool::produces_images() — No Hardcoding

Add to the `Tool` trait (`mobileclaw-core/src/tools/traits.rs`):

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult>;
    fn required_permissions(&self) -> Vec<Permission> { vec![] }
    fn timeout_ms(&self) -> u64 { 10_000 }

    /// Returns true if this tool produces image data that should be
    /// appended to the message history as ContentBlock::Image blocks.
    /// The default implementation returns false.
    fn produces_images(&self) -> bool { false }
}
```

`CameraCapture` overrides this:

```rust
impl Tool for CameraCapture {
    // ... other methods ...
    fn produces_images(&self) -> bool { true }
}
```

### 3.4 ContentBlock::Image Extension

```rust
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Image { mime_type: String, data: Vec<u8> },
}
```

### 3.5 camera_capture Tool

```rust
pub struct CameraCapture;

impl Tool for CameraCapture {
    fn name(&self) -> &str { "camera_capture" }
    fn description(&self) -> &str {
        "Capture N recent camera frames for visual analysis. \
         Returns metadata about captured frames."
    }
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
    fn produces_images(&self) -> bool { true }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult> {
        // 1. Check vision_supported
        // 2. Read latest N frames from MmapRingBuffer
        // 3. If empty → Err(ClawError::CameraUnauthorized)
        // 4. Return metadata JSON (images handled by loop_impl via produces_images())
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

Error messages in English. Flutter localizes.

### 3.7 AgentEventDto Extension

```rust
pub enum AgentEventDto {
    // ... existing variants ...
    CameraAuthRequired,
    CameraAlert {
        summary: String,
        frame_id: u64,
    },
}
```

`CameraAlert` does NOT include `jpeg: Vec<u8>` — summary text suffices for chat UI.

### 3.8 LlmClient::chat_text() — Single-Turn Non-Streaming

Add to the `LlmClient` trait:

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn stream_messages(
        &self,
        system: &str,
        messages: &[Message],
        max_tokens: u32,
        tools: &[ToolSpec],
    ) -> ClawResult<EventStream>;

    fn native_tool_support(&self) -> bool { false }
    fn vision_supported(&self) -> bool { false }

    /// Single-turn, non-streaming chat. Returns the complete response text.
    /// Used by the background monitor for guard prompts (max_tokens=1)
    /// and analysis prompts (max_tokens=150).
    /// Supports multi-modal messages (ContentBlock::Image).
    async fn chat_text(
        &self,
        system: &str,
        messages: &[Message],
        max_tokens: u32,
    ) -> ClawResult<String>;
}
```

**Default implementation** (works for any backend):
Call `stream_messages()` and collect `TextDelta` events into a String.
This is correct but not optimal for 1-token guard prompts.

**ClaudeClient override**: Use the non-streaming Messages API endpoint
(`POST /v1/messages` without `stream: true`). Parse `content[0].text` from
the JSON response. Faster and simpler for short responses.

**OpenAiCompatClient override**: Use the non-streaming `/chat/completions`
endpoint.

### 3.9 FFI Methods on AgentSession

```rust
impl AgentSession {
    pub async fn camera_start_monitor(
        &mut self,
        scenario: String,
        frames_per_check: u32,
        check_interval_ms: u32,
    ) -> anyhow::Result<String>;

    pub fn camera_stop_monitor(&mut self, monitor_id: &str) -> bool;
    pub fn camera_set_authorized(&mut self, authorized: bool);
}
```

`AgentSession` needs additional fields:

```rust
pub struct AgentSession {
    inner: AgentLoop<...>,
    memory: Arc<SqliteMemory>,
    secrets: Arc<SqliteSecretStore>,
    session_dir: Option<PathBuf>,
    session_id: String,
    camera_buffer: Arc<CameraFrameBuffer>,
    camera_authorized: Arc<AtomicBool>,       // shared with ToolContext
    monitor_tokens: Vec<(String, CancellationToken)>,
    camera_alert_rx: mpsc::Receiver<CameraAlert>,  // for FRB stream
}
```

`camera_authorized` is `Arc<AtomicBool>` shared between AgentSession and
ToolContext so the camera_capture tool can check authorization without
reaching back through FFI boundaries.

### 3.10 FRB Stream for Camera Alerts

```rust
impl AgentSession {
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

## 4. Authorization Flow

```
1. LLM calls camera_capture → tool reads RingBuffer
2. Buffer empty → returns Err(CameraUnauthorized)
3. loop_impl catches error → produces ToolResult with error message
4. FFI chat() converts events to DTOs; additionally checks if ANY
   tool result contains CameraUnauthorized → emits CameraAuthRequired
5. Dart shows Dialog: "Agent wants to access the camera. Allow?"
6a. User confirms → Dart calls camera_set_authorized(true)
    → Dart writes frames into mmap ring buffer at 2fps
    → Dart sends a follow-up chat message: "摄像头已就绪，请继续"
    → LLM calls camera_capture again → this time gets frames
6b. User declines → Dart calls camera_set_authorized(false)
    → LLM already received the error message in step 3's tool_result
7. Revocation: on ChatPage dispose, Dart calls camera_set_authorized(false)
   → Dart stops writing frames → buffer drains
```

No retry mechanism. User authorizes → sends new chat message → new camera_capture succeeds.

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
No cooldown — every "yes" fires an alert. If continuous anomalies, the user sees a rapid stream of alerts.

### 5.2 Monitor Task Lifecycle

```
camera_start_monitor() called
  → create independent LLM client via create_llm_client()
  → create CancellationToken
  → spawn tokio task:
      loop {
        tokio::select! {
          _ = token.cancelled() => break,
          _ = sleep(interval) => {
            if chat_in_progress.load(Ordering::Relaxed) { continue; }
            frames = ring_buffer.read_latest_n(frames_per_check);
            if frames.is_empty() { continue; }
            guard_result = llm.chat_text(guard_system, [images], 1).await;
            if parse_yes(&guard_result) {
              analysis = llm.chat_text(analysis_system, [images], 150).await;
              alert_tx.send(CameraAlert { summary: analysis, ... }).await;
              // Write alert to memory DB for future recall
              memory.store(
                &format!("camera_monitor/{}/{}", session_id, timestamp_hex),
                &analysis,
                MemoryCategory::Conversation,
              ).await;
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

**Independent LLM client**: Built fresh via `create_llm_client()` using the same provider config and API key as the main session. This ensures:
- Independent `reqwest::Client` instance
- No shared connection pool or request state
- No context pollution between monitor and main chat

**Concurrency with chat()**: Monitor sets `chat_in_progress` AtomicBool check before each LLM call.

### 5.3 Vision Support Detection

`vision_supported` computed from model metadata at session creation:
- **Anthropic Claude**: `true` for all models with `claude-` prefix
- **OpenAI-compatible**: `true` for `gpt-4o*`, `gpt-4-turbo*`; `false` otherwise
- **Ollama**: `false` by default

### 5.4 Token Cost Estimate

Per check (5 frames × 360p at ~50KB each):
- Per image: (50000 * 3) / 750 + 85 = 285 tokens
- 5 images: 5 × 285 = 1425 tokens
- Text prompt (guard): ~50 tokens
- Role + block overhead: 3 + 5 = 8 tokens
- **Guard total**: ~1483 tokens
- **Analysis total** (on "yes"): ~1425 + 150 (response) + ~100 (text) + 8 = ~1683 tokens

At 5s interval, guard-only: ~10,678 tokens/hour (~11.8M/month 24/7).
Image tokens count as input tokens for pricing (~$0.15/M for Claude Sonnet).

## 6. Memory — What Changes and What Doesn't

### No Changes:
- **SqliteMemory**: never stores images
- **Phase C summary**: unchanged — camera tool results are text-only
- **Memory DB schema**: unchanged

### Changes:
- **`ContentBlock::Image`** enum variant
- **`Message::text_content()`**: skip Image blocks
- **Token estimator**: handle Image blocks
- **Monitor alert storage**: monitor writes alerts to memory DB for future recall (path: `camera_monitor/{session_id}/{timestamp_hex}`)

### History Behavior:
- camera_capture's tool_result in history is text-only
- Image blocks exist only in current `Vec<Message>`, pruned normally
- Monitor alerts written to memory DB separately from conversation history

## 7. LLM Client Changes

### 7.1 Multi-Image Content in API Calls

**Critical**: The in-memory `ContentBlock::Image { mime_type, data: Vec<u8> }` does NOT
serialize to the format expected by LLM APIs. Serde produces `{"type": "image", "data": [255, 216, ...]}`
(array of bytes), but APIs expect base64-encoded strings.

**Anthropic native**: Must build request body manually, converting each ContentBlock::Image:
```json
{"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "<base64>"}}
```
Text blocks: `{"type": "text", "text": "..."}`.
ToolUse/ToolResult: handled separately in the `tool_use`/`tool_result` content block types.

**OpenAI-compatible**: Convert to:
```json
{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,..."}}
```

**XML path (Ollama)**: base64 embedded in text prompt with a marker like
`[IMAGE: base64data]` since Ollama's text-only models can't process images natively.
For vision-capable Ollama models (llava, etc.), the native API path may work.

Both ClaudeClient.stream_messages() and ClaudeClient.chat_text() need this conversion.
Create a helper `fn build_claude_messages(messages: &[Message]) -> Vec<serde_json::Value>`.

### 7.2 chat_text() Implementation

**ClaudeClient**: Use the non-streaming Messages API endpoint (no `stream: true` flag). Parse `content[0].text` from the JSON response.

**OpenAiCompatClient**: Use the non-streaming `/chat/completions` endpoint.

**Default impl**: Collect `TextDelta` events from `stream_messages()` — works for any backend.

### 7.3 Unsupported Model Handling

When `camera_capture.execute()` detects `vision_supported == false`:
```
Err(ClawError::CameraModelNotSupported(
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
| `CameraMonitorFailed` | background task LLM call failed | Internal — task retries or stops |

## 9. File Changes

### New Files
- `mobileclaw-core/src/tools/builtin/camera.rs` — `CameraCapture` tool + `MmapRingBuffer` + `FrameHeader` + `FrameData`
- `mobileclaw-flutter/.../camera_service.dart` — CameraController + mmap frame writing

### Modified Files
- `mobileclaw-core/src/error.rs` — add 4 camera `ClawError` variants
- `mobileclaw-core/src/llm/types.rs` — add `ContentBlock::Image`
- `mobileclaw-core/src/llm/client.rs` — add `vision_supported()` + `chat_text()` to `LlmClient` trait; add image-to-ClAude conversion helper
- `mobileclaw-core/src/llm/claude_client.rs` — impl `vision_supported()` + `chat_text()` + image base64 serialization
- `mobileclaw-core/src/llm/openai_compat.rs` — impl `vision_supported()` + `chat_text()` + image base64 serialization
- `mobileclaw-core/src/llm/ollama.rs` — impl `vision_supported()` (false) + `chat_text()` (default)
- `mobileclaw-core/src/agent/loop_impl.rs` — use `Tool::produces_images()` instead of string matching; construct Image blocks; `chat_in_progress` AtomicBool
- `mobileclaw-core/src/agent/token_counter.rs` — Image token calculation
- `mobileclaw-core/src/ffi.rs` — `AgentConfig` camera fields, mmap FFI functions, camera methods, `CameraAlert` struct, FRB stream, `camera_authorized` AtomicBool
- `mobileclaw-core/src/tools/traits.rs` — `ToolContext` gets `camera_frame_buffer` + `camera_authorized` + `vision_supported`; `Tool` trait gets `produces_images()`
- `mobileclaw-core/src/tools/builtin/mod.rs` — register `camera_capture`
- `mobileclaw-core/Cargo.toml` — add `base64` crate dependency for image encoding
- `mobileclaw-flutter/.../agent_impl.dart` — SDK camera service integration
- `mobileclaw-flutter/.../chat_page.dart` — CameraAuthRequired event handling, CameraAlert display

## 10. Risks and Open Questions

### 10.1 mmap Race Condition

Dart writes JPEG + header into mmap slot; Rust reads the same memory concurrently.
Without proper synchronization, Rust could read `size > 0` before Dart finishes
writing the JPEG data, producing a corrupted frame.

**Mitigation**: Use `AtomicU32` for the `size` field with release/acquire semantics.
Dart writes JPEG data first, then sets `size` with `Ordering::release`. Rust reads
`size` with `Ordering::acquire` — if > 0, the JPEG data is guaranteed visible.

Alternative: A separate atomic "generation counter" that Dart increments after
completing each write. Rust reads the counter before and after copying the frame;
if they match, the copy was consistent.

### 10.2 Dual FFI Strategy for mmap

flutter_rust_bridge v2 opaque handles don't expose raw pointers. The mmap address
`(u64, usize)` must be returned via a separate `extern "C"` FFI function, not via
FRB-generated bindings.

**Implication**: Two FFI layers coexist:
- FRB: `AgentSession` methods (chat, memory, camera_start_monitor, etc.)
- Raw C FFI: `mobileclaw_camera_mmap_info()` for mmap address access

Dart must call both layers. This is acceptable but adds complexity.

### 10.3 64KB Frame Size Limit

At 360p (640×360), JPEG size depends on quality setting:
- Quality 70: ~15-20KB
- Quality 85: ~25-35KB
- Quality 95: ~40-60KB (approaching the 64KB limit)

**Mitigation**: Flutter side caps quality at 85 for 360p. If frame exceeds
`MAX_FRAME_SIZE`, downscale further or drop the frame.

### 10.4 Image Block Pruning Behavior

When context pruning fires, `ContentBlock::Image` blocks in pruned messages
are lost permanently. The text_content() of those messages is also lost.
This is correct behavior — the LLM has already "seen" the images. However,
the token cost of images is high (~285 tokens per 360p frame), so pruning
should account for this.

### 10.5 Monitor Memory DB Writes

Monitor writes alerts to memory DB at `camera_monitor/{session_id}/{timestamp_hex}`.
This path prefix must be excluded from normal session history recall to avoid
polluting the "previously in this session" summary with monitor alerts.

**Mitigation**: `build_history_prefix()` in ffi.rs uses `history/{session_id}/`
prefix, which doesn't overlap with `camera_monitor/`.

### 10.6 Token Cost

At 5s interval with 5 frames × 360p (~50KB each):
- Guard: ~1483 tokens per check (see §5.4 for breakdown)
- Per hour: ~10,678 tokens
- Monthly (24/7): ~7.7M tokens → ~$1.15/month at Claude Sonnet pricing
  ($0.15/M input tokens, image tokens count as input)

This is acceptable for the value provided, but should be communicated to the user.
