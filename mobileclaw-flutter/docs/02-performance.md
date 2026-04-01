# Performance Constraints

**These are binding requirements, not aspirational goals.**  
Features that violate them must be redesigned, not merged.

---

## 1. Hard Limits (must not be exceeded)

| Metric | Limit | Measurement point |
|--------|-------|-------------------|
| First TextDelta latency | **< 500 ms** | `sendMessage()` call → first `TextDeltaEvent` received in Dart |
| MEMORY.md load | **< 5 ms** | Rust `MemoryManager::load_prompt()` on a 25 KB index |
| FTS5 memory search | **< 20 ms** | 1 000 documents, 10-word query |
| `http_request` tool (P95) | **< 3 s** | wall-clock including DNS + TLS |
| WASM tool startup | **< 50 ms** | instantiation → first host function call |
| SQLite WAL write | **< 2 ms** | single row INSERT |
| Image processing (1 frame) | **< 200 ms** | resize + JPEG encode in Tokio thread pool |
| Video frame extraction (30 s clip) | **< 2 s** | extract 20 key frames via platform API |
| Rust idle heap | **< 30 MB** | RSS after engine init, no active session |
| Rust session heap | **< 80 MB** | includes loaded Whisper model |

---

## 2. Flutter UI Frame Budget

The Flutter render thread must sustain **60 fps (16.7 ms/frame)** at all times.

Rules:
- **Never** call any `MobileclawAgent` method synchronously in a `build()` method.
- All agent calls are `async`; state updates flow through Riverpod providers.
- `TextDeltaEvent` processing uses `addPostFrameCallback` — never `setState`  
  from inside `StreamBuilder.builder` synchronously.
- Media picks (image/video) run in background isolates before passing bytes  
  to Rust via `FfiMedia`.

---

## 3. Streaming Architecture

```
Rust AgentLoop  →  tokio channel  →  claw_ffi StreamSink
        ↓                                     ↓
  TextDelta chunks                   FfiEvent stream (Dart)
  (~20-50 ms inter-chunk)            StreamBuilder rebuilds only text widget
```

- Each `TextDeltaEvent` must trigger a **partial** UI update — do not buffer  
  the entire response before rendering.
- Use `StringBuffer` to accumulate deltas; call `setState` at most once per  
  animation frame via `addPostFrameCallback`.

---

## 4. SQLite Tuning

The following PRAGMAs are set by `claw_storage` on every connection open.  
They are performance-critical and must not be altered without benchmarking:

```sql
PRAGMA journal_mode = WAL;          -- concurrent reads during writes
PRAGMA synchronous   = NORMAL;      -- fsync on checkpoint, not every write
PRAGMA mmap_size     = 8388608;     -- 8 MB memory-mapped I/O
PRAGMA cache_size    = -2000;       -- ~2 MB page cache
PRAGMA temp_store    = MEMORY;      -- temp tables in RAM
PRAGMA foreign_keys  = ON;
```

---

## 5. Media Pipeline Rules

- Images ≤ 20 MB accepted; larger → `MEDIA_TOO_LARGE` error.
- Videos ≤ 200 MB / ≤ 10 min; truncate to first 10 min silently.
- Maximum **20 frames** per video — prevents token budget explosion.
- HEIC decoding is **platform-native only** (iOS: `CGImageSource`,  
  Android: `BitmapFactory`). Do not add `libheif` to mobile targets.
- **Never ship `ffmpeg-next` / `libffmpeg` on mobile** (package size  
  +10–20 MB, LGPL/GPL risk, App Store audit risk).

---

## 6. Benchmarking Hooks

Every performance-critical path emits a `duration_ms` field in `ToolOutput`  
and `FfiEvent::ToolCallEnd`. The Flutter layer must surface these in  
`tool_call_card.dart` so slow tools are visible to the user.

CI gate: add a benchmark test that asserts `MockMobileclawAgent.chat()` end-to-end  
latency (mock only, no network) is under 100 ms for a 10-word input.
