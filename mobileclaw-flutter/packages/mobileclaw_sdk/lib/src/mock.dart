import 'dart:async';

import 'engine.dart';
import 'events.dart';
import 'memory.dart';
import 'models.dart';
// EmailAccountDto is part of models.dart — no extra import needed

/// Mock implementation of [MobileclawAgent].
/// All responses are synthetic; no Rust code is invoked.
/// Use during Phase 1 while the FFI binding is not yet available.
class MockMobileclawAgent implements MobileclawAgent {
  MockMobileclawAgent(
      {this.responseDelay = const Duration(milliseconds: 30)});

  final Duration responseDelay;
  final List<ChatMessage> _history = [];
  final _memory = MockMobileclawMemory();
  final List<SkillManifest> _skills = [];
  // Email accounts stored by id. Passwords are intentionally not stored.
  final Map<String, EmailAccountDto> _emailAccounts = {};
  final Map<String, ProviderConfigDto> _providers = {};
  String? _activeProviderId;

  static Future<MobileclawAgent> create({
    String? apiKey,
    required String dbPath,
    required String sandboxDir,
    required List<String> httpAllowlist,
    String? model,
    String? skillsDir,
    String? logDir,
  }) async =>
      MockMobileclawAgent();

  @override
  void dispose() {}

  @override
  Stream<AgentEvent> chat(String userInput, {String system = ''}) async* {
    _history.add(ChatMessage(role: 'user', content: userInput));

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

  @override
  Future<void> loadSkillsFromDir(String dirPath) async {}

  @override
  List<SkillManifest> get skills => List.unmodifiable(_skills);

  @override
  Future<void> emailAccountSave({
    required EmailAccountDto dto,
    required String password,
  }) async {
    // Store config only — password is intentionally discarded.
    _emailAccounts[dto.id] = dto;
  }

  @override
  Future<EmailAccountDto?> emailAccountLoad({required String id}) async =>
      _emailAccounts[id];

  @override
  Future<void> emailAccountDelete({required String id}) async {
    _emailAccounts.remove(id);
  }

  @override
  Future<void> providerSave({
    required ProviderConfigDto config,
    String? apiKey,
  }) async {
    // Assign a synthetic id if empty (mirrors Rust UUID generation)
    final id = config.id.isEmpty
        ? 'mock-${_providers.length + 1}'
        : config.id;
    _providers[id] = ProviderConfigDto(
      id: id,
      name: config.name,
      protocol: config.protocol,
      baseUrl: config.baseUrl,
      model: config.model,
      createdAt: config.createdAt == 0
          ? DateTime.now().millisecondsSinceEpoch ~/ 1000
          : config.createdAt,
    );
  }

  @override
  Future<List<ProviderConfigDto>> providerList() async =>
      _providers.values.toList()
        ..sort((a, b) => a.createdAt.compareTo(b.createdAt));

  @override
  Future<void> providerDelete({required String id}) async =>
      _providers.remove(id);

  @override
  Future<void> providerSetActive({required String id}) async {
    if (!_providers.containsKey(id)) {
      throw ClawException(
        type: 'ProviderNotFound',
        message: "provider not found: '$id'",
      );
    }
    _activeProviderId = id;
  }

  @override
  Future<ProviderConfigDto?> providerGetActive() async {
    if (_activeProviderId == null) return null;
    return _providers[_activeProviderId];
  }

  static Future<ProbeResultDto> probe({
    required ProviderConfigDto config,
    String? apiKey,
  }) async =>
      const ProbeResultDto(ok: true, latencyMs: 0, degraded: false, error: null);
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
