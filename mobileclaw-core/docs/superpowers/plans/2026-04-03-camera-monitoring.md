# Camera Monitoring Skill — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add camera capture, multi-image LLM analysis, and background monitoring to the Rust agent core with FFI exposure to Flutter.

**Architecture:** Flutter pushes JPEG frames via FFI into a Rust ring buffer. The `camera_capture` tool reads frames and constructs `ContentBlock::Image` for LLM vision APIs. A background tokio task runs guard+analysis prompts for continuous monitoring, streaming alerts via FRB stream.

**Tech Stack:** Rust (mobileclaw-core), flutter_rust_bridge v2, Anthropic Claude Vision API, OpenAI-compatible vision

---

### Task 1: Add camera ClawError variants

**Files:**
- Modify: `mobileclaw-core/src/error.rs`
- Modify: `mobileclaw-core/src/error.rs` (tests at end of file)

- [ ] **Step 1: Add 4 new error variants to ClawError**

Append after the existing `Session` variant, before the `Sql` variant:

```rust
    #[error("camera unauthorized: user denied access")]
    CameraUnauthorized,

    #[error("camera model not supported: {0}")]
    CameraModelNotSupported(String),

    #[error("camera frame timeout: no new frame for {0}s")]
    CameraFrameTimeout(u64),

    #[error("camera capture failed: {0}")]
    CameraCaptureFailed(String),
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p mobileclaw-core --lib -- error::tests
```

Expected: all existing tests pass (no new tests yet — error variants derive Display via thiserror).

- [ ] **Step 3: Commit**

```bash
git add mobileclaw-core/src/error.rs
git commit -m "feat(error): add camera-related ClawError variants"
```

---

### Task 2: Add ContentBlock::Image

**Files:**
- Modify: `mobileclaw-core/src/llm/types.rs`
- Test: `mobileclaw-core/src/llm/types.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)]` module in `mobileclaw-core/src/llm/types.rs`:

```rust
    #[test]
    fn image_block_serializes_with_type_tag() {
        let block = ContentBlock::Image {
            mime_type: "image/jpeg".into(),
            data: vec![0xFF, 0xD8, 0xFF],
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "image");
        assert_eq!(json["mime_type"], "image/jpeg");
        assert_eq!(json["data"], serde_json::json!([255, 216, 255]));
    }

    #[test]
    fn text_content_skips_image_blocks() {
        let mut msg = Message::user("hello");
        msg.content.push(ContentBlock::Image {
            mime_type: "image/jpeg".into(),
            data: vec![1, 2, 3],
        });
        // text_content should only return "hello", not panic
        assert_eq!(msg.text_content(), "hello");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p mobileclaw-core --lib -- llm::types::tests::image_block_serializes
cargo test -p mobileclaw-core --lib -- llm::types::tests::text_content_skips
```

Expected: compile error — `ContentBlock::Image` doesn't exist.

- [ ] **Step 3: Add ContentBlock::Image variant**

In `mobileclaw-core/src/llm/types.rs`, add to the `ContentBlock` enum:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Image { mime_type: String, data: Vec<u8> },
}
```

- [ ] **Step 4: Update text_content() to handle Image blocks**

In `Message::text_content()`, add a match arm for `Image`:

```rust
    pub fn text_content(&self) -> String {
        self.content.iter().map(|b| match b {
            ContentBlock::Text { text } => text.as_str(),
            ContentBlock::ToolUse { .. } => "",
            ContentBlock::ToolResult { .. } => "",
            ContentBlock::Image { .. } => "",
        }).collect::<Vec<_>>().join("")
    }
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test -p mobileclaw-core --lib -- llm::types::tests
```

Expected: all type tests pass including the 2 new ones.

- [ ] **Step 6: Commit**

```bash
git add mobileclaw-core/src/llm/types.rs
git commit -m "feat(llm): add ContentBlock::Image for multi-modal support"
```

---

### Task 3: RingBuffer + FrameData

**Files:**
- Create: `mobileclaw-core/src/tools/builtin/camera.rs`
- Test: `mobileclaw-core/src/tools/builtin/camera.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing tests**

Create the file with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_read_single_frame() {
        let buf = RingBuffer::new(4);
        buf.push(FrameData {
            id: 1,
            timestamp_ms: 1000,
            jpeg: vec![1, 2, 3],
            width: 640,
            height: 360,
        });
        let frames = buf.read_latest_n(1);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].id, 1);
        assert_eq!(frames[0].jpeg, vec![1, 2, 3]);
    }

    #[test]
    fn read_latest_n_returns_most_recent() {
        let buf = RingBuffer::new(4);
        for i in 1..=6 {
            buf.push(FrameData {
                id: i,
                timestamp_ms: i * 500,
                jpeg: vec![i as u8],
                width: 640,
                height: 360,
            });
        }
        // Buffer capacity is 4, so frames 1-2 were evicted
        let frames = buf.read_latest_n(4);
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0].id, 3);
        assert_eq!(frames[3].id, 6);
    }

    #[test]
    fn read_latest_n_clamps_to_available() {
        let buf = RingBuffer::new(4);
        buf.push(FrameData { id: 1, timestamp_ms: 100, jpeg: vec![1], width: 1, height: 1 });
        let frames = buf.read_latest_n(10);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].id, 1);
    }

    #[test]
    fn is_empty_returns_true_for_new_buffer() {
        let buf = RingBuffer::<FrameData>::new(4);
        assert!(buf.is_empty());
    }

    #[test]
    fn is_empty_returns_false_after_push() {
        let buf = RingBuffer::new(4);
        buf.push(FrameData { id: 1, timestamp_ms: 100, jpeg: vec![1], width: 1, height: 1 });
        assert!(!buf.is_empty());
    }

    #[test]
    fn latest_timestamp_ms_returns_none_when_empty() {
        let buf = RingBuffer::<FrameData>::new(4);
        assert!(buf.latest_timestamp_ms().is_none());
    }

    #[test]
    fn latest_timestamp_ms_returns_most_recent() {
        let buf = RingBuffer::new(4);
        buf.push(FrameData { id: 1, timestamp_ms: 100, jpeg: vec![1], width: 1, height: 1 });
        buf.push(FrameData { id: 2, timestamp_ms: 500, jpeg: vec![2], width: 1, height: 1 });
        assert_eq!(buf.latest_timestamp_ms(), Some(500));
    }

    #[test]
    fn concurrent_push_read_no_panic() {
        use std::sync::Arc;
        let buf = Arc::new(RingBuffer::new(16));
        let mut handles = vec![];
        for i in 0..10 {
            let b = buf.clone();
            handles.push(std::thread::spawn(move || {
                for j in 0..100 {
                    b.push(FrameData {
                        id: i * 100 + j,
                        timestamp_ms: (i * 100 + j) as u64 * 10,
                        jpeg: vec![j as u8],
                        width: 1,
                        height: 1,
                    });
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // Should not panic; may have any number of frames up to capacity
        let frames = buf.read_latest_n(16);
        assert!(frames.len() <= 16);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p mobileclaw-core --lib -- tools::builtin::camera::tests
```

Expected: compile error — module not found.

- [ ] **Step 3: Write the implementation**

Add to the top of `camera.rs`:

```rust
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct FrameData {
    pub id: u64,
    pub timestamp_ms: u64,
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub struct RingBuffer<T> {
    buffer: Mutex<VecDeque<T>>,
    capacity: usize,
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    pub fn push(&self, item: T) {
        let mut buf = self.buffer.lock().expect("ring buffer poisoned");
        if buf.len() >= self.capacity {
            buf.pop_front();
        }
        buf.push_back(item);
    }

    pub fn read_latest_n(&self, n: usize) -> Vec<T>
    where
        T: Clone,
    {
        let buf = self.buffer.lock().expect("ring buffer poisoned");
        let len = buf.len();
        let start = len.saturating_sub(n);
        buf.iter().skip(start).cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        let buf = self.buffer.lock().expect("ring buffer poisoned");
        buf.is_empty()
    }
}

impl<T> RingBuffer<T>
where
    T: Clone,
{
    pub fn latest_timestamp_ms(&self) -> Option<u64>
    where
        T: HasTimestamp,
    {
        let buf = self.buffer.lock().expect("ring buffer poisoned");
        buf.back().map(|item| item.timestamp_ms())
    }
}

/// Trait for items that can report their timestamp.
/// Used by the ring buffer to query the latest timestamp.
pub trait HasTimestamp {
    fn timestamp_ms(&self) -> u64;
}

impl HasTimestamp for FrameData {
    fn timestamp_ms(&self) -> u64 {
        self.timestamp_ms
    }
}
```

Also need a type alias for convenience:

```rust
/// The camera frame ring buffer type used throughout the codebase.
pub type CameraFrameBuffer = RingBuffer<FrameData>;
```

- [ ] **Step 4: Add camera module to mod.rs**

In `mobileclaw-core/src/tools/builtin/mod.rs`, add:

```rust
pub mod camera;
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p mobileclaw-core --lib -- tools::builtin::camera::tests
```

Expected: all 8 tests pass.

- [ ] **Step 6: Commit**

```bash
git add mobileclaw-core/src/tools/builtin/camera.rs mobileclaw-core/src/tools/builtin/mod.rs
git commit -m "feat(tools): add RingBuffer and FrameData for camera frames"
```

---

### Task 4: vision_supported() on LlmClient trait

**Files:**
- Modify: `mobileclaw-core/src/llm/client.rs`
- Modify: `mobileclaw-core/src/llm/openai_compat.rs`
- Modify: `mobileclaw-core/src/llm/ollama.rs`
- Modify: `mobileclaw-core/src/llm/client.rs` (tests in the existing `mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to the existing `mod tests` in `mobileclaw-core/src/llm/client.rs`:

```rust
    #[test]
    fn claude_client_vision_supported() {
        let client = ClaudeClient::new("key", "claude-sonnet-4-6-20250514");
        assert!(client.vision_supported());
    }

    #[test]
    fn claude_client_non_claude_model_vision_not_supported() {
        let client = ClaudeClient::new("key", "some-other-model");
        assert!(!client.vision_supported());
    }
```

- [ ] **Step 2: Add vision_supported() to LlmClient trait**

In `mobileclaw-core/src/llm/client.rs`, add to the `LlmClient` trait:

```rust
    /// Returns true if this provider/model supports vision (image analysis).
    fn vision_supported(&self) -> bool {
        false
    }
```

- [ ] **Step 3: Implement for ClaudeClient**

In the `impl LlmClient for ClaudeClient` block, add:

```rust
    fn vision_supported(&self) -> bool {
        self.model.starts_with("claude-")
    }
```

- [ ] **Step 4: Implement for OpenAiCompatClient**

In `mobileclaw-core/src/llm/openai_compat.rs`, find the `impl LlmClient for OpenAiCompatClient` block and add:

```rust
    fn vision_supported(&self) -> bool {
        self.model.starts_with("gpt-4o") || self.model.starts_with("gpt-4-turbo")
    }
```

- [ ] **Step 5: Implement for OllamaClient**

In `mobileclaw-core/src/llm/ollama.rs`, find the `impl LlmClient for OllamaClient` block and add:

```rust
    fn vision_supported(&self) -> bool {
        false
    }
```

- [ ] **Step 6: Add vision_supported() to Arc<dyn LlmClient> impl**

In `mobileclaw-core/src/llm/client.rs`, add to the `impl LlmClient for std::sync::Arc<dyn LlmClient>` block:

```rust
    fn vision_supported(&self) -> bool {
        self.as_ref().vision_supported()
    }
```

- [ ] **Step 7: Run tests**

```bash
cargo test -p mobileclaw-core --lib -- llm::client::tests
```

Expected: all tests pass including the 2 new ones.

- [ ] **Step 8: Commit**

```bash
git add mobileclaw-core/src/llm/client.rs mobileclaw-core/src/llm/openai_compat.rs mobileclaw-core/src/llm/ollama.rs
git commit -m "feat(llm): add vision_supported() to LlmClient trait"
```

---

### Task 5: Image token estimation

**Files:**
- Modify: `mobileclaw-core/src/agent/token_counter.rs`
- Test: `mobileclaw-core/src/agent/token_counter.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing tests**

Add to the existing test module in `token_counter.rs`:

```rust
    #[test]
    fn image_block_token_estimation() {
        // Formula: (data.len() * 3) / 750 + 85 per image block
        // For 50,000 bytes: (50000*3)/750 + 85 = 200 + 85 = 285
        // + 3 (role) + 1 (block overhead) = 289
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Image {
                mime_type: "image/jpeg".into(),
                data: vec![0u8; 50_000],
            }],
        };
        let tokens = estimate_message_tokens(&msg);
        assert_eq!(tokens, 289);
    }

    #[test]
    fn mixed_text_and_image_tokens() {
        let mut msg = Message::user("look at this");
        msg.content.push(ContentBlock::Image {
            mime_type: "image/jpeg".into(),
            data: vec![0u8; 100_000],
        });
        let tokens = estimate_message_tokens(&msg);
        // text: 12 bytes → ceil(12/4) = 3
        // image: (100000*3)/750 + 85 = 400 + 85 = 485
        // overhead: 3 + 2 blocks = 5
        // total: 3 + 485 + 5 = 493
        assert_eq!(tokens, 493);
    }

    #[test]
    fn multiple_images_in_same_message() {
        let mut msg = Message { role: Role::User, content: vec![] };
        for _ in 0..5 {
            msg.content.push(ContentBlock::Image {
                mime_type: "image/jpeg".into(),
                data: vec![0u8; 50_000], // (50000*3)/750+85 = 285 per image
            });
        }
        let tokens = estimate_message_tokens(&msg);
        // 5 × 285 + 3 (role) + 5 (block overhead) = 1425 + 8 = 1433
        assert_eq!(tokens, 1433);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p mobileclaw-core --lib -- agent::token_counter::tests::image_block
```

Expected: compile error or wrong token counts.

- [ ] **Step 3: Update estimate_message_tokens()**

Replace the `text_bytes` calculation in `estimate_message_tokens()`:

```rust
pub fn estimate_message_tokens(msg: &Message) -> usize {
    let mut text_bytes: usize = 0;
    let mut image_tokens: usize = 0;

    for block in &msg.content {
        match block {
            ContentBlock::Text { text } => {
                text_bytes += text.len();
            }
            ContentBlock::ToolUse { .. } => {}
            ContentBlock::ToolResult { content, .. } => {
                text_bytes += content.len();
            }
            ContentBlock::Image { data, .. } => {
                // Vision token estimation from compressed JPEG byte size.
                // Empirical formula: (jpeg_bytes * 3) / 750 + 85
                // The *3 factor approximates raw pixel count from compressed size
                // (JPEG ~20-30% compression ratio for 360p).
                // This gives ~285 tokens for a 50KB frame, ~485 for 100KB.
                let pixels_estimate = data.len() * 3;
                image_tokens += pixels_estimate / 750 + 85;
            }
        }
    }

    let overhead = 3 /* role tag */ + msg.content.len() /* per-block overhead */;

    let text_token = if text_bytes == 0 { 0 } else { text_bytes.div_ceil(4) };

    overhead + text_token + image_tokens
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p mobileclaw-core --lib -- agent::token_counter::tests
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add mobileclaw-core/src/agent/token_counter.rs
git commit -m "feat(token): add image token estimation to token_counter"
```

---

### Task 6: CameraCapture tool + ToolContext extension

**Files:**
- Modify: `mobileclaw-core/src/tools/traits.rs`
- Modify: `mobileclaw-core/src/tools/builtin/camera.rs`
- Modify: `mobileclaw-core/src/tools/builtin/mod.rs`

- [ ] **Step 1: Write the failing tests**

Add to `camera.rs` test module:

```rust
    #[tokio::test]
    async fn camera_capture_tool_returns_frame_metadata() {
        let buf = Arc::new(CameraFrameBuffer::new(16));
        buf.push(FrameData {
            id: 1, timestamp_ms: 1000,
            jpeg: vec![1,2,3,4,5], width: 640, height: 360,
        });
        buf.push(FrameData {
            id: 2, timestamp_ms: 2000,
            jpeg: vec![6,7,8,9,10], width: 640, height: 360,
        });

        let tool = CameraCapture;
        let ctx = ToolContext {
            memory: Arc::new(crate::memory::sqlite::SqliteMemory::open(":memory:").await.unwrap()),
            sandbox_dir: std::env::temp_dir(),
            http_allowlist: vec![],
            permissions: Arc::new(crate::tools::PermissionChecker::allow_all()),
            secrets: Arc::new(crate::secrets::store::test_helpers::NullSecretStore),
            camera_frame_buffer: Some(buf),
            vision_supported: true,
        };

        let result = tool.execute(serde_json::json!({"frames": 2}), &ctx).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output["frames_captured"], 2);
    }

    #[tokio::test]
    async fn camera_capture_tool_fails_when_no_buffer() {
        let tool = CameraCapture;
        let ctx = ToolContext {
            memory: Arc::new(crate::memory::sqlite::SqliteMemory::open(":memory:").await.unwrap()),
            sandbox_dir: std::env::temp_dir(),
            http_allowlist: vec![],
            permissions: Arc::new(crate::tools::PermissionChecker::allow_all()),
            secrets: Arc::new(crate::secrets::store::test_helpers::NullSecretStore),
            camera_frame_buffer: None,
            vision_supported: true,
        };

        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn camera_capture_tool_fails_when_vision_not_supported() {
        let buf = Arc::new(CameraFrameBuffer::new(16));
        buf.push(FrameData {
            id: 1, timestamp_ms: 1000,
            jpeg: vec![1], width: 1, height: 1,
        });

        let tool = CameraCapture;
        let ctx = ToolContext {
            memory: Arc::new(crate::memory::sqlite::SqliteMemory::open(":memory:").await.unwrap()),
            sandbox_dir: std::env::temp_dir(),
            http_allowlist: vec![],
            permissions: Arc::new(crate::tools::PermissionChecker::allow_all()),
            secrets: Arc::new(crate::secrets::store::test_helpers::NullSecretStore),
            camera_frame_buffer: Some(buf),
            vision_supported: false,
        };

        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p mobileclaw-core --lib -- tools::builtin::camera::tests::camera_capture
```

Expected: compile error — ToolContext doesn't have camera_frame_buffer field.

- [ ] **Step 3: Extend ToolContext**

In `mobileclaw-core/src/tools/traits.rs`, add imports and fields:

```rust
use std::sync::Arc;

// Add at top with other imports
use super::builtin::camera::CameraFrameBuffer;

pub struct ToolContext {
    pub memory: Arc<dyn Memory>,
    pub sandbox_dir: PathBuf,
    pub http_allowlist: Vec<String>,
    pub permissions: Arc<PermissionChecker>,
    pub secrets: Arc<dyn SecretStore>,
    pub camera_frame_buffer: Option<Arc<CameraFrameBuffer>>,
    pub vision_supported: bool,
}
```

- [ ] **Step 4: Implement CameraCapture tool**

In `camera.rs`, add:

```rust
use crate::tools::{Tool, ToolContext};
use crate::ClawError;
use async_trait::async_trait;
use serde_json::Value;

pub struct CameraCapture;

#[async_trait]
impl Tool for CameraCapture {
    fn name(&self) -> &str { "camera_capture" }

    fn description(&self) -> &str {
        "Capture N recent camera frames for visual analysis. \
         Returns metadata about captured frames. \
         The actual images are appended to the conversation for the LLM to analyze."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
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

    async fn execute(&self, args: Value, ctx: &ToolContext) -> crate::ClawResult<crate::tools::ToolResult> {
        if !ctx.vision_supported {
            return Err(ClawError::CameraModelNotSupported(
                "The current model does not support image analysis. \
                 Please switch to a vision-capable model (Claude 3.x, GPT-4o, etc.).".into()
            ));
        }

        let buffer = ctx.camera_frame_buffer.as_ref()
            .ok_or(ClawError::CameraUnauthorized)?;

        let n = args.get("frames")
            .and_then(Value::as_u64)
            .unwrap_or(5) as usize;
        let n = n.min(16);

        if buffer.is_empty() {
            return Err(ClawError::CameraUnauthorized);
        }

        let frames = buffer.read_latest_n(n);
        let frame_ids: Vec<u64> = frames.iter().map(|f| f.id).collect();
        let resolution = if frames.is_empty() {
            "unknown".to_string()
        } else {
            format!("{}x{}", frames[0].width, frames[0].height)
        };

        Ok(crate::tools::ToolResult::ok(serde_json::json!({
            "frames_captured": frames.len(),
            "frame_ids": frame_ids,
            "resolution": resolution,
        })))
    }
}
```

- [ ] **Step 5: Register camera_capture in register_core_builtins**

In `mobileclaw-core/src/tools/builtin/mod.rs`, add to `register_core_builtins`:

```rust
    registry.register_builtin(Arc::new(camera::CameraCapture));
```

- [ ] **Step 6: Update all existing ToolContext usages**

All existing test code that constructs `ToolContext` needs the two new fields. Add `camera_frame_buffer: None, vision_supported: true` to every `ToolContext` construction in:

- `mobileclaw-core/src/agent/loop_impl.rs` (test module)
- `mobileclaw-core/src/tools/builtin/mod.rs` (test helper `make_tool_registry_with` if it creates ToolContext)
- `mobileclaw-core/src/tools/traits.rs` (tests if any construct ToolContext)

- [ ] **Step 7: Run tests**

```bash
cargo test -p mobileclaw-core --lib
```

Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add mobileclaw-core/src/tools/traits.rs mobileclaw-core/src/tools/builtin/camera.rs mobileclaw-core/src/tools/builtin/mod.rs
git commit -m "feat(tools): add camera_capture tool with vision check"
```

---

### Task 7: AgentLoop — construct Image blocks after camera_capture

**Files:**
- Modify: `mobileclaw-core/src/agent/loop_impl.rs`
- Modify: `mobileclaw-core/src/tools/builtin/mod.rs` (test fixtures)

This is the core integration: after `camera_capture` executes successfully, the image data from the ring buffer must be appended to the message history as `ContentBlock::Image` blocks so the LLM can see them.

- [ ] **Step 1: Write the failing test**

Add to the existing test module in `loop_impl.rs`:

```rust
    #[tokio::test]
    async fn native_path_camera_capture_adds_image_to_history() {
        use std::sync::Arc;
        use crate::tools::builtin::camera::{CameraCapture, CameraFrameBuffer, FrameData};

        let dir = TempDir::new().unwrap();
        let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());

        // Create a frame buffer with test data
        let buf = Arc::new(CameraFrameBuffer::new(16));
        buf.push(FrameData {
            id: 1, timestamp_ms: 1000,
            jpeg: vec![1, 2, 3], width: 640, height: 360,
        });

        let mut registry = ToolRegistry::new();
        register_all_builtins(&mut registry);

        let ctx = ToolContext {
            memory: mem,
            sandbox_dir: dir.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: Arc::new(NullSecretStore),
            camera_frame_buffer: Some(buf.clone()),
            vision_supported: true,
        };

        // NOTE: MockLlmClient emits tool_uses on every stream_messages() call.
        // This means after camera_capture executes, the mock returns the same
        // tool_use again, triggering another round. The test runs all 10
        // MAX_TOOL_ROUNDS before the mock eventually produces text-only output
        // (if the mock's tool_uses vec is consumed) or exhausts rounds.
        // This is expected behavior — the test still validates Image block creation.
        let llm = MockLlmClient::new_native(
            "Analyzing the frames...",
            vec![("tu_cam".to_string(), "camera_capture".to_string(), serde_json::json!({"frames": 1}))],
        );

        let mut agent = AgentLoop::new(llm, registry, ctx, SkillManager::new(vec![]));
        let _ = agent.chat("look at the camera", "").await.unwrap();

        // Find the tool result message — it should contain Image blocks
        let history = agent.history();
        let tool_result_msg = history.iter().find(|m| {
            m.role == crate::llm::types::Role::User
                && m.content.iter().any(|b| matches!(b, crate::llm::types::ContentBlock::ToolResult { .. }))
        });
        assert!(tool_result_msg.is_some(), "tool result message should exist");

        // The message should contain image blocks
        let has_image = tool_result_msg.unwrap().content.iter()
            .any(|b| matches!(b, crate::llm::types::ContentBlock::Image { .. }));
        assert!(has_image, "tool result message should contain Image blocks");
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p mobileclaw-core --lib -- agent::loop_impl::tests::native_path_camera
```

Expected: test fails — Image blocks are not constructed.

- [ ] **Step 3: Modify the native tool execution path**

In `loop_impl.rs`, in the native path tool execution section (around line 392), after the tool executes successfully and before pushing the `ToolResult` content block, check if the tool was `camera_capture` and add `Image` blocks:

```rust
                    match result {
                        Ok(r) => {
                            tracing::info!(tool = %name, success = %r.success, output = %r.output, "tool result (native)");
                            all_events.push(AgentEvent::ToolResult { name: name.clone(), success: r.success });

                            // For camera_capture, also add Image blocks from the ring buffer
                            let mut result_blocks = vec![ContentBlock::ToolResult {
                                tool_use_id: id.clone(),
                                content: r.output.to_string(),
                                is_error: !r.success,
                            }];

                            if name == "camera_capture" && r.success {
                                if let Some(ref buf) = self.ctx.camera_frame_buffer {
                                    let frames_to_add = args.get("frames")
                                        .and_then(serde_json::Value::as_u64)
                                        .unwrap_or(5) as usize;
                                    let frames_to_add = frames_to_add.min(16);
                                    let frames = buf.read_latest_n(frames_to_add);
                                    for frame in frames {
                                        result_blocks.push(ContentBlock::Image {
                                            mime_type: "image/jpeg".into(),
                                            data: frame.jpeg,
                                        });
                                    }
                                    tracing::debug!(frames = frames.len(), "camera_capture: added image blocks to history");
                                }
                            }

                            result_content.extend(result_blocks);
                        }
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p mobileclaw-core --lib -- agent::loop_impl::tests
```

Expected: all tests pass including the new camera test.

- [ ] **Step 5: Commit**

```bash
git add mobileclaw-core/src/agent/loop_impl.rs
git commit -m "feat(agent): construct Image blocks after camera_capture tool execution"
```

---

### Task 8: FFI — CameraAuthRequired event + camera FFI methods

**Files:**
- Modify: `mobileclaw-core/src/ffi.rs`
- Modify: `mobileclaw-core/src/agent/loop_impl.rs` (AgentEvent enum)
- Test: `mobileclaw-core/src/ffi.rs` (tests)

- [ ] **Step 1: Add AgentEvent::CameraAuthRequired to loop_impl**

In `mobileclaw-core/src/agent/loop_impl.rs`, add to the `AgentEvent` enum:

```rust
pub enum AgentEvent {
    TextDelta { text: String },
    ToolCall { name: String },
    ToolResult { name: String, success: bool },
    ContextStats(ContextStats),
    CameraAuthRequired,  // NEW
    Done,
}
```

- [ ] **Step 2: Emit CameraAuthRequired when camera_capture fails with CameraUnauthorized**

In the native path error handling (around line 417), add:

```rust
                        Err(ClawError::CameraUnauthorized) => {
                            tracing::info!(tool = %name, "camera unauthorized — emitting auth required event");
                            all_events.push(AgentEvent::CameraAuthRequired);
                            all_events.push(AgentEvent::ToolResult { name: name.clone(), success: false });
                            result_content.push(ContentBlock::ToolResult {
                                tool_use_id: id.clone(),
                                content: serde_json::json!({
                                    "error": "camera_unauthorized",
                                    "message": "Camera access denied. The user has not authorized camera access."
                                }).to_string(),
                                is_error: true,
                            });
                        }
```

- [ ] **Step 2b: XML path — emit CameraAuthRequired for camera_capture failures**

In the XML path tool execution section (around line 492), after the existing `match result` block, add handling:

```rust
                        Err(ClawError::CameraUnauthorized) => {
                            tracing::info!(tool = %name, "camera unauthorized (XML path)");
                            all_events.push(AgentEvent::CameraAuthRequired);
                            all_events.push(AgentEvent::ToolResult { name: call.name.clone(), success: false });
                            tool_results_xml.push_str(&format_tool_result(&call.name, false,
                                &serde_json::json!({"error": "camera_unauthorized"}).to_string()));
                        }
```

Note: For the XML path, vision_supported is typically false (Ollama), so camera_capture will fail with CameraModelNotSupported before reaching CameraUnauthorized. This is defense-in-depth.

- [ ] **Step 3: Convert CameraAuthRequired in FFI chat()**

In `AgentSession::chat()` Phase D (DTO conversion), add the mapping:

```rust
                AgentEvent::CameraAuthRequired => AgentEventDto::CameraAuthRequired,
```

- [ ] **Step 4: Add CameraAuthRequired to AgentEventDto**

In `ffi.rs`, add to `AgentEventDto`:

```rust
pub enum AgentEventDto {
    // ... existing variants ...
    CameraAuthRequired,
}
```

- [ ] **Step 5: Add camera fields to AgentConfig**

```rust
pub struct AgentConfig {
    // ... existing fields ...
    pub camera_frames_per_capture: Option<u32>,
    pub camera_max_frames_per_capture: Option<u32>,
    pub camera_ring_buffer_capacity: Option<usize>,
}
```

- [ ] **Step 6: Create RingBuffer in AgentSession::create() and pass to ToolContext**

In `AgentSession::create()`, before creating the `ToolContext`:

```rust
        use crate::tools::builtin::camera::CameraFrameBuffer;
        let ring_capacity = config.camera_ring_buffer_capacity.unwrap_or(16);
        let camera_buffer = Arc::new(CameraFrameBuffer::new(ring_capacity));

        let ctx = ToolContext {
            memory: memory.clone() as Arc<dyn Memory>,
            sandbox_dir: config.sandbox_dir.into(),
            http_allowlist: config.http_allowlist,
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: secrets.clone() as Arc<dyn crate::secrets::SecretStore>,
            camera_frame_buffer: Some(camera_buffer.clone()),
            vision_supported: llm.vision_supported(),
        };
```

Store the camera_buffer and monitor state in AgentSession:

```rust
use tokio_util::sync::CancellationToken;  // NEW import
use crate::tools::builtin::camera::CameraFrameBuffer;  // NEW import

pub struct AgentSession {
    inner: AgentLoop<std::sync::Arc<dyn crate::llm::client::LlmClient>>,
    memory: Arc<SqliteMemory>,
    secrets: Arc<SqliteSecretStore>,
    session_dir: Option<std::path::PathBuf>,
    session_id: String,
    camera_buffer: Arc<CameraFrameBuffer>,           // NEW
    monitor_tokens: Vec<(String, CancellationToken)>, // NEW: for Phase 2 monitor cleanup
}
```

- [ ] **Step 7: Add camera_push_frame FFI function**

In `ffi.rs`, add as a free function:

```rust
/// Push a camera frame from Flutter into the ring buffer.
/// Returns false if the handle is invalid.
pub fn camera_push_frame(
    session_ptr: i64,
    jpeg: Vec<u8>,
    frame_id: u64,
    timestamp_ms: u64,
    width: u32,
    height: u32,
) -> bool {
    use crate::tools::builtin::camera::{CameraFrameBuffer, FrameData};

    // SAFETY: session_ptr is a pointer cast from Arc<AgentSession>.
    // The caller (Dart) must ensure the session outlives all push_frame calls.
    let session_ptr = session_ptr as *const AgentSession;
    if session_ptr.is_null() {
        return false;
    }

    // SAFETY: We read the camera_buffer Arc from the session.
    // The Arc keeps the buffer alive even if the session is dropped.
    let session = unsafe { &*session_ptr };
    let buffer = &session.camera_buffer;
    buffer.push(FrameData {
        id: frame_id,
        timestamp_ms,
        jpeg,
        width,
        height,
    });
    true
}
```

- [ ] **Step 8: Add camera_set_authorized, camera_start_monitor, camera_stop_monitor stubs**

Add placeholder implementations to `AgentSession`:

```rust
    pub fn camera_set_authorized(&mut self, _authorized: bool) {
        // Phase 1: no-op. Authorization is managed by Flutter starting/stopping frame push.
        tracing::info!(authorized = _authorized, "camera authorization set");
    }

    pub async fn camera_start_monitor(
        &mut self,
        _scenario: String,
        _frames_per_check: u32,
        _check_interval_ms: u32,
        _cooldown_after_alert_ms: u32,
    ) -> anyhow::Result<String> {
        // Phase 2: implement background monitor
        Ok("monitor-id-todo".to_string())
    }

    pub fn camera_stop_monitor(&mut self, _monitor_id: &str) -> bool {
        // Phase 2: implement
        false
    }
```

- [ ] **Step 9: Run tests**

```bash
cargo test -p mobileclaw-core --lib
```

Expected: all tests pass.

- [ ] **Step 10: Commit**

```bash
git add mobileclaw-core/src/ffi.rs mobileclaw-core/src/agent/loop_impl.rs
git commit -m "feat(ffi): add CameraAuthRequired event and camera FFI methods"
```

---

### Task 9: Sweep — fix any remaining compilation errors

**Files:** Search all files for `ToolContext {`

After Tasks 6 and 8, most ToolContext constructions should already be updated.
This task catches any stragglers that were missed.

- [ ] **Step 1: Find all remaining ToolContext constructions**

```bash
grep -rn "ToolContext {" mobileclaw-core/src/
```

- [ ] **Step 2: Fix any that still lack the two new fields**

```bash
cargo test -p mobileclaw-core --lib
```

If compilation fails with "missing fields `camera_frame_buffer` and `vision_supported`",
fix each location by adding `camera_frame_buffer: None, vision_supported: true` to the
`ToolContext` construction.

Common locations to check:
- `mobileclaw-core/src/tools/builtin/file.rs` (tests)
- `mobileclaw-core/src/tools/builtin/http.rs` (tests)
- `mobileclaw-core/src/tools/builtin/memory_tools.rs` (tests)

- [ ] **Step 3: Run full test suite**

```bash
cargo test -p mobileclaw-core --features test-utils
```

Expected: all tests pass.

- [ ] **Step 4: Run clippy**

```bash
cargo clippy -p mobileclaw-core --features test-utils -- -D warnings
```

Expected: zero warnings.

- [ ] **Step 5: Commit**

```bash
git add mobileclaw-core/src/
git commit -m "chore: fix all remaining ToolContext constructions for camera fields"
```

---

### Task 10: CameraAlert struct + FRB stream (background monitor scaffold)

**Files:**
- Modify: `mobileclaw-core/src/ffi.rs`
- Modify: `mobileclaw-core/src/agent/loop_impl.rs`

- [ ] **Step 1: Add CameraAlert struct**

In `ffi.rs`:

```rust
#[derive(Debug, Clone)]
pub struct CameraAlert {
    pub summary: String,
    pub frame_id: u64,
    pub timestamp_ms: u64,
}
```

- [ ] **Step 2: Add camera_alert_stream scaffold**

Note: The design spec calls for `#[frb(stream)]` with `impl Stream<Item = CameraAlert>`.
For Phase 1 we use a sync stub returning an empty vec. The real stream replaces this
in Phase 2 when the background monitor task is implemented.

```rust
    #[frb(sync)]
    pub fn camera_alert_stream(&self) -> Vec<CameraAlert> {
        // Phase 2: implement FRB stream backed by mpsc channel
        vec![]
    }
```

- [ ] **Step 3: Run tests and clippy**

```bash
cargo test -p mobileclaw-core --features test-utils
cargo clippy -p mobileclaw-core --features test-utils -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add mobileclaw-core/src/ffi.rs
git commit -m "feat(ffi): add CameraAlert struct and stream scaffold"
```

---

### Task 11: Integration test — end-to-end camera capture

**Files:**
- Create: `mobileclaw-core/tests/integration_camera.rs`

- [ ] **Step 1: Write the integration test**

```rust
//! Integration test: camera capture through the full agent loop.
//! Requires --features test-utils.

use std::sync::Arc;
use tempfile::TempDir;

use mobileclaw_core::agent::loop_impl::AgentLoop;
use mobileclaw_core::llm::client::test_helpers::MockLlmClient;
use mobileclaw_core::memory::sqlite::SqliteMemory;
use mobileclaw_core::secrets::store::test_helpers::NullSecretStore;
use mobileclaw_core::skill::SkillManager;
use mobileclaw_core::tools::{ToolContext, ToolRegistry, PermissionChecker, builtin::{register_all_builtins, camera::{CameraFrameBuffer, FrameData}}};

#[tokio::test]
async fn camera_capture_full_agent_loop() {
    let dir = TempDir::new().unwrap();
    let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());

    let buf = Arc::new(CameraFrameBuffer::new(16));
    buf.push(FrameData {
        id: 1, timestamp_ms: 1000,
        jpeg: vec![0xFF, 0xD8, 0xFF, 0xE0],
        width: 640, height: 360,
    });

    let mut registry = ToolRegistry::new();
    register_all_builtins(&mut registry);

    let ctx = ToolContext {
        memory: mem,
        sandbox_dir: dir.path().to_path_buf(),
        http_allowlist: vec![],
        permissions: Arc::new(PermissionChecker::allow_all()),
        secrets: Arc::new(NullSecretStore),
        camera_frame_buffer: Some(buf),
        vision_supported: true,
    };

    // Mock that triggers camera_capture then responds with text
    let llm = MockLlmClient::new_native(
        "I can see the camera feed.",
        vec![("tu_1".to_string(), "camera_capture".to_string(), serde_json::json!({"frames": 1}))],
    );

    let mut agent = AgentLoop::new(llm, registry, ctx, SkillManager::new(vec![]));
    let events = agent.chat("check the camera", "").await.unwrap();

    // Should have ToolCall and ToolResult for camera_capture
    let tool_calls: Vec<_> = events.iter()
        .filter(|e| matches!(e, mobileclaw_core::agent::loop_impl::AgentEvent::ToolCall { name } if name == "camera_capture"))
        .collect();
    assert!(!tool_calls.is_empty(), "camera_capture tool should be called");

    // History should contain Image blocks in the tool result message
    let has_image_in_history = agent.history().iter().any(|m| {
        m.content.iter().any(|b| matches!(b, mobileclaw_core::llm::types::ContentBlock::Image { .. }))
    });
    assert!(has_image_in_history, "history should contain Image blocks from camera_capture");
}
```

- [ ] **Step 2: Run the test**

```bash
cargo test -p mobileclaw-core --features test-utils --test integration_camera
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add mobileclaw-core/tests/integration_camera.rs
git commit -m "test(camera): integration test for camera capture through agent loop"
```

---

### Task 12: Coverage check and final cleanup

- [ ] **Step 1: Run coverage check**

```bash
cargo llvm-cov --package mobileclaw-core --features test-utils --all-targets --fail-under-lines 85
```

- [ ] **Step 2: Run full test suite**

```bash
cargo test -p mobileclaw-core --features test-utils
```

- [ ] **Step 3: Run clippy**

```bash
cargo clippy -p mobileclaw-core --features test-utils -- -D warnings
```

- [ ] **Step 4: Verify git status is clean**

```bash
git diff --stat
```

All changes should be committed at this point.
