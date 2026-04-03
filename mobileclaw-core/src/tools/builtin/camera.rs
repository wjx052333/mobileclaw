//! Camera frame buffer and capture tool.
//!
//! Phase 1: `RingBuffer` backed by `VecDeque` for in-memory frame storage.
//! Phase 2: swap to mmap-backed ring buffer for zero-copy FFI (Dart writes
//! directly into mmap slots).
//!
//! The public API (`push`, `read_latest_n`, `is_empty`, `latest_timestamp_ms`)
//! is stable — callers don't need to change when the backing storage changes.

use std::collections::VecDeque;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Frame types
// ---------------------------------------------------------------------------

/// Fixed-size header for each frame slot. Must be repr(C) for cross-FFI
/// compatibility in Phase 2 (mmap-backed storage).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FrameHeader {
    pub id: u64,
    pub timestamp_ms: u64,
    pub size: u32,    // 0 = empty slot, >0 = valid frame
    pub width: u16,
    pub height: u16,
}

/// Rust-side frame data (copied from mmap or ring buffer for LLM use).
#[derive(Debug, Clone)]
pub struct FrameData {
    pub id: u64,
    pub timestamp_ms: u64,
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

// ---------------------------------------------------------------------------
// RingBuffer
// ---------------------------------------------------------------------------

/// Fixed-capacity ring buffer. Oldest items are evicted when full.
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

impl<T> RingBuffer<T>
where
    T: HasTimestamp + Clone,
{
    pub fn latest_timestamp_ms(&self) -> Option<u64> {
        let buf = self.buffer.lock().expect("ring buffer poisoned");
        buf.back().map(|item| item.timestamp_ms())
    }
}

/// Type alias used throughout the codebase.
pub type CameraFrameBuffer = RingBuffer<FrameData>;

// ---------------------------------------------------------------------------
// CameraCapture tool (defined here, registered in mod.rs)
// ---------------------------------------------------------------------------

use crate::tools::{Tool, ToolContext, ToolResult};
use crate::ClawError;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::atomic::Ordering;

/// Capture N recent camera frames for visual analysis.
/// The LLM sees the frames as `ContentBlock::Image` blocks appended to
/// the message history by `AgentLoop` (via `Tool::produces_images()`).
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

    fn produces_images(&self) -> bool { true }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> crate::ClawResult<ToolResult> {
        if !ctx.vision_supported {
            return Err(ClawError::CameraModelNotSupported(
                "The current model does not support image analysis. \
                 Please switch to a vision-capable model (Claude 3.x, GPT-4o, etc.).".into()
            ));
        }

        let n = args.get("frames")
            .and_then(Value::as_u64)
            .unwrap_or(5) as usize;
        let n = n.min(16);

        // Check authorization first
        if !ctx.camera_authorized.load(Ordering::Relaxed) {
            return Err(ClawError::CameraUnauthorized);
        }

        let buffer = ctx.camera_frame_buffer.as_ref()
            .ok_or(ClawError::CameraUnauthorized)?;

        // Authorized but buffer empty → frames not arriving yet
        if buffer.is_empty() {
            return Err(ClawError::CameraFrameTimeout(5));
        }

        let frames = buffer.read_latest_n(n);
        let frame_ids: Vec<u64> = frames.iter().map(|f| f.id).collect();
        let resolution = if frames.is_empty() {
            "unknown".to_string()
        } else {
            format!("{}x{}", frames[0].width, frames[0].height)
        };

        Ok(ToolResult::ok(serde_json::json!({
            "frames_captured": frames.len(),
            "frame_ids": frame_ids,
            "resolution": resolution,
        })))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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

    #[tokio::test]
    async fn camera_capture_tool_returns_frame_metadata() {
        let buf = Arc::new(CameraFrameBuffer::new(16));
        let auth = Arc::new(std::sync::atomic::AtomicBool::new(true));
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
            camera_authorized: auth,
            vision_supported: true,
        };

        let result = tool.execute(serde_json::json!({"frames": 2}), &ctx).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output["frames_captured"], 2);
    }

    #[tokio::test]
    async fn camera_capture_tool_fails_when_no_buffer() {
        let tool = CameraCapture;
        let auth = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ctx = ToolContext {
            memory: Arc::new(crate::memory::sqlite::SqliteMemory::open(":memory:").await.unwrap()),
            sandbox_dir: std::env::temp_dir(),
            http_allowlist: vec![],
            permissions: Arc::new(crate::tools::PermissionChecker::allow_all()),
            secrets: Arc::new(crate::secrets::store::test_helpers::NullSecretStore),
            camera_frame_buffer: None,
            camera_authorized: auth,
            vision_supported: true,
        };

        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn camera_capture_tool_fails_when_vision_not_supported() {
        let buf = Arc::new(CameraFrameBuffer::new(16));
        let auth = Arc::new(std::sync::atomic::AtomicBool::new(true));
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
            camera_authorized: auth,
            vision_supported: false,
        };

        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[test]
    fn camera_capture_produces_images_returns_true() {
        let tool = CameraCapture;
        assert!(tool.produces_images());
    }
}
