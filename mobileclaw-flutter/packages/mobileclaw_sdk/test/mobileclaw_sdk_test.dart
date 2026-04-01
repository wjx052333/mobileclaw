import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
import 'package:mobileclaw_sdk/src/bridge/frb_generated.dart';

void main() {
  // ---------------------------------------------------------------------------
  // AgentEvent sealed hierarchy
  // ---------------------------------------------------------------------------
  group('AgentEvent', () {
    test('TextDeltaEvent carries text', () {
      const e = TextDeltaEvent(text: 'hello');
      expect(e.text, 'hello');
      expect(e, isA<AgentEvent>());
    });

    test('ToolCallEvent carries toolName', () {
      const e = ToolCallEvent(toolName: 'http_request');
      expect(e.toolName, 'http_request');
      expect(e, isA<AgentEvent>());
    });

    test('ToolResultEvent carries toolName and success flag', () {
      const ok = ToolResultEvent(toolName: 'file_read', success: true);
      const fail = ToolResultEvent(toolName: 'file_read', success: false);
      expect(ok.success, isTrue);
      expect(fail.success, isFalse);
    });

    test('DoneEvent is an AgentEvent', () {
      const e = DoneEvent();
      expect(e, isA<AgentEvent>());
    });

    test('sealed switch is exhaustive — all variants reachable', () {
      String tag(AgentEvent e) => switch (e) {
            TextDeltaEvent() => 'text',
            ToolCallEvent() => 'call',
            ToolResultEvent() => 'result',
            DoneEvent() => 'done',
          };
      expect(tag(const TextDeltaEvent(text: 'x')), 'text');
      expect(tag(const ToolCallEvent(toolName: 't')), 'call');
      expect(tag(const ToolResultEvent(toolName: 't', success: true)), 'result');
      expect(tag(const DoneEvent()), 'done');
    });
  });

  // ---------------------------------------------------------------------------
  // ClawException
  // ---------------------------------------------------------------------------
  group('ClawException', () {
    test('type and message are preserved', () {
      const e = ClawException(type: 'Llm', message: 'rate limited');
      expect(e.type, 'Llm');
      expect(e.message, 'rate limited');
    });

    test('toString includes type and message', () {
      const e = ClawException(type: 'Io', message: 'disk full');
      expect(e.toString(), contains('Io'));
      expect(e.toString(), contains('disk full'));
    });

    test('pathTraversal factory', () {
      final e = ClawException.pathTraversal('../../etc/passwd');
      expect(e.type, 'PathTraversal');
      expect(e.message, contains('../../etc/passwd'));
    });

    test('urlNotAllowed factory', () {
      final e = ClawException.urlNotAllowed('http://evil.com');
      expect(e.type, 'UrlNotAllowed');
      expect(e.message, contains('http://evil.com'));
    });

    test('permissionDenied factory', () {
      final e = ClawException.permissionDenied('write disabled in community session');
      expect(e.type, 'PermissionDenied');
    });

    test('tool factory includes both tool name and message', () {
      final e = ClawException.tool('http_request', 'timeout after 5 s');
      expect(e.type, 'Tool');
      expect(e.message, contains('http_request'));
      expect(e.message, contains('timeout after 5 s'));
    });

    test('llm factory', () {
      final e = ClawException.llm('context window exceeded');
      expect(e.type, 'Llm');
    });

    test('memory factory', () {
      final e = ClawException.memory('SQLite locked');
      expect(e.type, 'Memory');
    });

    test('skillLoad factory', () {
      final e = ClawException.skillLoad('invalid manifest YAML');
      expect(e.type, 'SkillLoad');
    });

    test('implements Exception', () {
      const e = ClawException(type: 'Io', message: 'err');
      expect(e, isA<Exception>());
    });
  });

  // ---------------------------------------------------------------------------
  // MemoryCategory
  // ---------------------------------------------------------------------------
  group('MemoryCategory', () {
    test('named constants are equal to themselves', () {
      expect(MemoryCategory.core, equals(MemoryCategory.core));
      expect(MemoryCategory.daily, equals(MemoryCategory.daily));
      expect(MemoryCategory.conversation, equals(MemoryCategory.conversation));
    });

    test('different named constants are not equal', () {
      expect(MemoryCategory.core, isNot(equals(MemoryCategory.daily)));
      expect(MemoryCategory.daily, isNot(equals(MemoryCategory.conversation)));
    });

    test('custom category equal when same label', () {
      const a = MemoryCategory.custom('work');
      const b = MemoryCategory.custom('work');
      expect(a, equals(b));
    });

    test('custom category not equal when different label', () {
      const a = MemoryCategory.custom('work');
      const b = MemoryCategory.custom('personal');
      expect(a, isNot(equals(b)));
    });

    test('custom is not equal to a named category with same string', () {
      const c = MemoryCategory.custom('core');
      expect(c, isNot(equals(MemoryCategory.core)));
    });

    test('hashCode consistent with equality', () {
      expect(
        MemoryCategory.core.hashCode,
        equals(MemoryCategory.core.hashCode),
      );
      const a = MemoryCategory.custom('tag');
      const b = MemoryCategory.custom('tag');
      expect(a.hashCode, equals(b.hashCode));
    });

    test('toString returns expected values', () {
      expect(MemoryCategory.core.toString(), 'core');
      expect(MemoryCategory.daily.toString(), 'daily');
      expect(MemoryCategory.conversation.toString(), 'conversation');
      expect(const MemoryCategory.custom('x').toString(), 'custom:x');
    });
  });

  // ---------------------------------------------------------------------------
  // MockMobileclawMemory
  // ---------------------------------------------------------------------------
  group('MockMobileclawMemory', () {
    late MockMobileclawMemory mem;

    setUp(() => mem = MockMobileclawMemory());

    test('count starts at 0', () async {
      expect(await mem.count(), 0);
    });

    test('store returns a doc with correct fields', () async {
      final doc = await mem.store('notes/a.md', 'hello', MemoryCategory.core);
      expect(doc.path, 'notes/a.md');
      expect(doc.content, 'hello');
      expect(doc.category, MemoryCategory.core);
      expect(doc.id, isNotEmpty);
    });

    test('count increments after each distinct store', () async {
      await mem.store('a.md', 'x', MemoryCategory.core);
      await mem.store('b.md', 'y', MemoryCategory.daily);
      expect(await mem.count(), 2);
    });

    test('store overwrites existing path — id and createdAt are stable', () async {
      final first = await mem.store('p.md', 'v1', MemoryCategory.core);
      await Future.delayed(const Duration(milliseconds: 10));
      final second = await mem.store('p.md', 'v2', MemoryCategory.core);
      expect(second.id, first.id);
      expect(second.createdAt, first.createdAt);
      expect(second.content, 'v2');
      expect(second.updatedAt, greaterThanOrEqualTo(first.updatedAt));
    });

    test('store does not increase count on overwrite', () async {
      await mem.store('p.md', 'v1', MemoryCategory.core);
      await mem.store('p.md', 'v2', MemoryCategory.core);
      expect(await mem.count(), 1);
    });

    test('get returns null for unknown path', () async {
      expect(await mem.get('missing.md'), isNull);
    });

    test('get returns doc after store', () async {
      await mem.store('x.md', 'content', MemoryCategory.conversation);
      final doc = await mem.get('x.md');
      expect(doc, isNotNull);
      expect(doc!.content, 'content');
    });

    test('forget returns true when doc exists and removes it', () async {
      await mem.store('del.md', 'bye', MemoryCategory.core);
      expect(await mem.forget('del.md'), isTrue);
      expect(await mem.get('del.md'), isNull);
      expect(await mem.count(), 0);
    });

    test('forget returns false when doc does not exist', () async {
      expect(await mem.forget('ghost.md'), isFalse);
    });

    test('recall finds by content substring', () async {
      await mem.store('a.md', 'the quick brown fox', MemoryCategory.core);
      await mem.store('b.md', 'lazy dog', MemoryCategory.core);
      final results = await mem.recall('quick');
      expect(results.length, 1);
      expect(results.first.doc.path, 'a.md');
    });

    test('recall finds by path substring', () async {
      await mem.store('notes/profile.md', 'user data', MemoryCategory.core);
      final results = await mem.recall('profile');
      expect(results, isNotEmpty);
      expect(results.first.doc.path, 'notes/profile.md');
    });

    test('recall is case-insensitive', () async {
      await mem.store('a.md', 'Hello World', MemoryCategory.core);
      final results = await mem.recall('hello');
      expect(results, isNotEmpty);
    });

    test('recall filters by category', () async {
      await mem.store('a.md', 'shared content', MemoryCategory.core);
      await mem.store('b.md', 'shared content', MemoryCategory.daily);
      final results = await mem.recall(
        'shared',
        category: MemoryCategory.core,
      );
      expect(results.length, 1);
      expect(results.first.doc.category, MemoryCategory.core);
    });

    test('recall respects limit', () async {
      for (var i = 0; i < 5; i++) {
        await mem.store('doc$i.md', 'same content', MemoryCategory.core);
      }
      final results = await mem.recall('same', limit: 2);
      expect(results.length, 2);
    });

    test('recall returns empty list when no match', () async {
      await mem.store('a.md', 'hello', MemoryCategory.core);
      final results = await mem.recall('zzznomatch');
      expect(results, isEmpty);
    });

    test('recall score is positive', () async {
      await mem.store('a.md', 'hello', MemoryCategory.core);
      final results = await mem.recall('hello');
      expect(results.first.score, greaterThan(0));
    });

    test('recall since/until timestamp filtering', () async {
      // Store a doc first, then wait, then store another
      await mem.store('old.md', 'old content', MemoryCategory.core);
      await Future.delayed(const Duration(milliseconds: 20));
      final cutoff = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      await Future.delayed(const Duration(milliseconds: 20));
      await mem.store('new.md', 'new content', MemoryCategory.core);

      final sinceResults = await mem.recall('content', since: cutoff);
      expect(sinceResults.any((r) => r.doc.path == 'new.md'), isTrue);

      final untilResults = await mem.recall('content', until: cutoff);
      expect(untilResults.any((r) => r.doc.path == 'old.md'), isTrue);
    });
  });

  // ---------------------------------------------------------------------------
  // MockMobileclawAgent — chat flow
  // ---------------------------------------------------------------------------
  group('MockMobileclawAgent.chat', () {
    late MockMobileclawAgent agent;

    setUp(() => agent = MockMobileclawAgent(responseDelay: Duration.zero));

    test('emits TextDeltaEvent fragments then DoneEvent', () async {
      final events = await agent.chat('hello').toList();
      expect(events.last, isA<DoneEvent>());
      final textEvents = events.whereType<TextDeltaEvent>();
      expect(textEvents, isNotEmpty);
    });

    test('concatenated text contains user input echo', () async {
      final events = await agent.chat('ping').toList();
      final text = events
          .whereType<TextDeltaEvent>()
          .map((e) => e.text)
          .join();
      expect(text, contains('ping'));
    });

    test('input containing "file" triggers read_file ToolCallEvent', () async {
      final events = await agent.chat('read this file please').toList();
      final calls = events.whereType<ToolCallEvent>();
      expect(calls.any((e) => e.toolName == 'read_file'), isTrue);
    });

    test('input containing "search" triggers memory_search ToolCallEvent', () async {
      final events = await agent.chat('search my memory').toList();
      final calls = events.whereType<ToolCallEvent>();
      expect(calls.any((e) => e.toolName == 'memory_search'), isTrue);
    });

    test('ToolCallEvent is immediately followed by ToolResultEvent', () async {
      final events = await agent.chat('read this file').toList();
      final callIdx = events.indexWhere((e) => e is ToolCallEvent);
      expect(callIdx, greaterThanOrEqualTo(0));
      expect(events[callIdx + 1], isA<ToolResultEvent>());
    });

    test('ToolResultEvent for read_file has success=true', () async {
      final events = await agent.chat('read this file').toList();
      final result = events.whereType<ToolResultEvent>().first;
      expect(result.success, isTrue);
    });

    test('chatText returns non-empty string', () async {
      final text = await agent.chatText('hello');
      expect(text, isNotEmpty);
    });

    test('chatText result matches concatenated TextDeltaEvent fragments', () async {
      final text = await agent.chatText('hello');
      final events = await agent.chat('hello').toList();
      final fragmented = events
          .whereType<TextDeltaEvent>()
          .map((e) => e.text)
          .join();
      // Both calls produce a mock response to "hello" — trim for comparison
      expect(text.trim(), fragmented.trim());
    });

    test('history grows by 2 per chat call (user + assistant)', () async {
      expect(agent.history, isEmpty);
      await agent.chatText('first');
      expect(agent.history.length, 2);
      await agent.chatText('second');
      expect(agent.history.length, 4);
    });

    test('history roles alternate user / assistant', () async {
      await agent.chatText('test');
      expect(agent.history[0].role, 'user');
      expect(agent.history[1].role, 'assistant');
    });

    test('history is unmodifiable', () {
      expect(
        () => (agent.history as List).add(
          const ChatMessage(role: 'user', content: 'x'),
        ),
        throwsUnsupportedError,
      );
    });

    test('skills list is initially empty', () {
      expect(agent.skills, isEmpty);
    });

    test('skills list is unmodifiable', () {
      expect(
        () => (agent.skills as List).add(
          const SkillManifest(
            name: 'x',
            description: 'x',
            trust: SkillTrust.installed,
            keywords: [],
          ),
        ),
        throwsUnsupportedError,
      );
    });

    test('loadSkillsFromDir completes without throwing', () async {
      await expectLater(agent.loadSkillsFromDir('/any/path'), completes);
    });

    test('memory getter returns a MobileclawMemory', () {
      expect(agent.memory, isA<MobileclawMemory>());
    });

    test('memory operations work through the agent', () async {
      final doc = await agent.memory.store(
        'notes/profile.md',
        'User prefers dark mode.',
        MemoryCategory.core,
      );
      expect(doc.path, 'notes/profile.md');

      final results = await agent.memory.recall('dark mode');
      expect(results, isNotEmpty);
    });

    test('dispose does not throw', () {
      expect(() => agent.dispose(), returnsNormally);
    });
  });

  // ---------------------------------------------------------------------------
  // MemoryDoc — timestamp helpers
  // ---------------------------------------------------------------------------
  group('MemoryDoc', () {
    test('createdAtDt converts Unix seconds to DateTime', () {
      const doc = MemoryDoc(
        id: 'abc',
        path: 'x.md',
        content: 'y',
        category: MemoryCategory.core,
        createdAt: 1_000_000,
        updatedAt: 1_000_001,
      );
      expect(
        doc.createdAtDt,
        DateTime.fromMillisecondsSinceEpoch(1_000_000 * 1000),
      );
      expect(
        doc.updatedAtDt,
        DateTime.fromMillisecondsSinceEpoch(1_000_001 * 1000),
      );
    });
  });

  // ---------------------------------------------------------------------------
  // ChatMessage
  // ---------------------------------------------------------------------------
  group('ChatMessage', () {
    test('fields are accessible', () {
      const m = ChatMessage(role: 'user', content: 'hello');
      expect(m.role, 'user');
      expect(m.content, 'hello');
    });
  });

  // ---------------------------------------------------------------------------
  // SkillManifest
  // ---------------------------------------------------------------------------
  group('SkillManifest', () {
    test('bundled skill has no allowedTools restriction', () {
      const s = SkillManifest(
        name: 'weather',
        description: 'Weather queries',
        trust: SkillTrust.bundled,
        keywords: ['weather', 'temperature'],
      );
      expect(s.allowedTools, isNull);
      expect(s.trust, SkillTrust.bundled);
    });

    test('installed skill can have allowedTools restriction', () {
      const s = SkillManifest(
        name: 'calc',
        description: 'Calculator',
        trust: SkillTrust.installed,
        keywords: ['calculate'],
        allowedTools: ['http_request'],
      );
      expect(s.allowedTools, contains('http_request'));
    });

    test('keywords are accessible', () {
      const s = SkillManifest(
        name: 'x',
        description: 'x',
        trust: SkillTrust.bundled,
        keywords: ['kw1', 'kw2'],
      );
      expect(s.keywords, ['kw1', 'kw2']);
    });
  });

  // ---------------------------------------------------------------------------
  // MobileclawAgentImpl (Linux integration)
  // ---------------------------------------------------------------------------
  group('MobileclawAgentImpl (Linux integration)', () {
    final _run = Platform.environment['INTEGRATION'] == 'true';

    setUpAll(() async {
      if (!_run) return;
      // Load the native Rust library once for all integration tests.
      await MobileclawCoreBridge.init(
        externalLibrary: ExternalLibrary.open(
          '${Directory.current.path}/linux/libmobileclaw_core.so',
        ),
      );
    });

    test('create() succeeds and memory starts empty', () async {
      if (!_run) return;

      final dir = Directory.systemTemp.createTempSync('claw_test_');
      try {
        final agent = await MobileclawAgentImpl.create(
          apiKey: 'test-key',
          dbPath: '${dir.path}/m.db',
          sandboxDir: dir.path,
          httpAllowlist: [],
        );
        expect(await agent.memory.count(), 0);
        agent.dispose();
      } finally {
        dir.deleteSync(recursive: true);
      }
    }, timeout: const Timeout(Duration(seconds: 10)));

    test('memory store / recall round-trip via real SQLite', () async {
      if (!_run) return;

      final dir = Directory.systemTemp.createTempSync('claw_test2_');
      try {
        final agent = await MobileclawAgentImpl.create(
          apiKey: 'test-key',
          dbPath: '${dir.path}/m.db',
          sandboxDir: dir.path,
          httpAllowlist: [],
        );
        final doc = await agent.memory.store(
          'notes/test.md', 'hello world', MemoryCategory.core,
        );
        expect(doc.path, 'notes/test.md');
        final results = await agent.memory.recall('hello');
        expect(results, isNotEmpty);
        expect(results.first.score, greaterThan(0));
        agent.dispose();
      } finally {
        dir.deleteSync(recursive: true);
      }
    }, timeout: const Timeout(Duration(seconds: 10)));
  }, skip: Platform.environment['INTEGRATION'] != 'true' ? 'set INTEGRATION=true to run' : null);

  // ---------------------------------------------------------------------------
  // Model equality
  // ---------------------------------------------------------------------------
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

      // Two manifests differing only in allowedTools must be unequal
      // and have different hash codes.
      const withTools = SkillManifest(
        name: 'n', description: 'd',
        trust: SkillTrust.bundled, keywords: ['k'],
        allowedTools: ['file_read'],
      );
      const withoutTools = SkillManifest(
        name: 'n', description: 'd',
        trust: SkillTrust.bundled, keywords: ['k'],
      );
      expect(withTools, isNot(equals(withoutTools)));
      expect(withTools.hashCode, isNot(equals(withoutTools.hashCode)));
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
}

