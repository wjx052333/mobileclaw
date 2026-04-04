# Camera Pipeline Performance Logging — Design Spec

**Date:** 2026-04-04  
**Status:** Approved (v2 — post spec-review fixes)  
**Scope:** Flutter + Rust instrumentation for camera → JPEG → FFI pipeline; stress-test benchmark with log-based result extraction.

---

## 1. Problem

The camera data path (YUV420 → JPEG encode → Dart FFI → Rust ring buffer) has no timing instrumentation. We need to know real latency distributions on an Android emulator under a sustained 100-frame load, with results persisted to the existing log files so they can be read back and asserted on inside the same test process.

---

## 2. Pipeline Under Test

```
CameraController.imageStream
  → CameraImage (YUV420)               ← raw frame, NOT yet encoded
  → [FLUTTER] _cameraImageToJpeg()     ← encode stage
  → [FLUTTER] agent.cameraPushFrame()  ← ffi_total stage (Dart call + Rust execution)
      → [RUST]  camera_push_frame_dart()
          → RingBuffer.push()          ← rust_push stage
```

Three stages are independently timed:

| Stage | Owner | Measured as |
|---|---|---|
| `encode` | Flutter | `_cameraImageToJpeg()` wall time |
| `ffi_total` | Flutter | `cameraPushFrame()` round-trip wall time |
| `rust_push` | Rust | `RingBuffer.push()` wall time inside FFI |

---

## 3. Instrumentation Design

### 3.1 Flutter Side

**Where:** `camera_bench_test.dart` — helpers `_captureCameraImages()` and `_benchmarkFrames()`.

**Key distinction from existing `_captureRealFrames()`:**  
The existing helper in `camera_real_capture_test.dart` already encodes frames to JPEG before returning `List<List<int>>`. It **cannot** be reused for the encode-stage timing benchmark. A new helper `_captureCameraImages()` must return raw `List<CameraImage>` so that the benchmark loop can time `_cameraImageToJpeg()` independently.

**Scope note:** Storing `CameraImage` objects after the stream callback returns is safe on the Android emulator (the virtual camera plugin allocates frames as ordinary Dart `Uint8List`s). On physical devices the plane bytes may be native-buffer-backed views valid only for the callback duration. This benchmark targets emulator only (see Section 7).

```dart
/// Capture [count] raw CameraImage frames without encoding.
Future<List<CameraImage>> _captureCameraImages({required int count, ...}) async {
  // same controller setup as _captureRealFrames but accumulates CameraImage,
  // not JPEG bytes. CameraImage plane bytes are plain Uint8List — safe to hold.
}
```

**Benchmark loop (in `_benchmarkFrames`):**

```dart
final encodeUs = <int>[];
final ffiUs    = <int>[];

for (var i = 0; i < rawFrames.length; i++) {
  final sw = Stopwatch()..start();
  final jpeg = _cameraImageToJpeg(rawFrames[i])!;
  encodeUs.add(sw.elapsedMicroseconds);

  sw.reset(); sw.start();
  await agent.cameraPushFrame(jpeg: jpeg, frameId: i + 1,
      timestampMs: DateTime.now().millisecondsSinceEpoch, width: 320, height: 240);
  ffiUs.add(sw.elapsedMicroseconds);
}
```

After the loop, compute `_perfStats(List<int>)` → `PerfStats` for each stage, write **two** `[MCLAW_PERF_SUMMARY]` lines to `'${logDir}/flutter.log'` using a helper `_logPerf(String logDir, String line)` that appends directly to that path.

**`logDir` is always the test-controlled `tmpDir.path`** — the same directory passed as `logDir` to `AgentSession.create()`. This ensures both log files (`flutter.log` and `mobileclaw.log`) land in the same directory and can be read with `File('${tmpDir.path}/flutter.log').readAsString()` — no `adb` needed.

**`_logPerf` signature:**
```dart
Future<void> _logPerf(String logDir, String line) async {
  final f = File('$logDir/flutter.log');
  await f.writeAsString('$line\n', mode: FileMode.append, flush: true);
}
```

**`_perfStats` edge cases:**
- Empty input: returns `null`; caller skips writing the log line entirely.
- Single sample: p50 = p95 = max = min = that value. Percentile index: `sorted[(n * pct ~/ 100).clamp(0, n - 1)]` — correct for n = 1.

### 3.2 Rust Side

**New field on `AgentSession`:**
```rust
camera_perf: Arc<Mutex<Vec<u64>>>,  // per-push elapsed microseconds
```
Initialized as `Arc::new(Mutex::new(Vec::new()))` in `AgentSession::create()`.

**In `camera_push_frame_dart()`:**
```rust
let t = std::time::Instant::now();
self.camera_buffer.push(FrameData { ... });
let elapsed_us = t.elapsed().as_micros() as u64;
self.camera_perf.lock().expect("perf lock poisoned").push(elapsed_us);
```

**New method `camera_perf_flush()` on `AgentSession`:**
- Return type: `()` (sync, not async, infallible — logging failures are swallowed)
- Drains the vec (swap with empty), computes stats, writes one `tracing::info!` line if the vec was non-empty, does nothing if empty.
- Empty-vec behaviour: **no log line emitted** (so the test's assertion that `rust_push` is present is a real correctness check, not vacuous).

```rust
pub fn camera_perf_flush(&self) {
    let samples = {
        let mut v = self.camera_perf.lock().expect("perf lock poisoned");
        std::mem::take(&mut *v)
    };
    if samples.is_empty() { return; }
    let mut s = samples;
    s.sort_unstable();
    let n = s.len();
    let p50 = s[n * 50 / 100];
    let p95 = s[(n * 95 / 100).min(n - 1)];
    let min = s[0]; let max = s[n - 1];
    let mean = s.iter().sum::<u64>() / n as u64;
    tracing::info!(
        "[MCLAW_PERF_SUMMARY] frames={n} stage=rust_push \
         min_us={min} p50_us={p50} p95_us={p95} max_us={max} mean_us={mean}"
    );
}
```

### 3.3 Log Line Format (parseable contract)

Both sides emit lines matching this regex:
```
\[MCLAW_PERF_SUMMARY\] frames=(\d+) stage=(\S+) min_us=(\d+) p50_us=(\d+) p95_us=(\d+) max_us=(\d+) mean_us=(\d+)
```

This format is stable — `_parsePerfSummary()` relies on it.  
Lines are never emitted for empty sample sets (defined behaviour, not undefined).

---

## 4. Benchmark Integration Test

**File:** `integration_test/camera_bench_test.dart` (new file).

**Test:** `bench_100_frames_camera_pipeline`

```
setUp:
  tmpDir = Directory.systemTemp.createTempSync('mclaw_bench_')
  // Copy secrets.db into tmpDir (same pattern as camera_test.dart)
  agent = await MobileclawAgentImpl.create(
    ...,
    logDir: tmpDir.path,   ← mobileclaw.log lands here
  )

test body:
  1. rawFrames = await _captureCameraImages(count: 100)
  2. _benchmarkFrames(agent, rawFrames, logDir: tmpDir.path)
       — encodes, pushes, accumulates timings
       — writes [MCLAW_PERF_SUMMARY] lines to flutter.log in tmpDir
  3. agent.cameraPerfFlush()
       — Rust logs [MCLAW_PERF_SUMMARY] to mobileclaw.log in tmpDir
  4. flutterLog  = await File('${tmpDir.path}/flutter.log').readAsString()
     rustLog     = await File('${tmpDir.path}/mobileclaw.log').readAsString()
  5. stats = _parsePerfSummary(flutterLog + '\n' + rustLog)
       — returns Map<String, PerfStats> keyed by stage name
  6. assert stats.containsKey('encode')
     assert stats.containsKey('ffi_total')
     assert stats.containsKey('rust_push')
     assert stats['encode']!.p95 > 0
     assert stats['ffi_total']!.p95 > 0
     assert stats['rust_push']!.p95 > 0
     assert stats['encode']!.p50 < 500000  // sanity: < 500 ms
  7. print summary table (stdout)

tearDown:
  agent.dispose()
  tmpDir.deleteSync(recursive: true)
```

**No `adb` calls inside the test.** Both log files are in `tmpDir` which is readable by the app process via `dart:io`.

**Summary table format:**
```
stage        | frames | min    | p50    | p95    | max    | mean
-------------|--------|--------|--------|--------|--------|-------
encode       |    100 |  8.1ms | 11.4ms | 16.2ms | 23.0ms | 11.8ms
ffi_total    |    100 |  0.2ms |  0.2ms |  0.4ms |  1.0ms |  0.3ms
rust_push    |    100 |  0.1ms |  0.2ms |  0.4ms |  0.9ms |  0.2ms
```

---

## 5. New Dart/Rust API Surface

| Item | Type | Notes |
|---|---|---|
| `MobileclawAgent.cameraPerfFlush()` | `Future<void>` abstract | Added to `engine.dart` |
| `MobileclawAgentImpl.cameraPerfFlush()` | `Future<void>` | Delegates to `_session.cameraPerfFlush()` |
| `MockMobileclawAgent.cameraPerfFlush()` | no-op `Future<void>` | Tests that don't need real timing |
| `AgentSession.camera_perf_flush()` (Rust) | `fn() -> ()` sync | Flush + clear accumulator, logs to `mobileclaw.log` |
| `_captureCameraImages()` | Dart helper in bench test | Returns `List<CameraImage>` (raw, unencoded) |
| `_benchmarkFrames()` | Dart helper in bench test | Encode+push loop, writes flutter.log, returns nothing |
| `_logPerf(logDir, line)` | Dart helper in bench test | Appends to `$logDir/flutter.log` |
| `_perfStats(List<int>)` | Dart helper in bench test | Returns `PerfStats?`; null if empty |
| `_parsePerfSummary(String)` | Dart helper in bench test | Parses `[MCLAW_PERF_SUMMARY]` → `Map<String,PerfStats>` |

---

## 6. Files Changed

| File | Change |
|---|---|
| `mobileclaw-core/src/ffi.rs` | Add `camera_perf` field; accumulate in `camera_push_frame_dart`; add `camera_perf_flush()` |
| `mobileclaw-core/src/frb_generated.rs` | **Regenerate** via `flutter_rust_bridge_codegen generate` |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/bridge/frb_generated.dart` | **Regenerate** |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/engine.dart` | Add `cameraPerfFlush()` |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart` | Implement `cameraPerfFlush()` |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/mock.dart` | No-op `cameraPerfFlush()` |
| `mobileclaw-flutter/apps/mobileclaw_app/integration_test/camera_bench_test.dart` | New file |

**Code generation:** After adding `camera_perf_flush()` to Rust, run:
```bash
cd mobileclaw-flutter/packages/mobileclaw_sdk
flutter_rust_bridge_codegen generate
```

---

## 7. Out of Scope

- Per-frame log lines (only summary statistics)
- `adb` calls from within the test process
- iOS support (Android emulator only)
- Continuous background monitoring
- Histogram persistence beyond the single test run
