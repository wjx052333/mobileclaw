# Camera Pipeline Performance Logging — Design Spec

**Date:** 2026-04-04  
**Status:** Approved  
**Scope:** Flutter + Rust instrumentation for camera → JPEG → FFI pipeline; stress-test benchmark with log-based result extraction.

---

## 1. Problem

The camera data path (YUV420 → JPEG encode → Dart FFI → Rust ring buffer) has no timing instrumentation. We need to know real latency distributions on an Android emulator under a sustained 100-frame load, with results persisted to the existing log files so they survive the test run.

---

## 2. Pipeline Under Test

```
CameraController.imageStream
  → CameraImage (YUV420)
  → [FLUTTER] img.encodeJpg()              ← encode stage
  → [FLUTTER] agent.cameraPushFrame()      ← ffi_total stage (Dart call + Rust execution)
      → [RUST]  camera_push_frame_dart()
          → RingBuffer.push()              ← rust_push stage
```

Three stages are independently timed:

| Stage | Owner | Measured as |
|---|---|---|
| `encode` | Flutter | `img.encodeJpg()` wall time |
| `ffi_total` | Flutter | `cameraPushFrame()` round-trip wall time |
| `rust_push` | Rust | `RingBuffer.push()` wall time inside FFI |

---

## 3. Instrumentation Design

### 3.1 Flutter Side

**Where:** `camera_real_capture_test.dart` — new benchmark helper `_benchmarkFrames()`.

**How:** `Stopwatch` per stage, results accumulated in `List<int>` (microseconds).

```dart
// pseudocode
final encodeUs = <int>[];
final ffiUs    = <int>[];

for (final rawFrame in rawFrames) {
  final sw = Stopwatch()..start();
  final jpeg = _cameraImageToJpeg(rawFrame);
  encodeUs.add(sw.elapsedMicroseconds); sw.reset(); sw.start();
  await agent.cameraPushFrame(jpeg: jpeg, ...);
  ffiUs.add(sw.elapsedMicroseconds);
}
```

After the loop, compute `_stats(List<int>)` → `PerfStats(min, p50, p95, max, mean)`, then call `_logPerf()` (new helper, parallel to existing `_logError`, writes to `flutter.log`):

```
[MCLAW_PERF_SUMMARY] frames=100 stage=encode   min_us=8100  p50_us=11400 p95_us=16200 max_us=23000 mean_us=11800
[MCLAW_PERF_SUMMARY] frames=100 stage=ffi_total min_us=180   p50_us=240   p95_us=410   max_us=980   mean_us=260
```

**Log file:** Same `flutter.log` used by `_logError` (in `getApplicationSupportDirectory()`).

### 3.2 Rust Side

**Where:** `ffi.rs` — `AgentSession` struct and `camera_push_frame_dart()` method.

**New field on `AgentSession`:**
```rust
camera_perf: Arc<Mutex<Vec<u64>>>,  // per-push elapsed microseconds
```

**In `camera_push_frame_dart()`:**
```rust
let t = std::time::Instant::now();
self.camera_buffer.push(frame_data);
let elapsed_us = t.elapsed().as_micros() as u64;
self.camera_perf.lock().unwrap().push(elapsed_us);
```

**New FFI method `camera_perf_flush()`:** Called by Flutter after the benchmark loop. Drains the vec, computes stats, writes one `tracing::info!` line to `mobileclaw.log`, returns nothing (fire-and-forget for the test).

```
INFO mobileclaw_core::ffi: [MCLAW_PERF_SUMMARY] frames=100 stage=rust_push min_us=120 p50_us=190 p95_us=380 max_us=870 mean_us=210
```

The `camera_perf` vec is cleared after flush so the accumulator resets for subsequent runs.

### 3.3 Log Line Format (parseable contract)

Both sides emit lines matching this regex:

```
\[MCLAW_PERF_SUMMARY\] frames=(\d+) stage=(\S+) min_us=(\d+) p50_us=(\d+) p95_us=(\d+) max_us=(\d+) mean_us=(\d+)
```

This format is stable — tests rely on it for parsing.

---

## 4. Benchmark Integration Test

**File:** `integration_test/camera_bench_test.dart` (new file, separate from `camera_real_capture_test.dart`).

**Test:** `bench_100_frames_camera_pipeline`

Steps:
1. `setUp`: create `AgentSession` with `log_dir` pointing to a temp dir (so `mobileclaw.log` lands in a known path)
2. Capture 100 real camera frames with `_captureRealFrames(count: 100)` (reuse existing helper)
3. Run `_benchmarkFrames(agent, rawFrames)` — encodes + pushes all frames, accumulates Flutter timings
4. Call `agent.cameraPerfFlush()` — triggers Rust to log its summary to `mobileclaw.log`
5. Write Flutter summary to `flutter.log` via `_logPerf()`
6. `adb pull` both log files into a temp dir on the host
7. Parse `[MCLAW_PERF_SUMMARY]` lines from both files with `_parsePerfSummary()`
8. Assert all three stages are present in the parsed results
9. Print combined table to stdout:

```
stage        | frames | min    | p50    | p95    | max    | mean
-------------|--------|--------|--------|--------|--------|-------
encode       |    100 |  8.1ms | 11.4ms | 16.2ms | 23.0ms | 11.8ms
ffi_total    |    100 |  0.2ms |  0.2ms |  0.4ms |  1.0ms |  0.3ms
rust_push    |    100 |  0.1ms |  0.2ms |  0.4ms |  0.9ms |  0.2ms
```

The test **asserts** (not just prints):
- All three stages are present in parsed logs
- `p95 > 0` for each stage (sanity: we actually measured something)
- `encode` p50 < 500 ms (sanity: not pathologically slow)

---

## 5. New Dart/Rust API Surface

| Item | Type | Description |
|---|---|---|
| `MobileclawAgent.cameraPerfFlush()` | `Future<void>` | Drain Rust perf vec, write summary to `mobileclaw.log` |
| `MockMobileclawAgent.cameraPerfFlush()` | no-op | Tests that don't need real timing |
| `AgentSession.cameraPerfFlush()` (Rust FFI) | sync | Flush + clear accumulator |
| `_logPerf(String line)` | Dart helper | Append to `flutter.log` (mirrors `_logError`) |
| `_benchmarkFrames(agent, frames)` | Dart helper | Run encode+push loop, return `BenchResult` |
| `_parsePerfSummary(String logContent)` | Dart helper | Parse `[MCLAW_PERF_SUMMARY]` lines → `Map<String,PerfStats>` |

---

## 6. Files Changed

| File | Change |
|---|---|
| `mobileclaw-core/src/ffi.rs` | Add `camera_perf: Arc<Mutex<Vec<u64>>>` to `AgentSession`; accumulate in `camera_push_frame_dart`; add `camera_perf_flush()` |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/engine.dart` | Add `cameraPerfFlush()` to abstract class |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart` | Implement `cameraPerfFlush()` via FFI |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/mock.dart` | No-op `cameraPerfFlush()` |
| `mobileclaw-flutter/apps/mobileclaw_app/integration_test/camera_bench_test.dart` | New benchmark test |

No changes to `camera_real_capture_test.dart` (reuse its helpers via import or copy).

---

## 7. Out of Scope

- Continuous background monitoring (not a benchmark scenario)
- iOS support (benchmark targets Android emulator only)
- Histogram persistence beyond the single test run
- Per-frame log lines (intentionally excluded — only summary statistics)
