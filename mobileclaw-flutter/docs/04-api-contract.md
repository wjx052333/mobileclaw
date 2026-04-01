# Dart API Contract

This document is the authoritative reference for the `mobileclaw_sdk` public  
API. The Phase 2 FFI implementation must match these signatures exactly.

For the full narrative, see `docs/design/05-flutter-interface.md` in the  
repo root.

---

## 1. MobileclawAgent (abstract)

```dart
abstract class MobileclawAgent {
  static Future<MobileclawAgent> create({
    required String apiKey,        // Keystore alias, NOT the raw key
    required String dbPath,
    required String sandboxDir,
    required List<String> httpAllowlist,
    String model = 'claude-opus-4-6',
    String? skillsDir,
  });

  void dispose();

  Stream<AgentEvent> chat(String userInput, {String system = ''});
  Future<String> chatText(String userInput, {String system = ''});
  List<ChatMessage> get history;
  MobileclawMemory get memory;
  Future<void> loadSkillsFromDir(String dirPath);
  List<SkillManifest> get skills;
}
```

---

## 2. AgentEvent (sealed)

```
AgentEvent
  ├── TextDeltaEvent    { text: String }
  ├── ToolCallEvent     { toolName: String }
  ├── ToolResultEvent   { toolName: String, success: bool }
  └── DoneEvent
```

---

## 3. MobileclawMemory (abstract)

```dart
abstract class MobileclawMemory {
  Future<MemoryDoc> store(String path, String content, MemoryCategory category);
  Future<List<SearchResult>> recall(String query, {
    int limit = 10,
    MemoryCategory? category,
    int? since,
    int? until,
  });
  Future<MemoryDoc?> get(String path);
  Future<bool> forget(String path);
  Future<int> count();
}
```

---

## 4. MemoryCategory (sealed)

```dart
sealed class MemoryCategory {
  static const core         // never auto-expired
  static const daily        // pruned after 24 h
  static const conversation // pruned at session end
  factory MemoryCategory.custom(String label)
}
```

Equality is value-based: `MemoryCategory.core == MemoryCategory.core` is `true`.  
`MemoryCategory.custom('x') == MemoryCategory.custom('x')` is `true`.

---

## 5. ClawException

```dart
class ClawException implements Exception {
  final String type;     // Rust variant name
  final String message;

  // Factories
  ClawException.pathTraversal(String path)
  ClawException.urlNotAllowed(String url)
  ClawException.permissionDenied(String reason)
  ClawException.tool(String tool, String message)
  ClawException.llm(String message)
  ClawException.memory(String message)
  ClawException.skillLoad(String message)
}
```

Full `type` → `ClawError` variant mapping:

| type | Rust variant | When |
|------|-------------|------|
| `PathTraversal` | `ClawError::PathTraversal` | File path escapes sandbox |
| `UrlNotAllowed` | `ClawError::UrlNotAllowed` | HTTP to non-allowlisted host |
| `PermissionDenied` | `ClawError::PermissionDenied` | Tool lacks Permission |
| `Tool` | `ClawError::Tool` | Tool execution error |
| `ToolNameConflict` | `ClawError::ToolNameConflict` | Skill tries to override builtin |
| `Memory` | `ClawError::Memory` | SQLite memory store failure |
| `SkillLoad` | `ClawError::SkillLoad` | YAML/Markdown parse failure |
| `Llm` | `ClawError::Llm` | Upstream LLM error |
| `Parse` | `ClawError::Parse` | Tool-call XML parse failure |
| `Sql` | `ClawError::Sql` | Raw SQLite error |
| `Io` | `ClawError::Io` | File I/O error |
| `Json` | `ClawError::Json` | JSON serialisation error |

---

## 6. Phase 2 Integration Checklist

### Rust side
- [ ] `mobileclaw-core/src/ffi.rs` — `AgentSession` non-generic wrapper
- [ ] `AgentConfig`, `AgentEventDto`, `SkillManifestDto` structs
- [ ] Add `flutter_rust_bridge = "2"` to `mobileclaw-core/Cargo.toml`
- [ ] Annotate opaque types with `#[frb(opaque)]`
- [ ] `#[frb(dart_code)]` block mapping `ClawError` → `ClawException`
- [ ] `ClawError` implements `std::fmt::Display` (already via `thiserror`)
- [ ] Run `flutter_rust_bridge_codegen generate` and commit generated files

### Dart side
- [ ] Verify `bridge_generated.dart` exports match this document
- [ ] Implement real `MobileclawAgent` delegating to the bridge
- [ ] Confirm `ClawException.type` matches what the bridge emits
- [ ] Replace all `MockMobileclawAgent` usages
- [ ] Full widget test suite against real FFI (simulator + device)

### iOS
- [ ] `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`
- [ ] `.a` archive links correctly in Xcode
- [ ] No JIT restriction issues (all Rust is AOT-compiled)

### Android
- [ ] `cargo ndk -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs build --release`
- [ ] `.so` files loaded via Gradle `jniLibs`
- [ ] Tested on physical ARM64 device

### Integration
- [ ] End-to-end: `create()` → `chat()` → stream events match expected sequence
- [ ] Error path: trigger `ClawException.pathTraversal` from Dart, verify `type`
- [ ] Memory round-trip: `store()` → `recall()` → score > 0
