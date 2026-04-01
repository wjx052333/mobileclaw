# Flutter Layer Architecture

**Status**: Phase 1 — Dart API contract complete, FFI binding (Phase 2) pending  
**Version**: 0.3.0

---

## 1. Layer Overview

```
mobileclaw_app (Flutter Demo App)
        │  pubspec.yaml path dependency
        ▼
mobileclaw_sdk (Flutter Plugin Package)
        │  flutter_rust_bridge 2.x (Phase 2)
        ▼
claw_ffi  (Rust — the single Dart↔Rust boundary)
        │
        ▼
Rust Workspace: mobileclaw-core/crates/
   claw_core / claw_memory / claw_tools / claw_skills /
   claw_llm / claw_storage / claw_scheduler / claw_media
```

---

## 2. Package Layout

```
mobileclaw-flutter/
├── docs/                          ← THIS directory
│   ├── 01-architecture.md
│   ├── 02-performance.md
│   ├── 03-security.md
│   ├── 04-api-contract.md
│   └── 05-test-plan.md
├── packages/
│   └── mobileclaw_sdk/            ← Flutter Plugin (the SDK)
│       ├── lib/
│       │   ├── mobileclaw_sdk.dart  ← unified export
│       │   └── src/
│       │       ├── engine.dart      ← MobileclawAgent abstract interface
│       │       ├── events.dart      ← AgentEvent sealed hierarchy
│       │       ├── exceptions.dart  ← ClawException
│       │       ├── memory.dart      ← MobileclawMemory + MemoryCategory
│       │       ├── models.dart      ← ChatMessage, SkillManifest, etc.
│       │       └── mock.dart        ← MockMobileclawAgent (Phase 1)
│       ├── test/                    ← SDK unit tests
│       └── pubspec.yaml
└── apps/
    └── mobileclaw_app/              ← Demo App (shell)
        ├── lib/
        │   ├── main.dart
        │   ├── core/
        │   │   └── engine_provider.dart  ← Riverpod agentProvider
        │   └── features/
        │       ├── chat/chat_page.dart
        │       ├── tasks/
        │       ├── memory/
        │       ├── skills/
        │       └── settings/
        └── pubspec.yaml
```

---

## 3. Dependency Graph (Dart)

```
mobileclaw_app
  ├── flutter_riverpod   (state management)
  ├── path_provider      (find app support directory)
  └── mobileclaw_sdk
        ├── (Phase 1) MockMobileclawAgent — in-process, no native code
        └── (Phase 2) FFI bridge → claw_ffi → Rust workspace
```

---

## 4. Phase Roadmap

### Phase 1 (current): Dart-only, Mock backend
- `MobileclawAgent` abstract interface defined
- `MockMobileclawAgent` + `MockMobileclawMemory` available
- All UI code builds against the abstract interface
- SDK unit tests run on any host without native toolchain

### Phase 2: FFI Binding
- Create `mobileclaw-core/src/ffi.rs` (`AgentSession` non-generic wrapper)
- Run `flutter_rust_bridge_codegen generate`
- Implement real `MobileclawAgent` delegating to generated bridge
- Replace all `MockMobileclawAgent` usages
- See `docs/04-api-contract.md` §6 for step-by-step checklist

---

## 5. State Management

Riverpod is the sole state management solution:

| Provider | Type | Purpose |
|----------|------|---------|
| `agentProvider` | `FutureProvider<MobileclawAgent>` | Singleton agent lifecycle |
| `chatProvider` (Phase 2) | `StateNotifierProvider` | Message list + streaming state |
| `tasksProvider` (Phase 2) | `StreamProvider` | Background task events |
| `memoryProvider` (Phase 2) | `FutureProvider` | Memory list |
| `skillsProvider` (Phase 2) | `FutureProvider` | Installed skills |

Rule: all `FfiEvent` variants are dispatched through an `event_bus.dart`  
stream that individual providers subscribe to.

---

## 6. Thread Safety

`MobileclawAgent` wraps `AgentLoop` which takes `&mut self` in `chat` on the  
Rust side — **not safe to share across Flutter isolates**.

Rule: create exactly **one** agent instance per isolate. All calls to  
`chat()`, `memory.*`, `skills.*` must be serialized from the same isolate.  
The Riverpod `agentProvider` enforces this by scoping to the root `ProviderScope`.
