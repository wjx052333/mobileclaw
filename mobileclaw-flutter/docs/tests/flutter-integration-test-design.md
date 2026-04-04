# Flutter Integration Test Design

**Location:** `apps/mobileclaw_app/integration_test/`  
**Runner:** `apps/mobileclaw_app/scripts/run_integration_tests.sh`  
**Scope:** Dart → Rust FFI boundary, real device/emulator, real LLM API

---

## 1. Test Layers

```
┌─────────────────────────────────────────────────────┐
│  Flutter widget tests  (apps/mobileclaw_app/test/)  │  mock MobileclawAgent
├─────────────────────────────────────────────────────┤
│  Core black-box tests  (tests/rustcore-blackbox/)   │  Rust-only, FaultInjectingLlmClient
├─────────────────────────────────────────────────────┤
│  Flutter integration tests  ← THIS DOCUMENT        │  real device, real FFI, real LLM
└─────────────────────────────────────────────────────┘
```

Flutter integration tests exercise the full stack:

```
Dart test code
    → MobileclawAgentImpl  (agent_impl.dart)
    → flutter_rust_bridge bindings  (frb_generated.dart / ffi.dart)
    → libmobileclaw_core.so  (loaded via System.loadLibrary on Android)
    → AgentLoop + LlmClient (Rust)
    → LLM provider API  (real network call, provider stored in secrets.db)
```

They are the only layer that validates:
- Real `flutter_rust_bridge` binding generation is correct
- `AgentEventDto` variants round-trip faithfully through the FFI boundary
- Real provider credentials and real network path work end-to-end
- Camera frame capture → Rust ring buffer → LLM tool use pipeline

---

## 2. Credential Model

### Problem

Flutter integration tests run **inside the app process on the device**. They cannot read
host-machine environment variables at runtime. `Platform.environment` on Android does not
contain any variable set in the shell that launched the test.

### Solution: `--dart-define` + `adb push`

```
Host machine                        Android device
─────────────────                   ──────────────────────────────
MCLAW_SECRET=/path/secrets.db

run_integration_tests.sh
  adb push secrets.db  ──────────→  /data/local/tmp/mclaw_secrets.db
  flutter test
    --dart-define=
      MCLAW_SECRETS_DB_PATH=         String.fromEnvironment(
        /data/local/tmp/…   ──────→    'MCLAW_SECRETS_DB_PATH')
```

`String.fromEnvironment('KEY')` is a **compile-time constant** baked in by the Dart
compiler when `--dart-define=KEY=value` is passed. It survives the host → device boundary.

**There is no `MCLAW_API_KEY` variable.** The API key is stored inside `secrets.db` via
the provider system. Pass `apiKey: null` when constructing `MobileclawAgentImpl`; the
Rust layer reads the active provider's key from `secrets.db` at session creation time.

### Required environment variable

| Variable | Purpose | Where set |
|---|---|---|
| `MCLAW_SECRET` | Path to `secrets.db` **on the host machine** | Host shell before running the script |

The `secrets.db` must contain at least one active LLM provider with a valid API key.

### `secrets.db` isolation

Each test gets an isolated copy of `secrets.db` in a per-test `tmpDir`. The device file
at `/data/local/tmp/mclaw_secrets.db` is never modified by any test.

```dart
setUp(() async {
  tmpDir = Directory.systemTemp.createTempSync('mclaw_test_');
  await File(TestEnv.secretsDbPath).copy('${tmpDir.path}/secrets.db');
  agent = await MobileclawAgentImpl.create(
    apiKey: null,                                    // read from secrets.db
    secretsDbPath: '${tmpDir.path}/secrets.db',     // isolated copy
    dbPath: '${tmpDir.path}/mem.db',
    encryptionKey: devKey,
    sandboxDir: tmpDir.path,
    httpAllowlist: [],
  );
});
```

The **dev encryption key** is `b"mobileclaw-dev-key-32bytes000000"` — the same key used
by the `mclaw` CLI. Phase 3 will replace this with platform keystore integration.

---

## 3. Fail-Fast: `TestEnv`

`integration_test/helpers/test_env.dart` is the single entry point for credential access.
Every test file calls:

```dart
setUpAll(TestEnv.require);
```

If `MCLAW_SECRETS_DB_PATH` is empty (i.e. the test was run without the script),
`TestEnv.require` calls `fail(...)` immediately, printing instructions that point to
`scripts/run_integration_tests.sh`. The entire binary aborts before any test body runs.

---

## 4. Shell Script: `scripts/run_integration_tests.sh`

The script is the single entry point for running integration tests. It handles all
host-side setup so Dart test code stays clean.

### Validation steps (exit on first failure)

1. `MCLAW_SECRET` is set → exit with setup instructions
2. `$MCLAW_SECRET` file exists on disk → exit with path error
3. File starts with SQLite magic bytes → exit with format error
4. `adb` is in PATH → exit with install instructions
5. At least one device/emulator is connected → exit with connect instructions

### Device setup

```bash
adb -s $DEVICE push "$MCLAW_SECRET" /data/local/tmp/mclaw_secrets.db
```

### Camera permission pre-grant

For `camera_real_capture_test.dart`, the CAMERA runtime permission must be granted
before the tests run. On a headless emulator there is no UI to accept the dialog.
The script handles this automatically:

```bash
# Build and install the debug APK first so pm grant has a target
flutter build apk --debug
adb -s $DEVICE install -r build/app/outputs/flutter-apk/app-debug.apk

# Grant CAMERA permission before running tests
adb -s $DEVICE shell pm grant com.mobileclaw.mobileclaw_app android.permission.CAMERA
```

This step is a no-op on targets that don't declare `CAMERA` in their manifest, and
silently succeeds if the permission is already granted.

### Test invocation

```bash
flutter test "$TEST_TARGET" \
    --dart-define="MCLAW_SECRETS_DB_PATH=/data/local/tmp/mclaw_secrets.db"
```

### Usage

```bash
# Run all integration tests
export MCLAW_SECRET=/home/you/mobileclaw/build/secrets.db
bash scripts/run_integration_tests.sh

# Run a single test file
bash scripts/run_integration_tests.sh integration_test/camera_test.dart
bash scripts/run_integration_tests.sh integration_test/camera_real_capture_test.dart
bash scripts/run_integration_tests.sh integration_test/email_account_test.dart
```

---

## 5. Test Files

### `email_account_test.dart`

Tests email credential CRUD through the full Dart → Rust FFI path. Uses a fresh empty
`secrets.db`. No LLM calls.

---

### `camera_test.dart`

14 tests across 3 groups. All passing.

#### Group 1 — Session and credential infrastructure (9 tests)

| Test | What it verifies |
|---|---|
| `agent session creates with real secrets.db` | FFI bridge loads, secrets.db readable, `AgentSession` allocates |
| `real LLM call reaches API and returns events` | Provider credentials valid, network works, event stream returned |
| `secrets.db copy is isolated` | Test writes to copy; device file is never modified |
| `camera is not authorized by default` | `cameraIsAuthorized()` returns false on a fresh session |
| `cameraSetAuthorized toggles flag` | Round-trip true → false through FFI |
| `cameraPushFrame auto-authorizes and returns true` | Synthetic JPEG header push sets `camera_authorized=true` |
| `cameraAlertStream returns empty list in Phase 1` | Phase 1 stub returns `[]` |
| `cameraStartMonitor returns non-empty ID` | Returns a UUID string |
| `cameraStopMonitor returns false in Phase 1` | Phase 1 stub returns false |

#### Group 2 — Unauthorized camera capture (2 tests)

LLM is forced to call `camera_capture` via system prompt while `camera_authorized=false`.

| Test | What it verifies |
|---|---|
| `chat triggers CameraAuthRequired event when camera not authorized` | `CameraAuthRequiredEvent` appears in event stream |
| `CameraAuthRequired event appears before DoneEvent` | Correct event ordering at the FFI boundary |

#### Group 3 — Authorized camera capture (3 tests)

| Test | What it verifies |
|---|---|
| `push frame then chat emits successful ToolResult for camera_capture` | Push synthetic frame → `camera_capture` returns `success=true` |
| `two-turn auth recovery` | Turn 1: unauthorized → `CameraAuthRequired`; push frame; Turn 2: success |

**Key implementation note:** The authorization check in `CameraCapture::execute` runs
**before** the vision-support check. This ensures `CameraUnauthorized` (→ `CameraAuthRequired`
event) fires regardless of whether the current model supports vision.

---

### `camera_real_capture_test.dart`

5 tests. Requires a device/emulator with a working camera (physical or virtual).
CAMERA permission is pre-granted by the script; no manual steps needed.

#### Group — Real camera capture (5 tests)

| Test | What it verifies |
|---|---|
| `device has at least one camera` | AVD virtual camera is configured and accessible |
| `can capture a real frame and encode it as JPEG` | `CameraImage` (YUV420) → JPEG encoding via `image` package; validates FF D8 magic bytes |
| `real JPEG frame pushes successfully via FFI` | Real JPEG bytes → `cameraPushFrame` → Rust ring buffer; `camera_authorized` set to true |
| `push 5 real frames, ring buffer holds them` | 5 consecutive real frames pushed; all return true |
| `LLM camera_capture tool succeeds with real JPEG frame in ring buffer` | Full pipeline: real camera frame → ring buffer → LLM `camera_capture` tool call → `ToolResult{success=true}` |

#### YUV420 → JPEG encoding

```
CameraImage (YUV420, 3 planes)
    Y plane:  full-resolution luminance
    U plane:  half-resolution Cb  (bytesPerPixel stride)
    V plane:  half-resolution Cr  (bytesPerPixel stride)
    ↓  BT.601 YUV → RGB per-pixel
img.Image (RGB)
    ↓  img.encodeJpg(quality: 75)
List<int>  (JPEG bytes, starts with FF D8)
    ↓  cameraPushFrame(jpeg: ..., frameId: ..., width: 320, height: 240)
Rust CameraFrameBuffer (RingBuffer<FrameData>)
```

---

## 6. What Is NOT Tested Here

| What | Why not here | Where |
|---|---|---|
| Widget rendering, navigation | Flutter widget tests | `apps/mobileclaw_app/test/` |
| AgentLoop event pipeline | Rust-only, no device needed | `tests/rustcore-blackbox/` |
| CameraCapture tool internals | Rust unit tests | `mobileclaw-core/src/tools/builtin/camera.rs` |
| Camera auth/event integration | Rust integration tests | `mobileclaw-core/tests/integration_camera*.rs` |
| mmap zero-copy frame path | Phase 2, not yet implemented | — |
| CI secrets provisioning | Requires CI design | Future work |
