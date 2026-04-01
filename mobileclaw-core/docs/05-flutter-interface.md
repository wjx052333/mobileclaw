# 05 — Flutter Interface Contract

**Status:** Phase 1 complete (Rust core), Phase 2 pending (Flutter FFI binding)
**Date:** 2026-04-01
**Audience:** Flutter/Dart engineers building the mobile UI layer

---

## 1. Overview

### What `mobileclaw-core` provides

`mobileclaw-core` is a Rust library that implements the full agent loop for MobileClaw. It exposes four major subsystems:

- **Agent Loop** (`AgentLoop<L>`) — drives multi-turn conversation, streams LLM output, dispatches tool calls, enforces a maximum of 10 tool rounds per user turn.
- **Memory** (`SqliteMemory`) — stores and full-text-searches documents in a local SQLite database (WAL mode, FTS5 trigram tokenizer). Documents are categorised as `core`, `daily`, `conversation`, or `custom`.
- **Tool System** (`ToolRegistry`, `ToolContext`) — a registry of sandboxed tools (file I/O, HTTP fetch, memory read/write, etc.) governed by a `PermissionChecker` and a path/URL sandbox.
- **Skill System** (`SkillManager`, `SkillManifest`) — loads YAML-manifest + Markdown-prompt skill bundles from disk. Skills are activated by keyword matching and inject additional context into the system prompt.

### Why flutter_rust_bridge 2.x

The Flutter binding layer uses [flutter_rust_bridge 2.x](https://cjycode.com/flutter_rust_bridge/) for these reasons:

- **Zero-copy where possible** — large byte buffers can cross the FFI boundary without heap allocation.
- **Async / Stream support** — Rust `async fn` generates a Dart `Future<T>`; Rust `Stream<Item = T>` generates a Dart `Stream<T>`. This maps directly onto the agent's streaming chat output.
- **Type-safe codegen** — `flutter_rust_bridge_codegen generate` reads annotated Rust source and emits strongly-typed Dart stubs, eliminating handwritten `ffi.dart` boilerplate.
- **Opaque handle model** — Rust objects that cannot be `Send + Clone` across the FFI boundary (e.g. `AgentLoop`) are exposed as opaque Dart objects backed by a Rust `Arc`.

### How to use this document

This document defines the **Dart API contract** that Flutter developers code against. During Phase 2 development Flutter engineers should:

1. Implement UI and state management against the abstract interfaces and data classes defined here.
2. Use the `MockMobileclawAgent` (section 5) as a stand-in.
3. Once Phase 2 delivers the real FFI binding, swap out the mock for the generated implementation — no UI code should need to change.

---

## 2. Type Mapping (Rust → Dart)

| Rust Type | Dart Type | Notes |
|---|---|---|
| `String` | `String` | UTF-8 on both sides |
| `&str` | `String` | bridge copies into Dart heap |
| `Vec<T>` | `List<T>` | |
| `Option<T>` | `T?` | `None` → `null` |
| `Result<T, ClawError>` | throws `ClawException` | flutter_rust_bridge maps `Err(e)` to a thrown Dart exception |
| `Arc<T>` / opaque struct | opaque handle (Dart class) | lifetime managed by Rust; Dart holds a pointer token |
| `u64` | `int` | Dart `int` is 64-bit on all platforms |
| `usize` | `int` | same |
| `u32` | `int` | |
| `f32` | `double` | Dart `double` is always 64-bit; precision loss is negligible for scores |
| `bool` | `bool` | |
| `serde_json::Value` | `Map<String, dynamic>` | JSON object; arrays become `List<dynamic>` |
| `Stream<AgentEvent>` | `Stream<AgentEvent>` | Dart async stream; backpressure managed by bridge |
| `Path` / `PathBuf` | `String` | passed as string path |
| `MemoryCategory::Custom(String)` | `MemoryCategory.custom(String label)` | see section 3.3 |

---

## 3. Core API (Dart Interface Contract)

The types and signatures below are the **authoritative contract**. The Phase 2 FFI implementation must match these exactly. Flutter developers may start coding against them immediately using the mock in section 5.

### 3.1 Initialization

```dart
import 'package:mobileclaw/mobileclaw.dart';

/// Top-level agent handle. Wraps the Rust AgentSession (Phase 2).
/// Opaque: do not store internal fields; use method calls only.
abstract class MobileclawAgent {
  /// Create and initialise an agent.
  ///
  /// - [apiKey]          Anthropic API key.
  /// - [dbPath]          Absolute path to the SQLite database file.
  ///                     The file is created if it does not exist.
  /// - [sandboxDir]      Root directory for file-system tools.
  ///                     All file operations are confined to this tree.
  /// - [httpAllowlist]   URL prefixes the HTTP tool is allowed to fetch.
  ///                     Example: ['https://api.example.com/'].
  /// - [model]           LLM model identifier (default: 'claude-opus-4-6').
  /// - [skillsDir]       Optional path to a directory of skill bundles.
  ///                     Each bundle is a sub-directory containing
  ///                     skill.yaml + skill.md.
  static Future<MobileclawAgent> create({
    required String apiKey,
    required String dbPath,
    required String sandboxDir,
    required List<String> httpAllowlist,
    String model = 'claude-opus-4-6',
    String? skillsDir,
  });

  /// Release all Rust-side resources (closes SQLite, drops Arc handles).
  /// The object must not be used after this call.
  void dispose();
}
```

> **Thread safety**: `MobileclawAgent` wraps `AgentLoop` which takes `&mut self` in `chat` — it is not safe to share across Flutter isolates. Create one instance per isolate, or serialize all calls to a single instance from a dedicated isolate.

> **Implementation note:** In Phase 2 this will call the generated
> `AgentSession.create(config: AgentConfig(...))` Rust FFI entry point.
> `SqliteMemory.open(dbPath)` is called on the Rust side; no SQLite
> driver is needed in the Flutter layer.

---

### 3.2 Chat (Agent Loop)

#### 3.2.1 Event types

The Rust core emits `AgentEvent` variants as the agent runs. These map to the following sealed Dart class hierarchy:

```dart
/// Sealed base class for all events emitted during a chat turn.
sealed class AgentEvent {}

/// A fragment of assistant text is available to display.
/// Multiple TextDeltaEvent instances are emitted during a single turn;
/// concatenate [text] fields in order to build the full response.
final class TextDeltaEvent extends AgentEvent {
  const TextDeltaEvent({required this.text});
  final String text;
}

/// The agent is about to execute a tool.
/// Display a progress indicator: "Running ${event.toolName}…"
final class ToolCallEvent extends AgentEvent {
  const ToolCallEvent({required this.toolName});
  final String toolName;
}

/// A tool execution has completed.
final class ToolResultEvent extends AgentEvent {
  const ToolResultEvent({required this.toolName, required this.success});
  final String toolName;

  /// true  → tool returned a result (output may still indicate a domain error)
  /// false → tool threw an exception or the Rust side caught an error
  final bool success;
}

/// The turn is complete. No further events will be emitted on this stream.
final class DoneEvent extends AgentEvent {
  const DoneEvent();
}
```

These correspond exactly to the Rust enum:

```rust
pub enum AgentEvent {
    TextDelta { text: String },
    ToolCall  { name: String },
    ToolResult { name: String, success: bool },
    Done,
}
```

#### 3.2.2 Chat methods

Add these to `MobileclawAgent`:

```dart
abstract class MobileclawAgent {
  // ... (create / dispose from 3.1)

  /// Stream all events for one user turn.
  ///
  /// The stream completes when [DoneEvent] is emitted or when an error
  /// is thrown as [ClawException].
  ///
  /// [userInput]  The user message text.
  /// [system]     Optional additional system prompt text prepended to the
  ///              base system prompt. Skills may also augment this.
  Stream<AgentEvent> chat(String userInput, {String system = ''});

  /// Convenience wrapper: runs [chat] and collects all [TextDeltaEvent]
  /// fragments into a single string. Throws [ClawException] on error.
  Future<String> chatText(String userInput, {String system = ''});

  /// The full conversation history for the current session.
  /// Each [ChatMessage] represents one user or assistant turn.
  List<ChatMessage> get history;
}

/// A single turn in the conversation history.
class ChatMessage {
  const ChatMessage({required this.role, required this.content});

  /// 'user' or 'assistant'
  final String role;
  final String content;
}
```

> **Note:** The Dart `Stream<AgentEvent>` is produced by `AgentSession.chat_stream` in the FFI wrapper (see Phase 2 Integration Notes). The underlying Rust `AgentLoop::chat` returns `Vec<AgentEvent>` — the wrapper converts this to a Dart stream.

> **Mapping note:** Rust `Message` has `role: Role` (enum) and
> `content: Vec<ContentBlock>`. The FFI layer serialises
> `text_content()` to a single `String` and maps `Role` to the string
> literals `"user"` / `"assistant"`.

---

### 3.3 Memory

#### MemoryCategory

`MemoryCategory` in Rust is an enum with a `Custom(String)` variant. Dart represents this as a sealed class rather than a plain enum so the `custom` case can carry a label:

```dart
sealed class MemoryCategory {
  const MemoryCategory();

  /// Persistent facts about the user or application. Never auto-expired.
  static const core = _NamedCategory('core');

  /// Notes generated today; may be summarised or pruned after 24 h.
  static const daily = _NamedCategory('daily');

  /// Transient notes from the current session.
  static const conversation = _NamedCategory('conversation');

  /// User-defined category with an arbitrary [label].
  const factory MemoryCategory.custom(String label) = _CustomCategory;
}

final class _NamedCategory extends MemoryCategory {
  const _NamedCategory(this._name);
  final String _name;
  @override String toString() => _name;

  @override
  bool operator ==(Object other) =>
      other is _NamedCategory && other._name == _name;

  @override
  int get hashCode => _name.hashCode;
}

final class _CustomCategory extends MemoryCategory {
  const _CustomCategory(this.label);
  final String label;
  @override String toString() => 'custom:$label';

  @override
  bool operator ==(Object other) =>
      other is _CustomCategory && other.label == label;

  @override
  int get hashCode => label.hashCode;
}
```

#### MemoryDoc

Mirrors the Rust `MemoryDoc` struct:

```dart
/// A stored memory document.
class MemoryDoc {
  const MemoryDoc({
    required this.id,
    required this.path,
    required this.content,
    required this.category,
    required this.createdAt,
    required this.updatedAt,
  });

  /// Opaque hex ID generated by the Rust side.
  final String id;

  /// Logical path used as the unique key, e.g. 'notes/profile.md'.
  final String path;

  final String content;
  final MemoryCategory category;

  /// Seconds since Unix epoch (Rust u64 → Dart int).
  final int createdAt;
  final int updatedAt;

  /// Convenience: convert Unix timestamp to Dart DateTime.
  DateTime get createdAtDt => DateTime.fromMillisecondsSinceEpoch(createdAt * 1000);
  DateTime get updatedAtDt => DateTime.fromMillisecondsSinceEpoch(updatedAt * 1000);
}
```

#### SearchResult

```dart
/// A memory document returned by a search query, with a relevance score.
class SearchResult {
  const SearchResult({required this.doc, required this.score});

  final MemoryDoc doc;

  /// FTS5 rank converted to a positive score. Higher is more relevant.
  /// Rust type: f32 → Dart double.
  final double score;
}
```

#### MobileclawMemory interface

```dart
/// Memory subsystem accessed through [MobileclawAgent.memory].
///
/// In MVP Phase 2, [memory] is accessed exclusively through the
/// [MobileclawAgent] handle. A standalone [MobileclawMemory] handle
/// may be exposed in a future phase.
abstract class MobileclawMemory {
  /// Store or overwrite a document at [path].
  ///
  /// If a document at [path] already exists it is updated in place;
  /// the [id] remains stable, [updatedAt] is refreshed.
  /// Throws [ClawException] on SQLite error.
  Future<MemoryDoc> store(
    String path,
    String content,
    MemoryCategory category,
  );

  /// Full-text search using SQLite FTS5 trigram index.
  ///
  /// [query]    Search text. Trigram matching; no special operators needed.
  /// [limit]    Maximum results to return (default 10, same as Rust default).
  /// [category] If provided, restricts results to that category.
  /// [since]    Unix timestamp lower bound on [MemoryDoc.createdAt].
  /// [until]    Unix timestamp upper bound on [MemoryDoc.createdAt].
  Future<List<SearchResult>> recall(
    String query, {
    int limit = 10,
    MemoryCategory? category,
    int? since,
    int? until,
  });

  /// Retrieve a document by exact [path]. Returns null if not found.
  Future<MemoryDoc?> get(String path);

  /// Delete the document at [path].
  /// Returns true if a document was deleted, false if none existed.
  Future<bool> forget(String path);

  /// Total number of documents in the store.
  Future<int> count();
}
```

Add a `memory` getter to `MobileclawAgent`:

```dart
abstract class MobileclawAgent {
  // ...
  MobileclawMemory get memory;
}
```

---

### 3.4 Skills

```dart
/// Trust level of a loaded skill bundle.
///
/// Mirrors Rust SkillTrust enum (serde: "bundled" | "installed").
enum SkillTrust {
  /// Shipped with the app binary. Granted full tool access by default.
  bundled,

  /// Downloaded by the user at runtime. Restricted to [allowedTools].
  installed,
}

/// Metadata loaded from a skill's skill.yaml manifest.
///
/// Mirrors Rust SkillManifest struct.
///
/// Note: [keywords] is flattened from Rust's `manifest.activation.keywords`
/// (the `SkillActivation` struct). The `AgentSession` DTO must flatten this
/// in `src/ffi.rs`.
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

  /// Activation keywords. If any keyword appears in the user input
  /// (case-insensitive substring match), the skill's prompt is injected.
  final List<String> keywords;

  /// If non-null (only for [SkillTrust.installed]), the skill may only
  /// invoke tools whose names appear in this list.
  final List<String>? allowedTools;
}
```

Add skill management methods to `MobileclawAgent`:

```dart
abstract class MobileclawAgent {
  // ...

  /// Load all skill bundles found under [dirPath].
  ///
  /// Each sub-directory must contain:
  ///   skill.yaml  — YAML-serialised SkillManifest
  ///   skill.md    — Markdown prompt injected as system context
  ///
  /// Already-loaded skills with the same [name] are replaced.
  /// Throws [ClawException.skillLoad] if parsing fails.
  Future<void> loadSkillsFromDir(String dirPath);

  /// The manifests of all currently loaded skills, in load order.
  List<SkillManifest> get skills;
}
```

---

### 3.5 Error Handling

All Rust `ClawError` variants are surfaced as `ClawException`. The `type` field matches the Rust variant name so UI code can branch on it.

```dart
/// Exception thrown when the Rust core returns an Err(ClawError).
///
/// flutter_rust_bridge maps Result::Err to a thrown Dart exception.
/// Catch with:
///   try {
///     await agent.chat('hello');
///   } on ClawException catch (e) {
///     log('Claw error [${e.type}]: ${e.message}');
///   }
class ClawException implements Exception {
  const ClawException({required this.type, required this.message});

  /// Rust variant name, e.g. 'PathTraversal', 'UrlNotAllowed',
  /// 'PermissionDenied', 'Tool', 'Memory', 'ToolNameConflict',
  /// 'SkillLoad', 'Llm', 'Parse', 'Sql', 'Io', 'Json'.
  final String type;

  /// Human-readable description from the Rust Display impl.
  final String message;

  // --- Convenience factories matching ClawError variants ---

  /// ClawError::PathTraversal(path)
  factory ClawException.pathTraversal(String path) => ClawException(
    type: 'PathTraversal',
    message: "path traversal attempt: '$path'",
  );

  /// ClawError::UrlNotAllowed(url)
  factory ClawException.urlNotAllowed(String url) => ClawException(
    type: 'UrlNotAllowed',
    message: "url not in allowlist: '$url'",
  );

  /// ClawError::PermissionDenied(reason)
  factory ClawException.permissionDenied(String reason) => ClawException(
    type: 'PermissionDenied',
    message: 'permission denied: $reason',
  );

  /// ClawError::Tool { tool, message }
  factory ClawException.tool(String tool, String message) => ClawException(
    type: 'Tool',
    message: 'tool error: $tool — $message',
  );

  /// ClawError::Llm(message)
  factory ClawException.llm(String message) => ClawException(
    type: 'Llm',
    message: 'llm error: $message',
  );

  /// ClawError::Memory(message)
  factory ClawException.memory(String message) => ClawException(
    type: 'Memory',
    message: 'memory error: $message',
  );

  /// ClawError::SkillLoad(message)
  factory ClawException.skillLoad(String message) => ClawException(
    type: 'SkillLoad',
    message: 'skill load error: $message',
  );

  @override
  String toString() => 'ClawException($type): $message';
}
```

Full mapping of Rust `ClawError` variants to `ClawException.type` values:

| Rust variant | `type` string | When thrown |
|---|---|---|
| `Memory(String)` | `"Memory"` | SQLite memory store failure |
| `Tool { tool, message }` | `"Tool"` | Tool execution error |
| `ToolNameConflict(String)` | `"ToolNameConflict"` | Skill tries to register a built-in tool name |
| `PermissionDenied(String)` | `"PermissionDenied"` | Tool lacks required `Permission` |
| `PathTraversal(String)` | `"PathTraversal"` | File path escapes sandbox |
| `UrlNotAllowed(String)` | `"UrlNotAllowed"` | HTTP fetch to non-allowlisted host |
| `SkillLoad(String)` | `"SkillLoad"` | YAML/Markdown parse failure |
| `Llm(String)` | `"Llm"` | Upstream LLM error |
| `Parse(String)` | `"Parse"` | Tool-call XML parse failure |
| `Sql(rusqlite::Error)` | `"Sql"` | Raw SQLite error |
| `Io(std::io::Error)` | `"Io"` | File I/O error |
| `Json(serde_json::Error)` | `"Json"` | JSON serialisation error |

---

## 3.6 Email Account Management

Email credentials are configured once by the user and stored encrypted in Rust. Dart never retrieves the password after saving.

### FFI Methods

```dart
// Save account (call once from settings screen)
await agent.emailAccountSave(
  dto: EmailAccountDto(
    id: 'work',
    smtpHost: 'smtp.gmail.com',
    smtpPort: 587,
    imapHost: 'imap.gmail.com',
    imapPort: 993,
    username: 'alice@gmail.com',
  ),
  password: _passwordController.text,  // plaintext, used once
);

// Load config for display (password NOT returned)
final EmailAccountDto? config = await agent.emailAccountLoad(id: 'work');

// Remove account
await agent.emailAccountDelete(id: 'work');
```

### Security Contract

- `emailAccountSave`: password is encrypted with AES-256-GCM before storage. The plaintext is never written to disk, logs, or memory beyond the immediate encryption call.
- `emailAccountLoad`: returns config fields only. There is no `emailAccountGetPassword` method. This is intentional and permanent.
- `emailAccountDelete`: removes both config and encrypted password atomically.

### Dart DTO

```dart
class EmailAccountDto {
  final String id;
  final String smtpHost;
  final int smtpPort;
  final String imapHost;
  final int imapPort;
  final String username;
  // No password field — by design
}
```

### Flutter Settings UI Pattern

```dart
// EmailSettingsScreen calls emailAccountSave with password from a
// SecureTextField (obscured, not cached in widget state after submission).
// After save, clear the password controller immediately:
await agent.emailAccountSave(dto: dto, password: _pwCtrl.text);
_pwCtrl.clear();
```

---

## 4. Streaming Chat Usage Example

### 4.1 StreamBuilder widget

```dart
class ChatScreen extends StatefulWidget {
  const ChatScreen({super.key, required this.agent});
  final MobileclawAgent agent;

  @override
  State<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends State<ChatScreen> {
  final _controller = TextEditingController();
  Stream<AgentEvent>? _stream;
  final _buffer = StringBuffer();
  String _displayText = '';

  void _send() {
    final input = _controller.text.trim();
    if (input.isEmpty) return;
    _controller.clear();
    _buffer.clear();
    setState(() {
      _displayText = '';
      _stream = widget.agent.chat(input);
    });
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Expanded(
          child: StreamBuilder<AgentEvent>(
            stream: _stream,
            builder: (context, snapshot) {
              if (snapshot.hasError) {
                final e = snapshot.error;
                if (e is ClawException) {
                  return Text('Error [${e.type}]: ${e.message}',
                      style: const TextStyle(color: Colors.red));
                }
                return Text('Unexpected error: $e');
              }

              if (snapshot.hasData) {
                final event = snapshot.data!;
                switch (event) {
                  case TextDeltaEvent(:final text):
                    _buffer.write(text);
                    // Use post-frame callback to avoid setState during build.
                    WidgetsBinding.instance.addPostFrameCallback((_) {
                      if (mounted) {
                        setState(() => _displayText = _buffer.toString());
                      }
                    });
                  case ToolCallEvent(:final toolName):
                    // Optionally surface tool activity to the user.
                    debugPrint('Running tool: $toolName');
                  case ToolResultEvent(:final toolName, :final success):
                    debugPrint('Tool $toolName finished (success=$success)');
                  case DoneEvent():
                    // Stream complete. Widget will stop rebuilding.
                    break;
                }
              }

              return SingleChildScrollView(
                child: Text(_displayText),
              );
            },
          ),
        ),
        Row(
          children: [
            Expanded(child: TextField(controller: _controller)),
            IconButton(icon: const Icon(Icons.send), onPressed: _send),
          ],
        ),
      ],
    );
  }
}
```

### 4.2 Simple future-based chat (non-streaming)

For cases where streaming UI is not needed:

```dart
Future<void> _sendSimple(MobileclawAgent agent, String input) async {
  try {
    final reply = await agent.chatText(input);
    setState(() => _messages.add(ChatMessage(role: 'assistant', content: reply)));
  } on ClawException catch (e) {
    _showError('${e.type}: ${e.message}');
  }
}
```

### 4.3 Memory usage example

```dart
Future<void> _rememberFact(MobileclawAgent agent) async {
  final doc = await agent.memory.store(
    'notes/user-profile.md',
    '# User Profile\n\nPrefers dark mode. Located in Berlin.',
    MemoryCategory.core,
  );
  debugPrint('Stored: ${doc.id} at ${doc.createdAtDt}');

  final results = await agent.memory.recall(
    'dark mode',
    limit: 5,
    category: MemoryCategory.core,
  );
  for (final r in results) {
    debugPrint('  [${r.score.toStringAsFixed(2)}] ${r.doc.path}');
  }
}
```

---

## 5. Mock Implementation for Flutter Development

Use this during Phase 1 while the Rust FFI binding is not yet available. Replace with the real implementation when Phase 2 is complete.

```dart
import 'dart:async';

/// Mock implementation of [MobileclawAgent].
/// All responses are synthetic; no Rust code is invoked.
class MockMobileclawAgent implements MobileclawAgent {
  MockMobileclawAgent({this.responseDelay = const Duration(milliseconds: 30)});

  final Duration responseDelay;
  final List<ChatMessage> _history = [];
  final _memory = MockMobileclawMemory();

  static Future<MobileclawAgent> create({
    required String apiKey,
    required String dbPath,
    required String sandboxDir,
    required List<String> httpAllowlist,
    String model = 'claude-opus-4-6',
    String? skillsDir,
  }) async =>
      MockMobileclawAgent();

  @override
  void dispose() {}

  @override
  Stream<AgentEvent> chat(String userInput, {String system = ''}) async* {
    _history.add(ChatMessage(role: 'user', content: userInput));

    // Simulate a tool call for inputs containing 'file' or 'search'.
    final lower = userInput.toLowerCase();
    if (lower.contains('file')) {
      yield const ToolCallEvent(toolName: 'read_file');
      await Future.delayed(responseDelay);
      yield const ToolResultEvent(toolName: 'read_file', success: true);
    } else if (lower.contains('search') || lower.contains('remember')) {
      yield const ToolCallEvent(toolName: 'memory_search');
      await Future.delayed(responseDelay);
      yield const ToolResultEvent(toolName: 'memory_search', success: true);
    }

    final response = 'Mock response to: "$userInput"';
    for (final word in response.split(' ')) {
      yield TextDeltaEvent(text: '$word ');
      await Future.delayed(responseDelay);
    }

    yield const DoneEvent();
    _history.add(ChatMessage(role: 'assistant', content: response));
  }

  @override
  Future<String> chatText(String userInput, {String system = ''}) async {
    final buffer = StringBuffer();
    await for (final event in chat(userInput, system: system)) {
      if (event is TextDeltaEvent) buffer.write(event.text);
    }
    return buffer.toString();
  }

  @override
  List<ChatMessage> get history => List.unmodifiable(_history);

  @override
  MobileclawMemory get memory => _memory;

  final List<SkillManifest> _skills = [];

  @override
  Future<void> loadSkillsFromDir(String dirPath) async {
    // No-op in mock: add synthetic skills here if needed for UI testing.
  }

  @override
  List<SkillManifest> get skills => List.unmodifiable(_skills);
}

/// In-memory mock of [MobileclawMemory].
class MockMobileclawMemory implements MobileclawMemory {
  final Map<String, MemoryDoc> _store = {};

  @override
  Future<MemoryDoc> store(
    String path,
    String content,
    MemoryCategory category,
  ) async {
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final existing = _store[path];
    final doc = MemoryDoc(
      id: existing?.id ?? path.hashCode.toRadixString(16),
      path: path,
      content: content,
      category: category,
      createdAt: existing?.createdAt ?? now,
      updatedAt: now,
    );
    _store[path] = doc;
    return doc;
  }

  @override
  Future<List<SearchResult>> recall(
    String query, {
    int limit = 10,
    MemoryCategory? category,
    int? since,
    int? until,
  }) async {
    final q = query.toLowerCase();
    return _store.values
        .where((d) {
          if (category != null && d.category != category) return false;
          if (since != null && d.createdAt < since) return false;
          if (until != null && d.createdAt > until) return false;
          return d.content.toLowerCase().contains(q) ||
              d.path.toLowerCase().contains(q);
        })
        .map((d) => SearchResult(doc: d, score: 1.0))
        .take(limit)
        .toList();
  }

  @override
  Future<MemoryDoc?> get(String path) async => _store[path];

  @override
  Future<bool> forget(String path) async => _store.remove(path) != null;

  @override
  Future<int> count() async => _store.length;
}
```

---

## 6. flutter_rust_bridge 2.x Integration Notes

This section is for the Phase 2 engineer who will wire up the FFI binding.

### 6.1 Why `AgentLoop<L>` needs a concrete wrapper

`AgentLoop` is generic over `L: LlmClient`. Generics cannot cross the FFI boundary directly because:

- The C ABI has no concept of type parameters.
- flutter_rust_bridge codegen must emit a concrete Dart class per Rust type; it cannot monomorphise generic parameters at codegen time.

The solution is a thin, non-generic wrapper `AgentSession` that fixes `L = ClaudeClient`:

```rust
// mobileclaw-core/src/ffi.rs  (Phase 2 — to be created)

use crate::{
    agent::loop_impl::{AgentLoop, AgentEvent as AgentEventCore},
    llm::claude::ClaudeClient,
    memory::sqlite::SqliteMemory,
    tools::{ToolContext, ToolRegistry},
    skill::SkillManager,
    ClawResult,
};
use std::sync::Arc;

/// Non-generic wrapper around AgentLoop<ClaudeClient>.
/// This is the type exposed via flutter_rust_bridge.
pub struct AgentSession {
    inner: AgentLoop<ClaudeClient>,
}

/// Config passed from Dart at construction time.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AgentConfig {
    pub api_key: String,
    pub db_path: String,
    pub sandbox_dir: String,
    pub http_allowlist: Vec<String>,
    pub model: String,
    pub skills_dir: Option<String>,
}

/// DTO emitted by the streaming chat endpoint.
/// All fields are concrete types that flutter_rust_bridge can codegen.
#[derive(serde::Serialize, serde::Deserialize)]
pub enum AgentEventDto {
    TextDelta { text: String },
    ToolCall  { name: String },
    ToolResult { name: String, success: bool },
    Done,
}

impl AgentSession {
    pub async fn create(config: AgentConfig) -> ClawResult<AgentSession> { todo!() }

    /// Collect all events for one turn (non-streaming convenience method).
    pub async fn chat(
        &mut self,
        input: String,
        system: String,
    ) -> ClawResult<Vec<AgentEventDto>> { todo!() }

    /// Streaming version — preferred for real-time UI updates.
    pub fn chat_stream(
        &mut self,
        input: String,
        system: String,
    ) -> impl futures::Stream<Item = ClawResult<AgentEventDto>> + '_ { todo!() }
}
```

### 6.2 Annotations to add to the Rust source

```rust
// On the opaque types — prevents bridge from trying to Clone them:
#[flutter_rust_bridge::frb(opaque)]
pub struct AgentSession { ... }

// On the config DTO — generates a Dart class with named fields:
#[flutter_rust_bridge::frb(dart_metadata = "immutable")]
pub struct AgentConfig { ... }

// Error mapping — emits a Dart ClawException class:
// Add to src/error.rs or ffi.rs:
#[flutter_rust_bridge::frb(dart_code = "
  class ClawException implements Exception {
    final String type;
    final String message;
    ClawException({required this.type, required this.message});
    @override String toString() => 'ClawException(\$type): \$message';
  }
")]
pub enum ClawError { ... }
```

### 6.3 Codegen workflow

```bash
# Install codegen tool (run once):
cargo install flutter_rust_bridge_codegen

# Run from the Flutter project root (adjust paths as needed):
flutter_rust_bridge_codegen generate \
  --rust-input  mobileclaw-core/src/ffi.rs \
  --dart-output lib/src/bridge_generated.dart
# Note: frb v2 generates C headers automatically alongside Dart bindings.
# For custom output paths, use a flutter_rust_bridge.yaml config file.

# Regenerate every time ffi.rs changes.
```

### 6.4 Key types to expose via FFI

| Rust type | FFI strategy |
|---|---|
| `AgentSession` | Opaque (`#[frb(opaque)]`), constructed via `create()` |
| `AgentConfig` | Plain struct (all fields are primitive / `String` / `Vec`) |
| `AgentEventDto` | Enum DTO, fully concrete |
| `SqliteMemory` | Wrapped inside `AgentSession`; not exposed directly |
| `ToolRegistry` | Internal to `AgentSession`; not exposed |
| `SkillManifest` | Plain struct, safe to expose as DTO |
| `ClawError` | Mapped to `ClawException` via `#[frb(dart_code)]` |

### 6.5 Platform-specific notes

**iOS**

- Apple prohibits JIT compilation in App Store apps. All Rust code runs as AOT-compiled native code, which is fine. No WASM, no interpreter.
- Link the `.a` static library via Xcode's "Link Binary With Libraries" phase, or via a CocoaPods `vendored_frameworks` entry.
- Verify the `rust-std` target: `aarch64-apple-ios` for device, `aarch64-apple-ios-sim` for M-series simulator, `x86_64-apple-ios` for Intel simulator.

**Android**

- Build `.so` shared libraries with [`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk):
  ```bash
  cargo ndk -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs build --release
  ```
- Load in Gradle via `System.loadLibrary("mobileclaw_core")` (or the bridge init call handles this automatically when using flutter_rust_bridge's `RustLib.init()`).

---

## 7. Phase 2 Checklist

The following tasks must be completed before the mock can be removed.

### Rust side

- [x] Create `mobileclaw-core/src/ffi.rs` with the non-generic `AgentSession` wrapper
- [x] Add `AgentConfig`, `AgentEventDto`, and `SkillManifestDto` structs to `ffi.rs`
- [x] Add `flutter_rust_bridge = "=2.12.0"` to `mobileclaw-core/Cargo.toml`
- [x] Annotate opaque types with `#[frb(opaque)]`
- [ ] Add `#[frb(dart_code)]` block to map `ClawError` → `ClawException`
- [x] Ensure `ClawError` implements `std::fmt::Display` (via `thiserror`)
- [x] Commit `frb_generated.rs` (manually maintained — FRB codegen not run in CI)
- [x] Add `secretsDbPath: String` to `AgentConfig` and `SqliteSecretStore` to `AgentSession`
- [x] Add `EmailAccountDto` and `email_account_save/load/delete` methods to `AgentSession`
- [ ] Wire `secretsDbPath` into `AgentConfig(...)` call site in `agent_impl.dart` (flutter-dev worktree)
- [ ] Replace hardcoded AES dev key with a key derived from `flutter_secure_storage` (cross-platform):
  - Dart: on first launch, generate a random 32-byte key with `dart:math` `Random.secure()`, store as base64 under key `mobileclaw.secrets_key` via `FlutterSecureStorage`
  - Dart: on subsequent launches, read the key from secure storage (Android Keystore-backed on Android, iOS Keychain-backed on iOS — `flutter_secure_storage` handles this automatically)
  - Dart: add `encryptionKey: List<int>` field to `AgentConfig` and pass the 32 raw bytes when calling `AgentSession.create`
  - Rust: add `encryption_key: Vec<u8>` to `AgentConfig` in `ffi.rs`; replace `b"mobileclaw-dev-key-32bytes000000"` with `config.encryption_key.as_slice().try_into()` in `AgentSession::create`; remove `compile_error!` guard once done
  - Dart: update `agent_impl.dart` `MobileclawAgentImpl.create` to accept and forward `encryptionKey`

### Dart side

- [ ] Verify the generated `bridge_generated.dart` exports match the API contract in this document
- [ ] Implement a real `MobileclawAgent` that delegates to the generated bridge
- [ ] Confirm `ClawException` fields (`type`, `message`) match what the bridge emits
- [ ] Replace all `MockMobileclawAgent` usages with the real implementation
- [ ] Run the full widget test suite against the real FFI (on simulator / device)

### iOS

- [ ] Add `aarch64-apple-ios` and `aarch64-apple-ios-sim` to `rustup` target list
- [ ] Verify no JIT restriction issues (all Rust is AOT, should be fine)
- [ ] Confirm `.a` archive links correctly in Xcode

### Android

- [ ] Install `cargo-ndk` and verify `arm64-v8a` build succeeds
- [ ] Confirm `.so` files are loaded at runtime via Gradle `jniLibs`
- [ ] Test on physical ARM64 device

### Integration

- [ ] End-to-end test: `create()` → `chat()` → stream events match expected sequence
- [ ] Error path test: trigger `ClawException.pathTraversal` from Dart, verify `type` field
- [ ] Memory round-trip: `store()` → `recall()` → confirm score > 0
