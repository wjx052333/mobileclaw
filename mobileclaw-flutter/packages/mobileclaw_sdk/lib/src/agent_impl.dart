import 'dart:async';

import 'bridge/ffi.dart';
import 'engine.dart';
import 'events.dart';
import 'memory.dart';
import 'models.dart';

// ---------------------------------------------------------------------------
// Category helpers
// ---------------------------------------------------------------------------

/// Convert a [MemoryCategory] to the string representation expected by Rust.
/// - core        → "core"
/// - daily       → "daily"
/// - conversation → "conversation"
/// - custom(x)   → "custom:x"
String _categoryToString(MemoryCategory c) => c.toString();

/// Convert a Rust category string back to a [MemoryCategory].
/// Strings that don't match a named constant are treated as custom.
MemoryCategory _stringToCategory(String s) {
  switch (s) {
    case 'core':
      return MemoryCategory.core;
    case 'daily':
      return MemoryCategory.daily;
    case 'conversation':
      return MemoryCategory.conversation;
    default:
      if (s.startsWith('custom:')) {
        return MemoryCategory.custom(s.substring(7));
      }
      return MemoryCategory.custom(s);
  }
}

// ---------------------------------------------------------------------------
// DTO → domain converters
// ---------------------------------------------------------------------------

MemoryDoc _docFromDto(MemoryDocDto dto) => MemoryDoc(
      id: dto.id,
      path: dto.path,
      content: dto.content,
      category: _stringToCategory(dto.category),
      createdAt: dto.createdAt.toInt(),
      updatedAt: dto.updatedAt.toInt(),
    );

AgentEvent _eventFromDto(AgentEventDto dto) => dto.when(
      textDelta: (text) => TextDeltaEvent(text: text),
      toolCall: (name) => ToolCallEvent(toolName: name),
      toolResult: (name, success) =>
          ToolResultEvent(toolName: name, success: success),
      done: () => const DoneEvent(),
    );

SkillTrust _trustFromString(String trust) {
  switch (trust) {
    case 'bundled':
      return SkillTrust.bundled;
    case 'installed':
    default:
      return SkillTrust.installed;
  }
}

SkillManifest _skillFromDto(SkillManifestDto dto) => SkillManifest(
      name: dto.name,
      description: dto.description,
      trust: _trustFromString(dto.trust),
      keywords: List.unmodifiable(dto.keywords),
      allowedTools: dto.allowedTools.isEmpty ? null : List.unmodifiable(dto.allowedTools),
    );

// ---------------------------------------------------------------------------
// _RealMemory
// ---------------------------------------------------------------------------

/// [MobileclawMemory] implementation backed by the Rust FFI session.
class _RealMemory implements MobileclawMemory {
  _RealMemory(this._session);

  final AgentSession _session;

  /// Store a document in the memory database.
  /// throws ClawException on memory or I/O error from Rust.
  @override
  Future<MemoryDoc> store(
    String path,
    String content,
    MemoryCategory category,
  ) async {
    final dto = await _session.memoryStore(
      path: path,
      content: content,
      category: _categoryToString(category),
    );
    return _docFromDto(dto);
  }

  /// Search the memory database and return ranked results.
  /// throws ClawException on memory or I/O error from Rust.
  @override
  Future<List<SearchResult>> recall(
    String query, {
    int limit = 10,
    MemoryCategory? category,
    int? since,
    int? until,
  }) async {
    final dtos = await _session.memoryRecall(
      query: query,
      limit: BigInt.from(limit),
      category: category != null ? _categoryToString(category) : null,
      since: since != null ? BigInt.from(since) : null,
      until: until != null ? BigInt.from(until) : null,
    );
    return dtos
        .map((r) => SearchResult(doc: _docFromDto(r.doc), score: r.score))
        .toList();
  }

  /// Retrieve a single memory document by path.
  /// Returns null if the document does not exist.
  /// throws ClawException on memory or I/O error from Rust.
  @override
  Future<MemoryDoc?> get(String path) async {
    final dto = await _session.memoryGet(path: path);
    if (dto == null) return null;
    return _docFromDto(dto);
  }

  /// Delete a memory document. Returns true if it existed.
  /// throws ClawException on memory or I/O error from Rust.
  @override
  Future<bool> forget(String path) => _session.memoryForget(path: path);

  /// Return the total number of memory documents.
  /// throws ClawException on memory or I/O error from Rust.
  @override
  Future<int> count() async {
    final n = await _session.memoryCount();
    return n.toInt();
  }
}

// ---------------------------------------------------------------------------
// MobileclawAgentImpl
// ---------------------------------------------------------------------------

/// Real [MobileclawAgent] implementation backed by the Rust FFI bridge.
///
/// Obtain via [MobileclawAgentImpl.create]; do not instantiate directly.
/// Thread safety: not safe to share across Flutter isolates.
/// Create one instance per isolate, or serialize all calls from a single isolate.
class MobileclawAgentImpl implements MobileclawAgent {
  MobileclawAgentImpl._(this._session)
      : _memory = _RealMemory(_session),
        _cachedHistory = [],
        _cachedSkills = [];

  final AgentSession _session;
  final _RealMemory _memory;

  // Caches for synchronous getters (populated after async calls).
  List<ChatMessage> _cachedHistory;
  List<SkillManifest> _cachedSkills;

  /// Create and initialise a real agent session backed by Rust.
  ///
  /// - [apiKey]        Anthropic API key.
  /// - [dbPath]        Absolute path to the SQLite database file.
  /// - [sandboxDir]    Root directory for file-system tools.
  /// - [httpAllowlist] URL prefixes the HTTP tool may fetch.
  /// - [model]         LLM model identifier.
  /// - [skillsDir]     Optional directory of skill bundles.
  ///
  /// throws ClawException if the Rust session cannot be created.
  static Future<MobileclawAgentImpl> create({
    required String apiKey,
    required String dbPath,
    required String sandboxDir,
    required List<String> httpAllowlist,
    String model = 'claude-opus-4-6',
    String? skillsDir,
  }) async {
    final config = AgentConfig(
      apiKey: apiKey,
      dbPath: dbPath,
      sandboxDir: sandboxDir,
      httpAllowlist: httpAllowlist,
      model: model,
      skillsDir: skillsDir,
    );
    final session = await AgentSession.create(config: config);
    return MobileclawAgentImpl._(session);
  }

  /// Release all Rust-side resources. Must not be used after this call.
  @override
  void dispose() {
    _session.dispose();
  }

  /// Stream all events for one user turn.
  ///
  /// Completes when [DoneEvent] is emitted or an error is thrown as [ClawException].
  /// throws ClawException on LLM, tool, or I/O error from Rust.
  @override
  Stream<AgentEvent> chat(String userInput, {String system = ''}) async* {
    final dtos = await _session.chat(input: userInput, system: system);
    // Refresh history cache after the turn completes.
    _refreshHistoryFromDtos(await _session.history());
    for (final dto in dtos) {
      yield _eventFromDto(dto);
    }
  }

  /// Convenience wrapper: collects all [TextDeltaEvent] fragments into a string.
  /// throws ClawException on LLM, tool, or I/O error from Rust.
  @override
  Future<String> chatText(String userInput, {String system = ''}) async {
    final buffer = StringBuffer();
    await for (final event in chat(userInput, system: system)) {
      if (event is TextDeltaEvent) buffer.write(event.text);
    }
    return buffer.toString();
  }

  /// The full conversation history for the current session.
  /// Reflects the state after the last completed [chat] call.
  @override
  List<ChatMessage> get history => List.unmodifiable(_cachedHistory);

  /// Memory subsystem.
  @override
  MobileclawMemory get memory => _memory;

  /// Load all skill bundles found under [dirPath].
  /// throws ClawException if the directory does not exist or contains invalid manifests.
  @override
  Future<void> loadSkillsFromDir(String dirPath) async {
    await _session.loadSkillsFromDir(dir: dirPath);
    _cachedSkills = (await _session.skills()).map(_skillFromDto).toList();
  }

  /// Manifests of all currently loaded skills, in load order.
  /// Reflects the state after the last completed [loadSkillsFromDir] call.
  @override
  List<SkillManifest> get skills => List.unmodifiable(_cachedSkills);

  // ---------------------------------------------------------------------------
  // Private helpers
  // ---------------------------------------------------------------------------

  void _refreshHistoryFromDtos(List<MessageDto> dtos) {
    _cachedHistory = dtos
        .map((m) => ChatMessage(role: m.role, content: m.content))
        .toList();
  }
}
