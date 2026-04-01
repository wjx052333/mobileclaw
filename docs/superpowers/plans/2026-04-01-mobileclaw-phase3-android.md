# MobileClaw Phase 3: Android Native Support Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a working Android APK backed by the real Rust `mobileclaw-core` via flutter_rust_bridge v2, replacing the mock agent on Android.

**Architecture:** Use `cargo-ndk` to cross-compile `libmobileclaw_core.so` for three Android ABIs (arm64-v8a, armeabi-v7a, x86_64); place the `.so` files in `android/src/main/jniLibs/`; update `build.gradle.kts` to declare the jniLibs source set; update `engine_provider.dart` to initialize the bridge and enable the real agent on Android. Fix the missing bridge-init call that was left out in Phase 2 production code.

**Tech Stack:** `cargo-ndk`, `rustup` Android targets, Android NDK 27 at `~/Android/Sdk/ndk/27.1.12297006`, `flutter_rust_bridge v2`, Android emulator `Medium_Phone_API_36.1`

---

## Environment

- Worktree: `/home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev`
- Rust workspace root (shared with main branch): `/home/wjx/agent_eyes/bot/mobileclaw`
- Flutter SDK: `~/flutter/bin/flutter`
- Cargo: `~/.cargo/bin/cargo`
- Rustup: `~/.cargo/bin/rustup`
- Android NDK: `~/Android/Sdk/ndk/27.1.12297006`
- Android SDK: `~/Android/Sdk`
- Emulator ID: `Medium_Phone_API_36.1`

---

## File Map

| File | Action | Purpose |
|------|--------|---------|
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/models.dart` | modify | Add `operator==` and `hashCode` to `ChatMessage`, `SkillManifest` |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/memory.dart` | modify | Add `operator==` and `hashCode` to `MemoryDoc`, `SearchResult` |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/events.dart` | modify | Add `operator==` and `hashCode` to `TextDeltaEvent`, `ToolCallEvent`, `ToolResultEvent`, `DoneEvent` |
| `mobileclaw-flutter/packages/mobileclaw_sdk/test/mobileclaw_sdk_test.dart` | modify | Add equality tests for model classes |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart` | modify | Call `MobileclawCoreBridge.init()` inside `create()` (idempotent bridge init) |
| `mobileclaw-flutter/apps/mobileclaw_app/lib/core/engine_provider.dart` | modify | Add `Platform.isAndroid` to `_nativeAvailable` |
| `mobileclaw-flutter/packages/mobileclaw_sdk/android/src/main/jniLibs/` | create | Pre-built `.so` for arm64-v8a, armeabi-v7a, x86_64 |
| `mobileclaw-flutter/packages/mobileclaw_sdk/android/build.gradle.kts` | modify | Declare jniLibs source set |

---

## Task 1: Add `operator==` and `hashCode` to Dart model classes

**Files:**
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/models.dart`
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/memory.dart`
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/events.dart`
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/test/mobileclaw_sdk_test.dart`

Dev-standards §8: "All types that cross the Rust–Dart boundary must have Dart `operator ==` and `hashCode` implemented." This was flagged as Important in the Phase 2 code review.

- [ ] **Step 1.1: Write failing tests for equality**

Add a new group to `mobileclaw-flutter/packages/mobileclaw_sdk/test/mobileclaw_sdk_test.dart` (before the closing `}` of the file):

```dart
  group('Model equality', () {
    test('ChatMessage equality', () {
      const a = ChatMessage(role: 'user', content: 'hi');
      const b = ChatMessage(role: 'user', content: 'hi');
      const c = ChatMessage(role: 'assistant', content: 'hi');
      expect(a, equals(b));
      expect(a, isNot(equals(c)));
      expect(a.hashCode, equals(b.hashCode));
    });

    test('SkillManifest equality', () {
      const a = SkillManifest(
        name: 'n', description: 'd',
        trust: SkillTrust.bundled, keywords: ['k'],
      );
      const b = SkillManifest(
        name: 'n', description: 'd',
        trust: SkillTrust.bundled, keywords: ['k'],
      );
      const c = SkillManifest(
        name: 'x', description: 'd',
        trust: SkillTrust.bundled, keywords: ['k'],
      );
      expect(a, equals(b));
      expect(a, isNot(equals(c)));
      expect(a.hashCode, equals(b.hashCode));
    });

    test('MemoryDoc equality', () {
      const a = MemoryDoc(
        id: '1', path: 'p', content: 'c',
        category: MemoryCategory.core, createdAt: 0, updatedAt: 0,
      );
      const b = MemoryDoc(
        id: '1', path: 'p', content: 'c',
        category: MemoryCategory.core, createdAt: 0, updatedAt: 0,
      );
      const c = MemoryDoc(
        id: '2', path: 'p', content: 'c',
        category: MemoryCategory.core, createdAt: 0, updatedAt: 0,
      );
      expect(a, equals(b));
      expect(a, isNot(equals(c)));
      expect(a.hashCode, equals(b.hashCode));
    });

    test('SearchResult equality', () {
      const doc = MemoryDoc(
        id: '1', path: 'p', content: 'c',
        category: MemoryCategory.core, createdAt: 0, updatedAt: 0,
      );
      const a = SearchResult(doc: doc, score: 0.9);
      const b = SearchResult(doc: doc, score: 0.9);
      const c = SearchResult(doc: doc, score: 0.5);
      expect(a, equals(b));
      expect(a, isNot(equals(c)));
      expect(a.hashCode, equals(b.hashCode));
    });

    test('AgentEvent equality', () {
      const a = TextDeltaEvent(text: 'hello');
      const b = TextDeltaEvent(text: 'hello');
      const c = TextDeltaEvent(text: 'world');
      expect(a, equals(b));
      expect(a, isNot(equals(c)));

      const d = ToolCallEvent(toolName: 't');
      const e = ToolCallEvent(toolName: 't');
      expect(d, equals(e));

      const f = ToolResultEvent(toolName: 't', success: true);
      const g = ToolResultEvent(toolName: 't', success: true);
      const h = ToolResultEvent(toolName: 't', success: false);
      expect(f, equals(g));
      expect(f, isNot(equals(h)));

      expect(const DoneEvent(), equals(const DoneEvent()));
    });
  });
```

- [ ] **Step 1.2: Run to verify failures**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk
~/flutter/bin/flutter test test/mobileclaw_sdk_test.dart 2>&1 | tail -20
```

Expected: failures on equality tests (`Expected: <ChatMessage instance> Actual: <ChatMessage instance>`).

- [ ] **Step 1.3: Add `operator==` and `hashCode` to `ChatMessage` and `SkillManifest`**

In `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/models.dart`, update `ChatMessage`:

```dart
class ChatMessage {
  const ChatMessage({required this.role, required this.content});

  final String role;
  final String content;

  @override
  bool operator ==(Object other) =>
      other is ChatMessage && other.role == role && other.content == content;

  @override
  int get hashCode => Object.hash(role, content);
}
```

Update `SkillManifest`:

```dart
class SkillManifest {
  const SkillManifest({
    required this.name,
    required this.description,
    required this.trust,
    required this.keywords,
    this.allowedTools,
  });

  final String name;
  final String description;
  final SkillTrust trust;
  final List<String> keywords;
  final List<String>? allowedTools;

  @override
  bool operator ==(Object other) =>
      other is SkillManifest &&
      other.name == name &&
      other.description == description &&
      other.trust == trust &&
      _listEq(other.keywords, keywords) &&
      _listEq(other.allowedTools, allowedTools);

  @override
  int get hashCode =>
      Object.hash(name, description, trust, Object.hashAll(keywords));
}

bool _listEq<T>(List<T>? a, List<T>? b) {
  if (a == null && b == null) return true;
  if (a == null || b == null) return false;
  if (a.length != b.length) return false;
  for (var i = 0; i < a.length; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}
```

- [ ] **Step 1.4: Add `operator==` and `hashCode` to `MemoryDoc` and `SearchResult`**

In `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/memory.dart`, update `MemoryDoc`:

```dart
class MemoryDoc {
  const MemoryDoc({
    required this.id,
    required this.path,
    required this.content,
    required this.category,
    required this.createdAt,
    required this.updatedAt,
  });

  final String id;
  final String path;
  final String content;
  final MemoryCategory category;
  final int createdAt;
  final int updatedAt;

  DateTime get createdAtDt =>
      DateTime.fromMillisecondsSinceEpoch(createdAt * 1000);
  DateTime get updatedAtDt =>
      DateTime.fromMillisecondsSinceEpoch(updatedAt * 1000);

  @override
  bool operator ==(Object other) =>
      other is MemoryDoc &&
      other.id == id &&
      other.path == path &&
      other.content == content &&
      other.category == category &&
      other.createdAt == createdAt &&
      other.updatedAt == updatedAt;

  @override
  int get hashCode =>
      Object.hash(id, path, content, category, createdAt, updatedAt);
}
```

Update `SearchResult`:

```dart
class SearchResult {
  const SearchResult({required this.doc, required this.score});
  final MemoryDoc doc;
  final double score;

  @override
  bool operator ==(Object other) =>
      other is SearchResult && other.doc == doc && other.score == score;

  @override
  int get hashCode => Object.hash(doc, score);
}
```

- [ ] **Step 1.5: Add `operator==` and `hashCode` to `AgentEvent` subclasses**

In `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/events.dart`, update each class:

```dart
sealed class AgentEvent {
  const AgentEvent();
}

final class TextDeltaEvent extends AgentEvent {
  const TextDeltaEvent({required this.text});
  final String text;

  @override
  bool operator ==(Object other) =>
      other is TextDeltaEvent && other.text == text;

  @override
  int get hashCode => text.hashCode;
}

final class ToolCallEvent extends AgentEvent {
  const ToolCallEvent({required this.toolName});
  final String toolName;

  @override
  bool operator ==(Object other) =>
      other is ToolCallEvent && other.toolName == toolName;

  @override
  int get hashCode => toolName.hashCode;
}

final class ToolResultEvent extends AgentEvent {
  const ToolResultEvent({required this.toolName, required this.success});
  final String toolName;
  final bool success;

  @override
  bool operator ==(Object other) =>
      other is ToolResultEvent &&
      other.toolName == toolName &&
      other.success == success;

  @override
  int get hashCode => Object.hash(toolName, success);
}

final class DoneEvent extends AgentEvent {
  const DoneEvent();

  @override
  bool operator ==(Object other) => other is DoneEvent;

  @override
  int get hashCode => 0;
}
```

- [ ] **Step 1.6: Run tests — must all pass**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk
~/flutter/bin/flutter test 2>&1 | tail -10
```

Expected: all tests pass (≥64 total, 5 new equality tests).

- [ ] **Step 1.7: Analyze SDK package — must be clean**

```bash
~/flutter/bin/flutter analyze 2>&1 | tail -10
```

Expected: `No issues found!` (confirms `_listEq` helper is correctly scoped and no dart lint errors).

- [ ] **Step 1.8: Commit**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev
git add mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/models.dart \
        mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/memory.dart \
        mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/events.dart \
        mobileclaw-flutter/packages/mobileclaw_sdk/test/mobileclaw_sdk_test.dart
git commit -m "feat(sdk): add operator== and hashCode to all cross-FFI Dart types"
```

---

## Task 2: Fix bridge initialization and enable Android in `engine_provider.dart`

**Files:**
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart`
- Modify: `mobileclaw-flutter/apps/mobileclaw_app/lib/core/engine_provider.dart`

**Problem:** `engine_provider.dart` calls `MobileclawAgentImpl.create()` but the Rust bridge (`MobileclawCoreBridge`) is never initialized before FFI calls are made. In production this crashes on the first FFI call.

**Fix:** Move bridge init inside `MobileclawAgentImpl.create()` in the SDK package (same package as the bridge, so no lint issues). The app just calls `create()` — the SDK handles initialization internally. Then update `engine_provider.dart` to also enable Android.

**Why not put init in `engine_provider.dart`:** That would require the app to import `package:mobileclaw_sdk/src/bridge/frb_generated.dart` — a `src/` path, which is an `implementation_imports` lint violation. Bridge internals belong in the SDK.

- [ ] **Step 2.1: Add bridge init inside `MobileclawAgentImpl.create()`**

In `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart`:

1. Add import at the top of the file (after existing imports):

```dart
import 'bridge/frb_generated.dart';
```

2. Inside the `static Future<MobileclawAgentImpl> create({...}) async {` method, add bridge init as the first statement (before `AgentSession.create(...)`):

```dart
  static Future<MobileclawAgentImpl> create({
    required String apiKey,
    required String dbPath,
    required String sandboxDir,
    required List<String> httpAllowlist,
    String model = 'claude-opus-4-6',
    String? skillsDir,
  }) async {
    // Initialize the FFI bridge on first call only.
    // flutter_rust_bridge v2 throws StateError if init() is called twice,
    // so guard with .initialized. This allows the integration tests' setUpAll
    // to call init(externalLibrary: ...) first without conflicting.
    // On Android: loads libmobileclaw_core.so from jniLibs via System.loadLibrary.
    // On Linux:   dlopen("libmobileclaw_core.so") found via bundle RUNPATH.
    if (!MobileclawCoreBridge.instance.initialized) {
      await MobileclawCoreBridge.init();
    }

    final session = await AgentSession.create(
    // ... rest of existing create() body unchanged
```

**The exact edit:** find the line `final session = await AgentSession.create(` in `agent_impl.dart` and insert these lines above it:

```dart
    if (!MobileclawCoreBridge.instance.initialized) {
      await MobileclawCoreBridge.init();
    }

    final session = await AgentSession.create(
```

And add the import `import 'bridge/frb_generated.dart';` at the top of the file.

- [ ] **Step 2.2: Update `engine_provider.dart` to enable Android**

In `mobileclaw-flutter/apps/mobileclaw_app/lib/core/engine_provider.dart`, change:

```dart
bool get _nativeAvailable {
  if (Platform.isLinux) {
    // The .so is bundled alongside the Flutter app binary.
    return true;
  }
  // iOS / Android native support lands in Phase 3.
  return false;
}
```

To:

```dart
/// `true` when the native library is available.
/// Phase 3 adds Android. iOS requires a Mac build — not yet supported.
bool get _nativeAvailable => Platform.isLinux || Platform.isAndroid;
```

No other changes to `engine_provider.dart` are needed (bridge init is now inside `MobileclawAgentImpl.create()`).

- [ ] **Step 2.3: Run SDK tests — all must pass**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk
~/flutter/bin/flutter test 2>&1 | tail -10
```

Expected: all tests pass. The bridge init is idempotent — the `setUpAll` in integration tests calls `init(externalLibrary: ...)` first; `create()` calling `init()` a second time is a no-op.

- [ ] **Step 2.4: Analyze the app — must be clean**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/apps/mobileclaw_app
~/flutter/bin/flutter analyze 2>&1 | tail -10
```

Expected: `No issues found!` (no `implementation_imports` lint, since bridge init is inside the SDK package itself).

- [ ] **Step 2.5: Run widget test — must still pass**

```bash
~/flutter/bin/flutter test 2>&1 | tail -10
```

Expected: `All tests passed!`

- [ ] **Step 2.6: Commit**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev
git add mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart \
        mobileclaw-flutter/apps/mobileclaw_app/lib/core/engine_provider.dart
git commit -m "fix(sdk): initialize Rust bridge inside create(); enable Android in engine_provider"
```

---

## Task 3: Install Android Rust cross-compilation toolchain

**Files:** (no code files — toolchain setup only)

- [ ] **Step 3.1: Install `cargo-ndk`**

```bash
~/.cargo/bin/cargo install cargo-ndk 2>&1 | tail -5
```

Expected: `Installed package 'cargo-ndk'` or `cargo-ndk v3.x.x is already installed`.

Verify:

```bash
~/.cargo/bin/cargo ndk --version
```

Expected: prints version like `cargo-ndk 3.x.x`.

- [ ] **Step 3.2: Add Android Rust targets**

```bash
~/.cargo/bin/rustup target add \
  aarch64-linux-android \
  armv7-linux-androideabi \
  x86_64-linux-android 2>&1 | tail -5
```

Expected: `Downloading component 'rust-std' for ...` then `done`.

Verify all three installed:

```bash
~/.cargo/bin/rustup target list --installed | grep android
```

Expected:
```
aarch64-linux-android
armv7-linux-androideabi
x86_64-linux-android
```

- [ ] **Step 3.3: Verify NDK is present**

```bash
ls ~/Android/Sdk/ndk/27.1.12297006/toolchains/llvm/prebuilt/linux-x86_64/bin/ | grep -E "aarch64.*clang$|armv7a.*clang$|x86_64.*clang$" | head -5
```

Expected: 3 clang compilers visible.

---

## Task 4: Build Android native libraries

**Files:**
- Create: `mobileclaw-flutter/packages/mobileclaw_sdk/android/src/main/jniLibs/arm64-v8a/libmobileclaw_core.so`
- Create: `mobileclaw-flutter/packages/mobileclaw_sdk/android/src/main/jniLibs/armeabi-v7a/libmobileclaw_core.so`
- Create: `mobileclaw-flutter/packages/mobileclaw_sdk/android/src/main/jniLibs/x86_64/libmobileclaw_core.so`

- [ ] **Step 4.1: Create jniLibs directories**

```bash
mkdir -p /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk/android/src/main/jniLibs/{arm64-v8a,armeabi-v7a,x86_64}
```

- [ ] **Step 4.2: Cross-compile for all three ABIs**

Run from the **worktree root** (not the main repo root) so `frb_generated.rs` is found:

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev
ANDROID_NDK_HOME=~/Android/Sdk/ndk/27.1.12297006 \
  ~/.cargo/bin/cargo ndk \
    --target aarch64-linux-android \
    --target armv7-linux-androideabi \
    --target x86_64-linux-android \
    -o mobileclaw-flutter/packages/mobileclaw_sdk/android/src/main/jniLibs \
    build --release -p mobileclaw-core 2>&1 | tail -15
```

Expected: `Finished release [optimized]` with three `.so` files placed automatically by `cargo ndk` into the ABI subdirectories.

- [ ] **Step 4.3: Verify the files are present and have the right symbols**

```bash
ls -lh mobileclaw-flutter/packages/mobileclaw_sdk/android/src/main/jniLibs/*/libmobileclaw_core.so
```

Expected: three files, each 5–15 MB.

```bash
nm -D mobileclaw-flutter/packages/mobileclaw_sdk/android/src/main/jniLibs/arm64-v8a/libmobileclaw_core.so \
  | grep frb_pde_ffi_dispatcher | head -3
```

Expected: at least one line containing `frb_pde_ffi_dispatcher_primary`.

- [ ] **Step 4.4: Commit the native libraries**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev
git add mobileclaw-flutter/packages/mobileclaw_sdk/android/src/main/jniLibs/
git commit -m "build(android): add pre-built native libraries for arm64, armv7, x86_64"
```

---

## Task 5: Update Android `build.gradle.kts`

**Files:**
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/android/build.gradle.kts`

The `jniLibs` source set must be declared so that Gradle packages the `.so` files into the APK's `lib/` directory. Although Gradle 7+ implicitly includes `src/main/jniLibs`, declaring it explicitly documents intent and avoids build-system drift.

- [ ] **Step 5.1: Update the `sourceSets` block in `build.gradle.kts`**

In `mobileclaw-flutter/packages/mobileclaw_sdk/android/build.gradle.kts`, find the existing `sourceSets` block:

```kotlin
    sourceSets {
        getByName("main") {
            java.srcDirs("src/main/kotlin")
        }
        getByName("test") {
            java.srcDirs("src/test/kotlin")
        }
    }
```

Replace with:

```kotlin
    sourceSets {
        getByName("main") {
            java.srcDirs("src/main/kotlin")
            // Pre-built Rust native libraries for FFI bridge.
            jniLibs.srcDirs("src/main/jniLibs")
        }
        getByName("test") {
            java.srcDirs("src/test/kotlin")
        }
    }
```

- [ ] **Step 5.2: Commit**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev
git add mobileclaw-flutter/packages/mobileclaw_sdk/android/build.gradle.kts
git commit -m "build(android): declare jniLibs source set in build.gradle.kts"
```

---

## Task 6: Run on Android emulator

**Files:** (no code changes — run and verify)

This task verifies the full Android stack: bridge loads, `AgentSession.create()` succeeds, `MockMobileclawAgent` fallback is NOT used (real impl is used).

> **Note:** The emulator may take 60–90 seconds to boot. Do not cancel early. If the machine has KVM acceleration (`ls /dev/kvm`), boot is fast; otherwise it may be slow.

- [ ] **Step 6.1: Check KVM availability**

```bash
ls /dev/kvm && echo "KVM available — fast emulation" || echo "No KVM — emulation will be slow"
```

- [ ] **Step 6.2: Launch the emulator in the background**

```bash
~/Android/Sdk/emulator/emulator \
  -avd Medium_Phone_API_36.1 \
  -no-snapshot-load \
  -no-audio \
  -gpu swiftshader_indirect &
```

Wait for it to boot:

```bash
~/Android/Sdk/platform-tools/adb wait-for-device
~/Android/Sdk/platform-tools/adb shell getprop sys.boot_completed
```

Repeat the last command until it returns `1`.

- [ ] **Step 6.3: Verify Flutter sees the emulator**

```bash
~/flutter/bin/flutter devices
```

Expected: `Android SDK built for x86_64 (mobile)` or similar in the list.

- [ ] **Step 6.4: Run `flutter pub get` in the SDK package**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk
~/flutter/bin/flutter pub get 2>&1 | tail -5
```

Expected: `Got dependencies!`

- [ ] **Step 6.5: Build and run the app on the emulator**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/apps/mobileclaw_app
~/flutter/bin/flutter run \
  -d emulator-5554 \
  --dart-define=ANTHROPIC_API_KEY="" 2>&1 | tail -30
```

> Use `-d emulator-5554` — if the device ID differs, check `flutter devices` output and substitute.
> Pass an empty API key for now; the app will launch and show the chat UI backed by the real Rust engine. Actual LLM calls require a valid key.

Expected: app builds, installs, and launches without a crash. The chat UI appears. The Riverpod loading spinner resolves to the chat screen (not an error screen).

If you see `Init error: ...` on screen or a crash in the log, check:
1. `adb logcat | grep -i "flutter\|mobileclaw\|rust"` for the Dart/native error
2. Common issue: `UnsatisfiedLinkError` → the `.so` is not in the APK. Check that `jniLibs` files are committed and the sourceSets block is correct.
3. Common issue: `MobileclawCoreBridge not initialized` → bridge `init()` was not called before `create()`.

- [ ] **Step 6.6: Commit any fixes from step 6.5**

If no fixes were needed: skip this step.

If fixes were needed: commit them with `fix(android): <description>`.

---

## Task 7: Final verification

- [ ] **Step 7.1: Full SDK test suite (mock tests + equality tests)**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk
~/flutter/bin/flutter test 2>&1 | tail -10
```

Expected: ≥69 tests passing (64 from Phase 2 + 5 new equality tests).

- [ ] **Step 7.2: Linux integration tests still pass**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk
INTEGRATION=true ~/flutter/bin/flutter test 2>&1 | tail -5
```

Expected: all pass including 2 integration tests (same as Phase 2).

- [ ] **Step 7.3: App widget test still passes**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/apps/mobileclaw_app
~/flutter/bin/flutter test 2>&1 | tail -5
```

Expected: `All tests passed!`

- [ ] **Step 7.4: Rust test suite still passes**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev
~/.cargo/bin/cargo test -p mobileclaw-core --features test-utils 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 7.5: Final commit (if any stray changes)**

```bash
git status
```

If clean: done. If there are unstaged changes: stage and commit with `chore: Phase 3 Android complete`.

---

## Troubleshooting

### `cargo ndk` build fails with "error: linker `aarch64-linux-android21-clang` not found"

Set `ANDROID_NDK_HOME` explicitly before running:

```bash
export ANDROID_NDK_HOME=~/Android/Sdk/ndk/27.1.12297006
```

### `cargo ndk` produces wrong ABI directory names

`cargo ndk -o <dir>` automatically creates `arm64-v8a/`, `armeabi-v7a/`, `x86_64/` subdirectories. Verify with `ls -la <dir>`.

### `UnsatisfiedLinkError: libmobileclaw_core.so` on Android

The `.so` was not packaged into the APK. Check:
1. `git status` — are the jniLibs committed?
2. The `jniLibs.srcDirs` line is in `build.gradle.kts`
3. Run `flutter clean && flutter run` to force a fresh build

### Emulator hangs or is too slow without KVM

Add `-no-window` to run headless, or use `-accel off` to confirm the issue is GPU not CPU. Consider using a physical Android device if available.

### `MobileclawCoreBridge not initialized` error

`MobileclawAgentImpl.create()` should call `MobileclawCoreBridge.init()` internally (guarded by `!MobileclawCoreBridge.instance.initialized`). Check that:
1. The `import 'bridge/frb_generated.dart';` line is present in `agent_impl.dart`
2. The `if (!MobileclawCoreBridge.instance.initialized) { await MobileclawCoreBridge.init(); }` block is the first thing in `create()`, before `AgentSession.create(...)`

Do **not** put bridge init in `engine_provider.dart` — that would require a `src/` import from the app package, causing a lint error.

### `StateError: Should not initialize flutter_rust_bridge twice`

The `initialized` guard in `create()` is missing or incorrect. Verify the guard is `if (!MobileclawCoreBridge.instance.initialized)` — without this guard, calling `create()` after a test's `setUpAll` init would throw.
