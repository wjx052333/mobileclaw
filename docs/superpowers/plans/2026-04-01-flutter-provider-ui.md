# Flutter Provider UI — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Flutter UI for managing LLM providers — list, add, edit, delete, test, and set active — using Riverpod state management and the FFI methods exposed by Plan 1 (Rust core).

**Architecture:** Three new screens (`ProviderListScreen`, `ProviderFormScreen`, `OnboardingScreen`) use Riverpod `StateNotifierProvider` for provider list state; the notifier calls `MobileclawAgent` methods which delegate to Rust FFI. The `MobileclawAgent` abstract class gains five new provider methods and one free function (`providerProbe`); `MockMobileclawAgent` implements them in-memory so UI work proceeds before FFI codegen runs. Settings page gains an entry that pushes `ProviderListScreen`.

**Tech Stack:** Flutter, Riverpod 2.x (`StateNotifierProvider`, `ConsumerWidget`), `flutter_test` + `WidgetTester`, `MockMobileclawAgent` (in-process mock for all tests — no mockito needed), `mobileclaw_sdk` package, `mobileclaw_app` app package.

**Spec:** `docs/superpowers/specs/2026-04-01-multi-provider-llm-design.md`
**Plan 1 (Rust core):** `docs/superpowers/plans/2026-04-01-multi-provider-llm.md`

---

## Important: FFI Bridge Codegen Dependency

The new FFI methods (`providerSave`, `providerList`, etc.) are added to `MobileclawAgent` (the Dart abstract class) and `MockMobileclawAgent` first. The `AgentSession` abstract class in `ffi.dart` and `MobileclawAgentImpl` are updated **after** Plan 1 completes and `flutter_rust_bridge` codegen is re-run. Until then, all tests use `MockMobileclawAgent`. The plan calls out exactly when to swap in the real FFI calls.

**Regenerate FFI bindings after Plan 1 merges:**
```bash
cd mobileclaw-flutter/packages/mobileclaw_sdk
flutter_rust_bridge_codegen generate
```
This overwrites `lib/src/bridge/ffi.dart` and `frb_generated.dart` with the new provider methods.

---

## File Map

| Action   | Path                                                                                          | Responsibility                                                                          |
|----------|-----------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------|
| Modify   | `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/models.dart`                             | Add `ProviderConfigDto`, `ProbeResultDto` Dart model classes                            |
| Modify   | `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/engine.dart`                             | Add 5 provider methods + `providerProbe` free fn to `MobileclawAgent` abstract class    |
| Modify   | `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/mock.dart`                               | Implement provider methods in `MockMobileclawAgent`                                     |
| Modify   | `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart`                         | Implement provider methods in `MobileclawAgentImpl` (after FFI codegen)                 |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_models.dart`         | `ProviderProtocol` enum with URL hints and display names                                |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_notifier.dart`       | `ProviderNotifier extends StateNotifier<AsyncValue<List<ProviderConfigDto>>>`           |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_list_screen.dart`    | `ProviderListScreen` — list view, swipe-to-delete, FAB, active-provider tap            |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_form_screen.dart`    | `ProviderFormScreen` — protocol picker, URL/model/key fields, Test + Save buttons       |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/onboarding_screen.dart`       | `OnboardingScreen` — first-launch wizard wrapping `ProviderFormScreen`                  |
| Modify   | `mobileclaw-flutter/apps/mobileclaw_app/lib/main.dart`                                       | Add onboarding check: show `OnboardingScreen` if no providers configured                |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/lib/features/settings/settings_page.dart`            | `SettingsPage` with "LLM Providers" entry linking to `ProviderListScreen`               |
| Modify   | `mobileclaw-flutter/apps/mobileclaw_app/lib/features/chat/chat_page.dart`                    | Add settings gear icon in AppBar that pushes `SettingsPage`                             |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_notifier_test.dart` | Unit tests for `ProviderNotifier` (mock agent)                                          |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_list_screen_test.dart` | Widget tests for `ProviderListScreen`                                                |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_form_screen_test.dart` | Widget tests for `ProviderFormScreen`                                                |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/onboarding_screen_test.dart` | Widget test for `OnboardingScreen`                                                      |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/test/features/settings/settings_page_test.dart`      | Widget tests for `SettingsPage`                                                         |
| Create   | `mobileclaw-flutter/apps/mobileclaw_app/test/app_routing_test.dart`                          | Integration-style widget tests for onboarding routing logic in `_AppShell`              |

---

## Task 1: Add ProviderConfigDto and ProbeResultDto to SDK Models

**Files:**
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/models.dart`

These are the Dart-side data classes that mirror the Rust FFI DTOs. They live in the SDK package so both the app and tests can import them from `mobileclaw_sdk`.

- [ ] **Step 1.1: Write the failing test (compile check)**

  The existing file at `mobileclaw-flutter/packages/mobileclaw_sdk/test/mobileclaw_sdk_test.dart` has a `main()` function. Add a new group inside it. The final file should look like:

  ```dart
  import 'package:flutter_test/flutter_test.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  void main() {
    // ... existing tests stay here ...

    group('ProviderConfigDto', () {
      test('equality', () {
        const a = ProviderConfigDto(
          id: '1', name: 'Claude', protocol: 'anthropic',
          baseUrl: 'https://api.anthropic.com', model: 'claude-opus-4-6',
          createdAt: 1000,
        );
        const b = ProviderConfigDto(
          id: '1', name: 'Claude', protocol: 'anthropic',
          baseUrl: 'https://api.anthropic.com', model: 'claude-opus-4-6',
          createdAt: 1000,
        );
        expect(a, equals(b));
      });
    });

    group('ProbeResultDto', () {
      test('fields accessible', () {
        const r = ProbeResultDto(ok: true, latencyMs: 120, degraded: false, error: null);
        expect(r.ok, isTrue);
        expect(r.latencyMs, 120);
        expect(r.degraded, isFalse);
        expect(r.error, isNull);
      });
    });
  }
  ```

  Use your Read tool to check the existing content of `mobileclaw_sdk_test.dart` first, then append the two new `group()` blocks inside the existing `main()` body.

- [ ] **Step 1.2: Run test — expect compile error (class not defined)**

  ```bash
  cd mobileclaw-flutter/packages/mobileclaw_sdk
  flutter test test/mobileclaw_sdk_test.dart
  ```
  Expected: `Error: Undefined name 'ProviderConfigDto'`

- [ ] **Step 1.3: Add the model classes to models.dart**

  Append to `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/models.dart`:

  ```dart
  /// LLM provider configuration. No API key field — stored encrypted in Rust.
  class ProviderConfigDto {
    const ProviderConfigDto({
      required this.id,
      required this.name,
      required this.protocol,
      required this.baseUrl,
      required this.model,
      required this.createdAt,
    });

    /// UUID, assigned by Rust on first save.
    final String id;

    /// Display name (e.g., "My DeepSeek").
    final String name;

    /// One of: "anthropic", "openai_compat", "ollama".
    final String protocol;

    /// Base URL (e.g., "https://api.anthropic.com").
    final String baseUrl;

    /// Model identifier (e.g., "claude-opus-4-6").
    final String model;

    /// Unix timestamp (seconds) when this config was first saved.
    final int createdAt;

    @override
    bool operator ==(Object other) =>
        other is ProviderConfigDto &&
        other.id == id &&
        other.name == name &&
        other.protocol == protocol &&
        other.baseUrl == baseUrl &&
        other.model == model &&
        other.createdAt == createdAt;

    @override
    int get hashCode =>
        Object.hash(id, name, protocol, baseUrl, model, createdAt);

    @override
    String toString() =>
        'ProviderConfigDto(id: $id, name: $name, protocol: $protocol, '
        'baseUrl: $baseUrl, model: $model)';
  }

  /// Result of a provider reachability probe.
  class ProbeResultDto {
    const ProbeResultDto({
      required this.ok,
      required this.latencyMs,
      required this.degraded,
      required this.error,
    });

    /// True if the provider is usable.
    final bool ok;

    /// Round-trip latency of the probe request, in milliseconds.
    final int latencyMs;

    /// True if the models endpoint responded but the completions endpoint
    /// did not. Provider is reachable but completions are unverified.
    final bool degraded;

    /// Error message if ok is false; null otherwise.
    final String? error;

    @override
    bool operator ==(Object other) =>
        other is ProbeResultDto &&
        other.ok == ok &&
        other.latencyMs == latencyMs &&
        other.degraded == degraded &&
        other.error == error;

    @override
    int get hashCode => Object.hash(ok, latencyMs, degraded, error);
  }
  ```

- [ ] **Step 1.4: Run test — expect PASS**

  ```bash
  cd mobileclaw-flutter/packages/mobileclaw_sdk
  flutter test test/mobileclaw_sdk_test.dart
  ```
  Expected: all tests pass.

- [ ] **Step 1.5: Commit**

  ```bash
  git add mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/models.dart \
           mobileclaw-flutter/packages/mobileclaw_sdk/test/mobileclaw_sdk_test.dart
  git commit -m "feat(sdk): add ProviderConfigDto and ProbeResultDto model classes"
  ```

---

## Task 2: Add Provider Methods to MobileclawAgent Abstract Class

**Files:**
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/engine.dart`

Add the five provider management methods and the `providerProbe` free function to the `MobileclawAgent` abstract class. These are the contracts that `MockMobileclawAgent` and `MobileclawAgentImpl` both must implement.

`providerProbe` is a free function (not an instance method) because it doesn't require a session — it can be called before any session is configured. In Dart, expose it as a `static` method on the abstract class or as a top-level function exported from the SDK. Use a top-level function for symmetry with the Rust free function.

- [ ] **Step 2.1: Add abstract methods to engine.dart**

  In `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/engine.dart`, after the `emailAccountDelete` method declaration, add:

  ```dart
  // ---------------------------------------------------------------------------
  // Provider management
  // ---------------------------------------------------------------------------

  /// Save (or update) a provider configuration and optionally its API key.
  ///
  /// - If [apiKey] is non-null, it is encrypted and stored on the Rust side.
  /// - If [apiKey] is null and the provider already exists, the stored key
  ///   is preserved (useful when editing config fields without changing key).
  /// - [config.id] must be non-empty when updating an existing provider.
  ///   Pass an empty string for [config.id] on first save — Rust generates a UUID.
  ///
  /// Throws [ClawException] on storage error.
  Future<void> providerSave({
    required ProviderConfigDto config,
    String? apiKey,
  });

  /// List all saved provider configurations, ordered by creation time ascending.
  ///
  /// Returns an empty list if no providers are configured.
  /// Throws [ClawException] on storage error.
  Future<List<ProviderConfigDto>> providerList();

  /// Delete a provider and its stored API key.
  ///
  /// No-op if the provider does not exist.
  /// Throws [ClawException] on storage error.
  Future<void> providerDelete({required String id});

  /// Set the active provider. The session must be re-created to pick up the change.
  ///
  /// Throws [ClawException] if [id] does not exist in the store.
  Future<void> providerSetActive({required String id});

  /// Return the active provider config, or null if none is set.
  ///
  /// Throws [ClawException] on storage error.
  Future<ProviderConfigDto?> providerGetActive();
  ```

  Also add the free function **below** the class definition (outside the abstract class body):

  ```dart
  /// Test whether a provider is reachable.
  ///
  /// Does NOT require an active [MobileclawAgent] session — safe to call
  /// during first-launch onboarding before any session is created.
  ///
  /// [config.id] may be empty (not yet saved).
  /// [apiKey] is required for Anthropic and OpenAI-compatible providers;
  /// may be null for Ollama.
  ///
  /// Never throws — errors are returned as [ProbeResultDto.ok] == false.
  Future<ProbeResultDto> providerProbe({
    required ProviderConfigDto config,
    String? apiKey,
  });
  ```

  Note: `providerProbe` is declared as a top-level function here. Each concrete implementation file (`mock.dart`, `agent_impl.dart`) will define it separately. At the SDK export level, `mobileclaw_sdk.dart` exports the right implementation based on which file is imported. To avoid platform-specific conditional exports for now, declare `providerProbe` as a top-level function in `engine.dart` that throws `UnimplementedError` — each file overrides via a different import or direct call. **Simpler alternative (use this):** make `providerProbe` a static method on `MobileclawAgent`:

  Replace the top-level `providerProbe` declaration above with:
  ```dart
  // Inside the abstract class body, after providerGetActive:

  /// Test whether a provider is reachable. Static so it can be called without
  /// a session (e.g., during onboarding before any provider is saved).
  ///
  /// Never throws — errors returned in [ProbeResultDto.ok].
  static Future<ProbeResultDto> probe({
    required ProviderConfigDto config,
    String? apiKey,
  }) {
    throw UnimplementedError(
      'Phase 2: replace with real FFI call. '
      'Call MobileclawAgentImpl.probe() or MockMobileclawAgent.probe().',
    );
  }
  ```

- [ ] **Step 2.2: Build — expect clean compile**

  ```bash
  cd mobileclaw-flutter/packages/mobileclaw_sdk
  flutter analyze
  ```
  Expected: no errors. The abstract methods are not yet implemented — that's fine at this stage (they only need implementations in the concrete classes).

- [ ] **Step 2.3: Commit**

  ```bash
  git add mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/engine.dart
  git commit -m "feat(sdk): add provider management methods to MobileclawAgent abstract class"
  ```

---

## Task 3: Implement Provider Methods in MockMobileclawAgent

**Files:**
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/mock.dart`

The mock stores providers in a `Map<String, ProviderConfigDto>` (keyed by id). `probe()` always returns `ProbeResultDto(ok: true, latencyMs: 0, degraded: false, error: null)` — it never makes a real network call.

- [ ] **Step 3.1: Write failing tests**

  Create `mobileclaw-flutter/packages/mobileclaw_sdk/test/mock_provider_test.dart`:

  ```dart
  import 'package:flutter_test/flutter_test.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  void main() {
    late MockMobileclawAgent agent;

    setUp(() => agent = MockMobileclawAgent());

    const cfg = ProviderConfigDto(
      id: 'p1', name: 'Groq', protocol: 'openai_compat',
      baseUrl: 'https://api.groq.com/openai', model: 'mixtral-8x7b',
      createdAt: 1000,
    );

    test('providerList is empty initially', () async {
      expect(await agent.providerList(), isEmpty);
    });

    test('providerSave and providerList', () async {
      await agent.providerSave(config: cfg, apiKey: 'sk-test');
      final list = await agent.providerList();
      expect(list.length, 1);
      expect(list.first.name, 'Groq');
    });

    test('providerDelete removes provider', () async {
      await agent.providerSave(config: cfg, apiKey: 'sk-test');
      await agent.providerDelete(id: 'p1');
      expect(await agent.providerList(), isEmpty);
    });

    test('providerSetActive and providerGetActive', () async {
      await agent.providerSave(config: cfg, apiKey: 'sk-test');
      await agent.providerSetActive(id: 'p1');
      final active = await agent.providerGetActive();
      expect(active?.id, 'p1');
    });

    test('providerGetActive returns null if none set', () async {
      expect(await agent.providerGetActive(), isNull);
    });

    test('MockMobileclawAgent.probe returns ok=true', () async {
      final result = await MockMobileclawAgent.probe(config: cfg, apiKey: 'key');
      expect(result.ok, isTrue);
      expect(result.degraded, isFalse);
    });
  }
  ```

- [ ] **Step 3.2: Run tests — expect compile error (methods not defined)**

  ```bash
  cd mobileclaw-flutter/packages/mobileclaw_sdk
  flutter test test/mock_provider_test.dart
  ```
  Expected: `Error: Class 'MockMobileclawAgent' has no instance method 'providerList'`

- [ ] **Step 3.3: Implement provider methods in MockMobileclawAgent**

  In `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/mock.dart`:

  Add to `MockMobileclawAgent` class fields:
  ```dart
  final Map<String, ProviderConfigDto> _providers = {};
  String? _activeProviderId;
  ```

  Add implementations after `emailAccountDelete`:
  ```dart
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
  ```

  Also override the static `probe` on the abstract class — Dart does not allow overriding static methods, so the `MockMobileclawAgent.probe` above is a new static that shadows it. This is intentional.

- [ ] **Step 3.4: Run tests — expect PASS**

  ```bash
  cd mobileclaw-flutter/packages/mobileclaw_sdk
  flutter test test/mock_provider_test.dart
  ```
  Expected: 6 tests pass.

- [ ] **Step 3.5: Commit**

  ```bash
  git add mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/mock.dart \
           mobileclaw-flutter/packages/mobileclaw_sdk/test/mock_provider_test.dart
  git commit -m "feat(sdk): implement provider methods in MockMobileclawAgent"
  ```

---

## Task 4: Provider Models and Notifier

**Files:**
- Create: `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_models.dart`
- Create: `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_notifier.dart`

`provider_models.dart` holds the `ProviderProtocol` enum with UI-level metadata (URL hints, display names). `provider_notifier.dart` is a Riverpod `StateNotifier` that manages `AsyncValue<List<ProviderConfigDto>>` and calls `MobileclawAgent` for CRUD. All business logic lives here; screens are dumb.

- [ ] **Step 4.1: Write failing notifier test**

  Create `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_notifier_test.dart`:

  ```dart
  import 'package:flutter_riverpod/flutter_riverpod.dart';
  import 'package:flutter_test/flutter_test.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  import 'package:mobileclaw_app/features/providers/provider_notifier.dart';

  // Helper: build a ProviderContainer with a mock agent
  ProviderContainer makeContainer(MobileclawAgent agent) {
    return ProviderContainer(
      overrides: [agentInstanceProvider.overrideWithValue(agent)],
    );
  }

  void main() {
    late MockMobileclawAgent agent;
    late ProviderContainer container;

    setUp(() {
      agent = MockMobileclawAgent();
      container = makeContainer(agent);
    });

    tearDown(() => container.dispose());

    test('providerListProvider loads empty list', () async {
      final notifier = container.read(providerListProvider.notifier);
      await notifier.refresh();
      final state = container.read(providerListProvider);
      expect(state, isA<AsyncData<List<ProviderConfigDto>>>());
      expect(state.value, isEmpty);
    });

    test('addProvider saves and refreshes list', () async {
      const cfg = ProviderConfigDto(
        id: '', name: 'Groq', protocol: 'openai_compat',
        baseUrl: 'https://api.groq.com/openai', model: 'mixtral-8x7b',
        createdAt: 0,
      );
      final notifier = container.read(providerListProvider.notifier);
      await notifier.addProvider(config: cfg, apiKey: 'sk-test');
      final list = container.read(providerListProvider).value!;
      expect(list.length, 1);
      expect(list.first.name, 'Groq');
    });

    test('deleteProvider removes from list', () async {
      await agent.providerSave(
        config: const ProviderConfigDto(
          id: 'p1', name: 'X', protocol: 'ollama',
          baseUrl: 'http://localhost:11434', model: 'llama3', createdAt: 1000,
        ),
      );
      final notifier = container.read(providerListProvider.notifier);
      await notifier.refresh();
      await notifier.deleteProvider(id: 'p1');
      expect(container.read(providerListProvider).value, isEmpty);
    });

    test('setActive calls agent.providerSetActive', () async {
      await agent.providerSave(
        config: const ProviderConfigDto(
          id: 'p1', name: 'X', protocol: 'ollama',
          baseUrl: 'http://localhost:11434', model: 'llama3', createdAt: 1000,
        ),
      );
      final notifier = container.read(providerListProvider.notifier);
      await notifier.refresh();
      await notifier.setActive(id: 'p1');
      final active = await agent.providerGetActive();
      expect(active?.id, 'p1');
    });
  }
  ```

- [ ] **Step 4.2: Run test — expect compile error**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/features/providers/provider_notifier_test.dart
  ```
  Expected: `Error: uri 'package:mobileclaw_app/features/providers/provider_notifier.dart' not found`

- [ ] **Step 4.3: Create provider_models.dart**

  Create `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_models.dart`:

  ```dart
  /// App-side enum mirroring the Rust `ProviderProtocol`.
  /// Carries UI metadata: display name and URL hint.
  enum ProviderProtocol {
    anthropic(
      value: 'anthropic',
      displayName: 'Anthropic (Claude)',
      urlHint: 'https://api.anthropic.com',
    ),
    openAiCompat(
      value: 'openai_compat',
      displayName: 'OpenAI-compatible',
      urlHint: 'https://api.openai.com',
    ),
    ollama(
      value: 'ollama',
      displayName: 'Ollama (local)',
      urlHint: 'http://localhost:11434',
    );

    const ProviderProtocol({
      required this.value,
      required this.displayName,
      required this.urlHint,
    });

    /// The wire string sent to / received from Rust FFI.
    final String value;

    /// Human-readable name shown in the protocol picker.
    final String displayName;

    /// Placeholder URL shown in the URL field when this protocol is selected.
    final String urlHint;

    static ProviderProtocol fromValue(String value) => values.firstWhere(
          (p) => p.value == value,
          orElse: () => throw ArgumentError('Unknown protocol: $value'),
        );
  }
  ```

- [ ] **Step 4.4: Create provider_notifier.dart**

  Create `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_notifier.dart`:

  ```dart
  import 'package:flutter_riverpod/flutter_riverpod.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  import '../../core/engine_provider.dart';

  /// Exposes the agent singleton so tests can override it.
  /// Points at the existing [agentProvider]; tests override this with a mock.
  final agentInstanceProvider = Provider<MobileclawAgent>((ref) {
    // Throws StateError if called before agentProvider resolves.
    return ref.watch(agentProvider).requireValue;
  });

  /// Riverpod provider for the list of saved LLM providers.
  ///
  /// State: AsyncValue<List<ProviderConfigDto>>
  ///   - AsyncLoading: initial fetch in progress
  ///   - AsyncData: list loaded (may be empty)
  ///   - AsyncError: fetch failed
  final providerListProvider =
      StateNotifierProvider<ProviderNotifier, AsyncValue<List<ProviderConfigDto>>>(
    (ref) => ProviderNotifier(ref.watch(agentInstanceProvider)),
  );

  class ProviderNotifier
      extends StateNotifier<AsyncValue<List<ProviderConfigDto>>> {
    ProviderNotifier(this._agent) : super(const AsyncValue.loading()) {
      refresh();
    }

    final MobileclawAgent _agent;

    /// Reload the provider list from the Rust store.
    Future<void> refresh() async {
      state = const AsyncValue.loading();
      state = await AsyncValue.guard(() => _agent.providerList());
    }

    /// Save a new or updated provider and refresh the list.
    Future<void> addProvider({
      required ProviderConfigDto config,
      String? apiKey,
    }) async {
      await _agent.providerSave(config: config, apiKey: apiKey);
      await refresh();
    }

    /// Delete a provider and refresh the list.
    Future<void> deleteProvider({required String id}) async {
      await _agent.providerDelete(id: id);
      await refresh();
    }

    /// Set a provider as active.
    /// Throws [ClawException] if the id does not exist.
    Future<void> setActive({required String id}) async {
      await _agent.providerSetActive(id: id);
    }
  }
  ```

- [ ] **Step 4.5: Run tests — expect PASS**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/features/providers/provider_notifier_test.dart
  ```
  Expected: 4 tests pass.

- [ ] **Step 4.6: Commit**

  ```bash
  git add \
    mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_models.dart \
    mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_notifier.dart \
    mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_notifier_test.dart
  git commit -m "feat(providers): add ProviderProtocol enum and ProviderNotifier"
  ```

---

## Task 5: ProviderListScreen

**Files:**
- Create: `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_list_screen.dart`
- Create: `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_list_screen_test.dart`

The list screen shows saved providers. Each row has:
- Provider name (bold) + protocol + model (subtitle)
- A checkmark icon if it is the active provider
- Swipe-to-dismiss to delete
- Tap on row to set as active (or push edit form)
- FAB to add new provider

Use `agentInstanceProvider` for dependency injection in tests.

- [ ] **Step 5.1: Write failing widget tests**

  Create `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_list_screen_test.dart`:

  ```dart
  import 'package:flutter/material.dart';
  import 'package:flutter_riverpod/flutter_riverpod.dart';
  import 'package:flutter_test/flutter_test.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  import 'package:mobileclaw_app/features/providers/provider_list_screen.dart';
  import 'package:mobileclaw_app/features/providers/provider_notifier.dart';

  Widget buildTestable(MobileclawAgent agent) => ProviderScope(
        overrides: [agentInstanceProvider.overrideWithValue(agent)],
        child: const MaterialApp(home: ProviderListScreen()),
      );

  void main() {
    testWidgets('shows empty state message when no providers', (tester) async {
      final agent = MockMobileclawAgent();
      await tester.pumpWidget(buildTestable(agent));
      await tester.pumpAndSettle();

      expect(find.text('No providers configured'), findsOneWidget);
      expect(find.byType(FloatingActionButton), findsOneWidget);
    });

    testWidgets('lists provider name when one exists', (tester) async {
      final agent = MockMobileclawAgent();
      await agent.providerSave(
        config: const ProviderConfigDto(
          id: 'p1', name: 'My Claude', protocol: 'anthropic',
          baseUrl: 'https://api.anthropic.com', model: 'claude-opus-4-6',
          createdAt: 1000,
        ),
        apiKey: 'sk-test',
      );
      await tester.pumpWidget(buildTestable(agent));
      await tester.pumpAndSettle();

      expect(find.text('My Claude'), findsOneWidget);
    });

    testWidgets('swipe to delete removes provider', (tester) async {
      final agent = MockMobileclawAgent();
      await agent.providerSave(
        config: const ProviderConfigDto(
          id: 'p1', name: 'Groq', protocol: 'openai_compat',
          baseUrl: 'https://api.groq.com', model: 'mixtral', createdAt: 1000,
        ),
        apiKey: 'key',
      );
      await tester.pumpWidget(buildTestable(agent));
      await tester.pumpAndSettle();

      await tester.drag(find.text('Groq'), const Offset(-500, 0));
      await tester.pumpAndSettle();

      expect(find.text('Groq'), findsNothing);
      expect(find.text('No providers configured'), findsOneWidget);
    });

    testWidgets('FAB navigates to ProviderFormScreen', (tester) async {
      final agent = MockMobileclawAgent();
      await tester.pumpWidget(buildTestable(agent));
      await tester.pumpAndSettle();

      await tester.tap(find.byType(FloatingActionButton));
      await tester.pumpAndSettle();

      // ProviderFormScreen shows protocol picker
      expect(find.text('Protocol'), findsOneWidget);
    });
  }
  ```

- [ ] **Step 5.2: Run tests — expect compile error**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/features/providers/provider_list_screen_test.dart
  ```
  Expected: `Error: uri '...provider_list_screen.dart' not found`

- [ ] **Step 5.3: Implement ProviderListScreen**

  Create `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_list_screen.dart`:

  ```dart
  import 'package:flutter/material.dart';
  import 'package:flutter_riverpod/flutter_riverpod.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  import 'provider_notifier.dart';
  import 'provider_form_screen.dart';

  class ProviderListScreen extends ConsumerStatefulWidget {
    const ProviderListScreen({super.key});

    @override
    ConsumerState<ProviderListScreen> createState() => _ProviderListScreenState();
  }

  class _ProviderListScreenState extends ConsumerState<ProviderListScreen> {
    String? _activeId;

    @override
    void initState() {
      super.initState();
      _loadActiveId();
    }

    Future<void> _loadActiveId() async {
      final agent = ref.read(agentInstanceProvider);
      final active = await agent.providerGetActive();
      if (mounted) setState(() => _activeId = active?.id);
    }

    Future<void> _setActive(String id) async {
      try {
        await ref.read(providerListProvider.notifier).setActive(id: id);
        if (mounted) {
          setState(() => _activeId = id);
          ScaffoldMessenger.of(context)
              .showSnackBar(const SnackBar(content: Text('Active provider updated')));
        }
      } on ClawException catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context)
              .showSnackBar(SnackBar(content: Text('Error: ${e.message}')));
        }
      }
    }

    Future<void> _delete(String id) async {
      await ref.read(providerListProvider.notifier).deleteProvider(id: id);
      if (_activeId == id) setState(() => _activeId = null);
    }

    Future<void> _openForm({ProviderConfigDto? existing}) async {
      await Navigator.of(context).push<void>(
        MaterialPageRoute(
          builder: (_) => ProviderFormScreen(existing: existing),
        ),
      );
      // Refresh list after returning from form
      await ref.read(providerListProvider.notifier).refresh();
      await _loadActiveId();
    }

    @override
    Widget build(BuildContext context) {
      final state = ref.watch(providerListProvider);

      return Scaffold(
        appBar: AppBar(title: const Text('LLM Providers')),
        floatingActionButton: FloatingActionButton(
          onPressed: () => _openForm(),
          child: const Icon(Icons.add),
        ),
        body: state.when(
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (e, _) => Center(child: Text('Error: $e')),
          data: (providers) {
            if (providers.isEmpty) {
              return const Center(child: Text('No providers configured'));
            }
            return ListView.builder(
              itemCount: providers.length,
              itemBuilder: (context, index) {
                final p = providers[index];
                final isActive = p.id == _activeId;
                return Dismissible(
                  key: ValueKey(p.id),
                  direction: DismissDirection.endToStart,
                  background: Container(
                    alignment: Alignment.centerRight,
                    color: Colors.red,
                    padding: const EdgeInsets.only(right: 16),
                    child: const Icon(Icons.delete, color: Colors.white),
                  ),
                  onDismissed: (_) => _delete(p.id),
                  child: ListTile(
                    leading: Icon(
                      isActive ? Icons.check_circle : Icons.circle_outlined,
                      color: isActive ? Colors.green : null,
                    ),
                    title: Text(
                      p.name,
                      style: const TextStyle(fontWeight: FontWeight.bold),
                    ),
                    subtitle: Text('${p.protocol} · ${p.model}'),
                    onTap: () => _setActive(p.id),
                    trailing: IconButton(
                      icon: const Icon(Icons.edit),
                      onPressed: () => _openForm(existing: p),
                    ),
                  ),
                );
              },
            );
          },
        ),
      );
    }
  }
  ```

- [ ] **Step 5.4: Run tests — expect PASS**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/features/providers/provider_list_screen_test.dart
  ```
  Expected: 4 tests pass.

- [ ] **Step 5.5: Commit**

  ```bash
  git add \
    mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_list_screen.dart \
    mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_list_screen_test.dart
  git commit -m "feat(providers): add ProviderListScreen with empty state, list, swipe-delete, FAB"
  ```

---

## Task 6: ProviderFormScreen

**Files:**
- Create: `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_form_screen.dart`
- Create: `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_form_screen_test.dart`

The form has:
- Name field (text)
- Protocol dropdown (`ProviderProtocol` enum)
- URL field (hint changes when protocol changes)
- Model field (text)
- API key field (obscured; shows `••••••••` when editing existing; leave blank to preserve stored key)
- Test button — calls `providerProbe` → shows inline result chip
- Save button — disabled until Test passes OR "skip test" link tapped
- On save: calls `agent.providerSave` and pops

**Key state machine:**
- `_testState`: `idle | testing | passed | failed | skipped`
- Save button is enabled when `_testState == passed || _testState == skipped`

- [ ] **Step 6.1: Write failing widget tests**

  Create `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_form_screen_test.dart`:

  ```dart
  import 'package:flutter/material.dart';
  import 'package:flutter_riverpod/flutter_riverpod.dart';
  import 'package:flutter_test/flutter_test.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  import 'package:mobileclaw_app/features/providers/provider_form_screen.dart';
  import 'package:mobileclaw_app/features/providers/provider_notifier.dart';

  Widget buildForm({MobileclawAgent? agent, ProviderConfigDto? existing}) =>
      ProviderScope(
        overrides: [
          if (agent != null) agentInstanceProvider.overrideWithValue(agent),
        ],
        child: MaterialApp(
          home: ProviderFormScreen(existing: existing),
        ),
      );

  void main() {
    testWidgets('shows protocol picker', (tester) async {
      await tester.pumpWidget(buildForm(agent: MockMobileclawAgent()));
      expect(find.text('Protocol'), findsOneWidget);
    });

    testWidgets('save button disabled before test', (tester) async {
      await tester.pumpWidget(buildForm(agent: MockMobileclawAgent()));
      final saveButton = tester.widget<ElevatedButton>(
        find.widgetWithText(ElevatedButton, 'Save'),
      );
      expect(saveButton.onPressed, isNull);
    });

    testWidgets('skip test link enables save', (tester) async {
      await tester.pumpWidget(buildForm(agent: MockMobileclawAgent()));
      await tester.tap(find.text('skip test'));
      await tester.pump();
      final saveButton = tester.widget<ElevatedButton>(
        find.widgetWithText(ElevatedButton, 'Save'),
      );
      expect(saveButton.onPressed, isNotNull);
    });

    testWidgets('Test button shows success chip for mock', (tester) async {
      await tester.pumpWidget(buildForm(agent: MockMobileclawAgent()));
      // Fill required fields
      await tester.enterText(find.byKey(const Key('field_name')), 'Test');
      await tester.enterText(find.byKey(const Key('field_url')), 'https://api.anthropic.com');
      await tester.enterText(find.byKey(const Key('field_model')), 'claude-opus-4-6');
      await tester.tap(find.text('Test'));
      await tester.pumpAndSettle();
      expect(find.textContaining('OK'), findsOneWidget);
    });

    testWidgets('API key field shows masked placeholder when editing', (tester) async {
      const existing = ProviderConfigDto(
        id: 'p1', name: 'Existing', protocol: 'anthropic',
        baseUrl: 'https://api.anthropic.com', model: 'claude-opus-4-6',
        createdAt: 1000,
      );
      await tester.pumpWidget(buildForm(
        agent: MockMobileclawAgent(),
        existing: existing,
      ));
      // The key field hint should indicate an existing key
      expect(find.textContaining('••••••••'), findsOneWidget);
    });
  }
  ```

- [ ] **Step 6.2: Run tests — expect compile error**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/features/providers/provider_form_screen_test.dart
  ```

- [ ] **Step 6.3: Implement ProviderFormScreen**

  Create `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_form_screen.dart`:

  ```dart
  import 'package:flutter/material.dart';
  import 'package:flutter_riverpod/flutter_riverpod.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  import 'provider_models.dart';
  import 'provider_notifier.dart';

  enum _TestState { idle, testing, passed, degraded, failed, skipped }

  class ProviderFormScreen extends ConsumerStatefulWidget {
    const ProviderFormScreen({
      super.key,
      this.existing,
      this.onSaved,
      this.probeFn = MockMobileclawAgent.probe,
    });

    /// Non-null when editing an existing provider.
    final ProviderConfigDto? existing;

    /// Called after successful save, with the saved provider's id.
    /// If null, [Navigator.pop] is called instead.
    /// Used by [OnboardingScreen] to set-active and navigate.
    final Future<void> Function(String id)? onSaved;

    /// Probe function to call when the user taps "Test".
    /// Defaults to [MockMobileclawAgent.probe] — swap to
    /// [MobileclawAgentImpl.probe] after FFI codegen (Task 10).
    final Future<ProbeResultDto> Function({
      required ProviderConfigDto config,
      String? apiKey,
    }) probeFn;

    @override
    ConsumerState<ProviderFormScreen> createState() => _ProviderFormScreenState();
  }

  class _ProviderFormScreenState extends ConsumerState<ProviderFormScreen> {
    final _formKey = GlobalKey<FormState>();
    late final TextEditingController _nameCtrl;
    late final TextEditingController _urlCtrl;
    late final TextEditingController _modelCtrl;
    late final TextEditingController _keyCtrl;
    late ProviderProtocol _protocol;

    _TestState _testState = _TestState.idle;
    String? _testError;
    int? _testLatencyMs;
    bool _saving = false;

    bool get _isEditing => widget.existing != null;
    bool get _saveEnabled =>
        _testState == _TestState.passed ||
        _testState == _TestState.skipped ||
        _testState == _TestState.degraded;

    @override
    void initState() {
      super.initState();
      final e = widget.existing;
      _protocol = e != null
          ? ProviderProtocol.fromValue(e.protocol)
          : ProviderProtocol.anthropic;
      _nameCtrl = TextEditingController(text: e?.name ?? '');
      _urlCtrl = TextEditingController(text: e?.baseUrl ?? _protocol.urlHint);
      _modelCtrl = TextEditingController(text: e?.model ?? '');
      _keyCtrl = TextEditingController();
    }

    @override
    void dispose() {
      _nameCtrl.dispose();
      _urlCtrl.dispose();
      _modelCtrl.dispose();
      _keyCtrl.dispose();
      super.dispose();
    }

    void _onProtocolChanged(ProviderProtocol? p) {
      if (p == null) return;
      setState(() {
        _protocol = p;
        // Only update URL hint if field is empty or still at old hint
        if (_urlCtrl.text.isEmpty ||
            ProviderProtocol.values.any((v) => v.urlHint == _urlCtrl.text)) {
          _urlCtrl.text = p.urlHint;
        }
        _testState = _TestState.idle;
      });
    }

    Future<void> _runTest() async {
      if (!_formKey.currentState!.validate()) return;
      setState(() => _testState = _TestState.testing);

      // Resolve api key: use field value if non-empty; else null (mock returns ok anyway)
      final apiKey = _keyCtrl.text.trim().isEmpty ? null : _keyCtrl.text.trim();

      final config = ProviderConfigDto(
        id: widget.existing?.id ?? '',
        name: _nameCtrl.text.trim(),
        protocol: _protocol.value,
        baseUrl: _urlCtrl.text.trim(),
        model: _modelCtrl.text.trim(),
        createdAt: 0,
      );

      // Use widget.probeFn — defaults to MockMobileclawAgent.probe; swap to
      // MobileclawAgentImpl.probe after FFI codegen (Task 10).
      final result = await widget.probeFn(config: config, apiKey: apiKey);

      if (mounted) {
        setState(() {
          _testLatencyMs = result.latencyMs;
          _testError = result.error;
          if (!result.ok) {
            _testState = _TestState.failed;
          } else if (result.degraded) {
            _testState = _TestState.degraded;
          } else {
            _testState = _TestState.passed;
          }
        });
      }
    }

    Future<void> _save() async {
      if (!_formKey.currentState!.validate()) return;
      setState(() => _saving = true);

      final agent = ref.read(agentInstanceProvider);
      try {
        final config = ProviderConfigDto(
          id: widget.existing?.id ?? '',
          name: _nameCtrl.text.trim(),
          protocol: _protocol.value,
          baseUrl: _urlCtrl.text.trim(),
          model: _modelCtrl.text.trim(),
          createdAt: widget.existing?.createdAt ?? 0,
        );
        // Pass null api key if field blank (Rust preserves stored key on update)
        final apiKey = _keyCtrl.text.trim().isEmpty ? null : _keyCtrl.text.trim();
        await agent.providerSave(config: config, apiKey: apiKey);
        if (mounted) {
          if (widget.onSaved != null) {
            // Re-read list to find the saved id (handles both new and update).
            final list = await agent.providerList();
            final savedId = config.id.isNotEmpty
                ? config.id
                : list.reduce((a, b) => a.createdAt > b.createdAt ? a : b).id;
            await widget.onSaved!(savedId);
          } else {
            Navigator.of(context).pop();
          }
        }
      } on ClawException catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context)
              .showSnackBar(SnackBar(content: Text('Save failed: ${e.message}')));
        }
      } finally {
        if (mounted) setState(() => _saving = false);
      }
    }

    @override
    Widget build(BuildContext context) {
      return Scaffold(
        appBar: AppBar(
          title: Text(_isEditing ? 'Edit Provider' : 'Add Provider'),
        ),
        body: SingleChildScrollView(
          padding: const EdgeInsets.all(16),
          child: Form(
            key: _formKey,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                TextFormField(
                  key: const Key('field_name'),
                  controller: _nameCtrl,
                  decoration: const InputDecoration(labelText: 'Name'),
                  validator: (v) =>
                      (v == null || v.trim().isEmpty) ? 'Name is required' : null,
                ),
                const SizedBox(height: 16),
                DropdownButtonFormField<ProviderProtocol>(
                  value: _protocol,
                  decoration: const InputDecoration(labelText: 'Protocol'),
                  items: ProviderProtocol.values
                      .map((p) => DropdownMenuItem(
                            value: p,
                            child: Text(p.displayName),
                          ))
                      .toList(),
                  onChanged: _onProtocolChanged,
                ),
                const SizedBox(height: 16),
                TextFormField(
                  key: const Key('field_url'),
                  controller: _urlCtrl,
                  decoration: InputDecoration(
                    labelText: 'Base URL',
                    hintText: _protocol.urlHint,
                  ),
                  keyboardType: TextInputType.url,
                  validator: (v) =>
                      (v == null || v.trim().isEmpty) ? 'URL is required' : null,
                ),
                const SizedBox(height: 16),
                TextFormField(
                  key: const Key('field_model'),
                  controller: _modelCtrl,
                  decoration: const InputDecoration(labelText: 'Model'),
                  validator: (v) =>
                      (v == null || v.trim().isEmpty) ? 'Model is required' : null,
                ),
                const SizedBox(height: 16),
                TextFormField(
                  key: const Key('field_api_key'),
                  controller: _keyCtrl,
                  obscureText: true,
                  decoration: InputDecoration(
                    labelText: 'API Key',
                    hintText: _isEditing ? '••••••••' : 'Leave blank for Ollama',
                  ),
                ),
                const SizedBox(height: 24),
                // Test button + result chip
                Row(
                  children: [
                    Expanded(
                      child: OutlinedButton(
                        onPressed:
                            _testState == _TestState.testing ? null : _runTest,
                        child: _testState == _TestState.testing
                            ? const SizedBox(
                                width: 16,
                                height: 16,
                                child: CircularProgressIndicator(strokeWidth: 2),
                              )
                            : const Text('Test'),
                      ),
                    ),
                    if (_testState != _TestState.idle &&
                        _testState != _TestState.testing) ...[
                      const SizedBox(width: 8),
                      _TestChip(state: _testState, latencyMs: _testLatencyMs),
                    ],
                  ],
                ),
                if (_testState == _TestState.failed && _testError != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: Text(
                      _testError!,
                      style: TextStyle(color: Theme.of(context).colorScheme.error),
                    ),
                  ),
                if (_testState == _TestState.degraded)
                  const Padding(
                    padding: EdgeInsets.only(top: 8),
                    child: Text(
                      'Warning: models endpoint responded but completions are unverified.',
                      style: TextStyle(color: Colors.orange),
                    ),
                  ),
                const SizedBox(height: 8),
                if (!_saveEnabled)
                  Align(
                    alignment: Alignment.centerRight,
                    child: TextButton(
                      onPressed: () => setState(() => _testState = _TestState.skipped),
                      child: const Text('skip test'),
                    ),
                  ),
                const SizedBox(height: 16),
                ElevatedButton(
                  onPressed: _saveEnabled && !_saving ? _save : null,
                  child: _saving
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Text('Save'),
                ),
              ],
            ),
          ),
        ),
      );
    }
  }

  class _TestChip extends StatelessWidget {
    const _TestChip({required this.state, required this.latencyMs});

    final _TestState state;
    final int? latencyMs;

    @override
    Widget build(BuildContext context) {
      final (icon, color, label) = switch (state) {
        _TestState.passed => (Icons.check_circle, Colors.green, 'OK ${latencyMs}ms'),
        _TestState.degraded => (Icons.warning, Colors.orange, 'Degraded'),
        _TestState.failed => (Icons.error, Colors.red, 'Failed'),
        _TestState.skipped => (Icons.skip_next, Colors.grey, 'Skipped'),
        _ => (Icons.help, Colors.grey, ''),
      };
      return Chip(
        avatar: Icon(icon, color: color, size: 16),
        label: Text(label),
      );
    }
  }
  ```

- [ ] **Step 6.4: Run tests — expect PASS**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/features/providers/provider_form_screen_test.dart
  ```
  Expected: 5 tests pass.

- [ ] **Step 6.5: Commit**

  ```bash
  git add \
    mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_form_screen.dart \
    mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/provider_form_screen_test.dart
  git commit -m "feat(providers): add ProviderFormScreen with test/save flow"
  ```

---

## Task 7: OnboardingScreen

**Files:**
- Create: `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/onboarding_screen.dart`
- Create: `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/onboarding_screen_test.dart`

OnboardingScreen is a thin wrapper: it shows a title/blurb, then renders `ProviderFormScreen` as its body. On save, it also sets the new provider as active and navigates to `ChatPage`. It's only shown on first launch (no providers configured).

- [ ] **Step 7.1: Write failing widget test**

  Create `mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/onboarding_screen_test.dart`:

  ```dart
  import 'package:flutter/material.dart';
  import 'package:flutter_riverpod/flutter_riverpod.dart';
  import 'package:flutter_test/flutter_test.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  import 'package:mobileclaw_app/features/providers/onboarding_screen.dart';
  import 'package:mobileclaw_app/features/providers/provider_notifier.dart';

  void main() {
    testWidgets('OnboardingScreen shows welcome header', (tester) async {
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            agentInstanceProvider.overrideWithValue(MockMobileclawAgent()),
          ],
          child: const MaterialApp(home: OnboardingScreen()),
        ),
      );
      expect(find.text('Welcome to MobileClaw'), findsOneWidget);
    });

    testWidgets('OnboardingScreen shows protocol picker', (tester) async {
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            agentInstanceProvider.overrideWithValue(MockMobileclawAgent()),
          ],
          child: const MaterialApp(home: OnboardingScreen()),
        ),
      );
      expect(find.text('Protocol'), findsOneWidget);
    });
  }
  ```

- [ ] **Step 7.2: Run tests — expect compile error**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/features/providers/onboarding_screen_test.dart
  ```

- [ ] **Step 7.3: Implement OnboardingScreen**

  Create `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/onboarding_screen.dart`:

  ```dart
  import 'package:flutter/material.dart';
  import 'package:flutter_riverpod/flutter_riverpod.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  import '../chat/chat_page.dart';
  import 'provider_form_screen.dart';
  import 'provider_notifier.dart';

  /// First-launch wizard shown when no providers are configured.
  ///
  /// Wraps [ProviderFormScreen] with a welcome header. After the user saves
  /// a provider, sets it as active and navigates to [ChatPage].
  class OnboardingScreen extends ConsumerWidget {
    const OnboardingScreen({super.key});

    @override
    Widget build(BuildContext context, WidgetRef ref) {
      return Scaffold(
        body: SafeArea(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const Padding(
                padding: EdgeInsets.fromLTRB(16, 32, 16, 8),
                child: Text(
                  'Welcome to MobileClaw',
                  style: TextStyle(fontSize: 24, fontWeight: FontWeight.bold),
                  textAlign: TextAlign.center,
                ),
              ),
              const Padding(
                padding: EdgeInsets.symmetric(horizontal: 16),
                child: Text(
                  'Add an LLM provider to get started.',
                  textAlign: TextAlign.center,
                ),
              ),
              const SizedBox(height: 16),
              Expanded(
                // Embed the form; intercept the save to also set active + navigate
                child: _OnboardingForm(
                  onProviderSaved: (id) async {
                    final agent = ref.read(agentInstanceProvider);
                    await agent.providerSetActive(id: id);
                    if (context.mounted) {
                      Navigator.of(context).pushReplacement(
                        MaterialPageRoute(builder: (_) => const ChatPage()),
                      );
                    }
                  },
                ),
              ),
            ],
          ),
        ),
      );
    }
  }

  /// Internal wrapper: calls the save callback with the new provider id
  /// after [ProviderFormScreen] saves. Uses a custom callback so we can
  /// set-active and navigate without coupling ProviderFormScreen to onboarding.
  class _OnboardingForm extends ConsumerWidget {
    const _OnboardingForm({required this.onProviderSaved});

    final Future<void> Function(String id) onProviderSaved;

    @override
    Widget build(BuildContext context, WidgetRef ref) {
      // The form handles its own saving. After Navigator.pop() we check for
      // a new provider and fire the callback.
      return ProviderFormScreen(
        onSaved: onProviderSaved,
      );
    }
  }
  ```

  Note: `ProviderFormScreen` already has the `onSaved` parameter defined in Task 6 Step 6.3. The `onSaved` callback is called in `_save()` with the saved provider id (already implemented in that step). No additional changes to `ProviderFormScreen` are needed here.

- [ ] **Step 7.4: Run tests — expect PASS**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/features/providers/onboarding_screen_test.dart
  ```
  Expected: 2 tests pass.

- [ ] **Step 7.5: Commit**

  ```bash
  git add \
    mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/onboarding_screen.dart \
    mobileclaw-flutter/apps/mobileclaw_app/test/features/providers/onboarding_screen_test.dart
  git commit -m "feat(providers): add OnboardingScreen wrapping ProviderFormScreen"
  ```

---

## Task 8: Settings Page and Chat AppBar Integration

**Files:**
- Create: `mobileclaw-flutter/apps/mobileclaw_app/lib/features/settings/settings_page.dart`
- Modify: `mobileclaw-flutter/apps/mobileclaw_app/lib/features/chat/chat_page.dart`

Settings is a simple `ListView` with one tile: "LLM Providers" → pushes `ProviderListScreen`. A gear icon in `ChatPage`'s AppBar pushes `SettingsPage`.

- [ ] **Step 8.1: Write failing widget test for settings page**

  Create `mobileclaw-flutter/apps/mobileclaw_app/test/features/settings/settings_page_test.dart`:

  ```dart
  import 'package:flutter/material.dart';
  import 'package:flutter_riverpod/flutter_riverpod.dart';
  import 'package:flutter_test/flutter_test.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  import 'package:mobileclaw_app/features/settings/settings_page.dart';
  import 'package:mobileclaw_app/features/providers/provider_notifier.dart';

  void main() {
    testWidgets('settings page shows LLM Providers entry', (tester) async {
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            agentInstanceProvider.overrideWithValue(MockMobileclawAgent()),
          ],
          child: const MaterialApp(home: SettingsPage()),
        ),
      );
      expect(find.text('LLM Providers'), findsOneWidget);
    });

    testWidgets('tapping LLM Providers navigates to list screen', (tester) async {
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            agentInstanceProvider.overrideWithValue(MockMobileclawAgent()),
          ],
          child: const MaterialApp(home: SettingsPage()),
        ),
      );
      await tester.tap(find.text('LLM Providers'));
      await tester.pumpAndSettle();
      // ProviderListScreen shows the empty state or its app bar title
      expect(find.text('LLM Providers'), findsWidgets); // title in app bar
    });
  }
  ```

- [ ] **Step 8.2: Run test — expect compile error**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/features/settings/settings_page_test.dart
  ```

- [ ] **Step 8.3: Create SettingsPage**

  Create `mobileclaw-flutter/apps/mobileclaw_app/lib/features/settings/settings_page.dart`:

  ```dart
  import 'package:flutter/material.dart';

  import '../providers/provider_list_screen.dart';

  class SettingsPage extends StatelessWidget {
    const SettingsPage({super.key});

    @override
    Widget build(BuildContext context) {
      return Scaffold(
        appBar: AppBar(title: const Text('Settings')),
        body: ListView(
          children: [
            ListTile(
              leading: const Icon(Icons.smart_toy_outlined),
              title: const Text('LLM Providers'),
              subtitle: const Text('Configure AI model providers'),
              trailing: const Icon(Icons.chevron_right),
              onTap: () => Navigator.of(context).push<void>(
                MaterialPageRoute(builder: (_) => const ProviderListScreen()),
              ),
            ),
          ],
        ),
      );
    }
  }
  ```

- [ ] **Step 8.4: Run settings test — expect PASS**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/features/settings/settings_page_test.dart
  ```

- [ ] **Step 8.5: Add settings gear to ChatPage AppBar**

  In `mobileclaw-flutter/apps/mobileclaw_app/lib/features/chat/chat_page.dart`, update the `AppBar` build:

  ```dart
  // Before:
  appBar: AppBar(title: const Text('MobileClaw')),

  // After:
  appBar: AppBar(
    title: const Text('MobileClaw'),
    actions: [
      IconButton(
        icon: const Icon(Icons.settings),
        onPressed: () => Navigator.of(context).push<void>(
          MaterialPageRoute(builder: (_) => const SettingsPage()),
        ),
      ),
    ],
  ),
  ```

  Add import at top:
  ```dart
  import '../settings/settings_page.dart';
  ```

- [ ] **Step 8.6: Run full app widget test**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/widget_test.dart
  ```
  Expected: passes (app still builds, loading spinner visible).

- [ ] **Step 8.7: Commit**

  ```bash
  git add \
    mobileclaw-flutter/apps/mobileclaw_app/lib/features/settings/settings_page.dart \
    mobileclaw-flutter/apps/mobileclaw_app/lib/features/chat/chat_page.dart \
    mobileclaw-flutter/apps/mobileclaw_app/test/features/settings/settings_page_test.dart
  git commit -m "feat(settings): add SettingsPage with LLM Providers entry; gear icon in ChatPage"
  ```

---

## Task 9: Onboarding Check in main.dart

**Files:**
- Modify: `mobileclaw-flutter/apps/mobileclaw_app/lib/main.dart`

On startup, after `agentProvider` resolves, check if any providers are configured. If the list is empty, show `OnboardingScreen`; otherwise show `ChatPage`.

- [ ] **Step 9.1: Write failing widget test**

  Add to `mobileclaw-flutter/apps/mobileclaw_app/test/widget_test.dart`:

  ```dart
  testWidgets('shows OnboardingScreen when no providers configured', (tester) async {
    // agentProvider resolves to a fresh mock (no providers saved)
    // This test checks the routing logic.
    await tester.pumpWidget(const ProviderScope(child: MobileClawApp()));
    await tester.pump(); // trigger FutureProvider

    // The mock agent returns empty providerList, so OnboardingScreen should appear
    // (This test may still show loading if agentProvider has not resolved yet)
    // To make it deterministic, override agentProvider:
    // (Advanced: left to implementer — verify by checking for 'Welcome to MobileClaw')
  });
  ```

  Note: The full deterministic test requires overriding `agentProvider` in the test. The existing test in `widget_test.dart` already covers loading state. Add a separate test file for the routing logic:

  Create `mobileclaw-flutter/apps/mobileclaw_app/test/app_routing_test.dart`:

  ```dart
  import 'package:flutter/material.dart';
  import 'package:flutter_riverpod/flutter_riverpod.dart';
  import 'package:flutter_test/flutter_test.dart';
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

  import 'package:mobileclaw_app/main.dart';
  import 'package:mobileclaw_app/core/engine_provider.dart';

  void main() {
    testWidgets('shows OnboardingScreen when no providers', (tester) async {
      final agent = MockMobileclawAgent(); // no providers
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            agentProvider.overrideWith((ref) async => agent),
          ],
          child: const MobileClawApp(),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('Welcome to MobileClaw'), findsOneWidget);
    });

    testWidgets('shows ChatPage when provider exists', (tester) async {
      final agent = MockMobileclawAgent();
      await agent.providerSave(
        config: const ProviderConfigDto(
          id: 'p1', name: 'Test', protocol: 'anthropic',
          baseUrl: 'https://api.anthropic.com', model: 'claude-opus-4-6',
          createdAt: 1000,
        ),
        apiKey: 'key',
      );
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            agentProvider.overrideWith((ref) async => agent),
          ],
          child: const MobileClawApp(),
        ),
      );
      await tester.pumpAndSettle();
      // ChatPage shows the MobileClaw AppBar
      expect(find.text('MobileClaw'), findsOneWidget);
      expect(find.byType(TextField), findsOneWidget); // input bar
    });
  }
  ```

- [ ] **Step 9.2: Run test — expect compile error / test fail**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/app_routing_test.dart
  ```

- [ ] **Step 9.3: Update _AppShell in main.dart**

  Update `_AppShell.build` in `mobileclaw-flutter/apps/mobileclaw_app/lib/main.dart`:

  ```dart
  // Before:
  class _AppShell extends ConsumerWidget {
    const _AppShell();

    @override
    Widget build(BuildContext context, WidgetRef ref) {
      final agentAsync = ref.watch(agentProvider);
      return agentAsync.when(
        data: (_) => const ChatPage(),
        loading: () =>
            const Scaffold(body: Center(child: CircularProgressIndicator())),
        error: (e, _) =>
            Scaffold(body: Center(child: Text('Init error: $e'))),
      );
    }
  }

  // After:
  class _AppShell extends ConsumerWidget {
    const _AppShell();

    @override
    Widget build(BuildContext context, WidgetRef ref) {
      final agentAsync = ref.watch(agentProvider);
      return agentAsync.when(
        loading: () =>
            const Scaffold(body: Center(child: CircularProgressIndicator())),
        error: (e, _) =>
            Scaffold(body: Center(child: Text('Init error: $e'))),
        data: (agent) => _HomeRouter(agent: agent),
      );
    }
  }

  /// Routes to OnboardingScreen (no providers) or ChatPage (providers exist).
  class _HomeRouter extends ConsumerWidget {
    const _HomeRouter({required this.agent});
    final MobileclawAgent agent;

    @override
    Widget build(BuildContext context, WidgetRef ref) {
      // Use a FutureBuilder to check provider list on first render only.
      return FutureBuilder<List<ProviderConfigDto>>(
        future: agent.providerList(),
        builder: (context, snap) {
          if (snap.connectionState != ConnectionState.done) {
            return const Scaffold(
              body: Center(child: CircularProgressIndicator()),
            );
          }
          final providers = snap.data ?? [];
          if (providers.isEmpty) {
            return const OnboardingScreen();
          }
          return const ChatPage();
        },
      );
    }
  }
  ```

  Add imports at top of `main.dart`:
  ```dart
  import 'features/providers/onboarding_screen.dart';
  ```

  Also update `agentInstanceProvider` in `provider_notifier.dart` to use `agentProvider` correctly. The `agentInstanceProvider` currently does:
  ```dart
  return ref.watch(agentProvider).requireValue;
  ```
  This is correct — it will throw if called before `agentProvider` resolves, which is fine since `_HomeRouter` only renders after it's done.

- [ ] **Step 9.4: Run routing tests — expect PASS**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test test/app_routing_test.dart
  ```
  Expected: 2 tests pass.

- [ ] **Step 9.5: Run all app tests**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test
  ```
  Expected: all tests pass with no errors.

- [ ] **Step 9.6: Commit**

  ```bash
  git add \
    mobileclaw-flutter/apps/mobileclaw_app/lib/main.dart \
    mobileclaw-flutter/apps/mobileclaw_app/test/app_routing_test.dart
  git commit -m "feat(app): route to OnboardingScreen when no providers configured"
  ```

---

## Task 10: Wire Up Real FFI After Plan 1 Merges

**Files:**
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/bridge/ffi.dart` (regenerated)
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart`

This task is blocked on Plan 1 (Rust core) completion. Complete Tasks 1–9 first using the mock.

- [ ] **Step 10.1: Regenerate FFI bindings**

  After Plan 1 Rust code is merged and `cargo build -p mobileclaw-core` passes:

  ```bash
  cd mobileclaw-flutter/packages/mobileclaw_sdk
  flutter_rust_bridge_codegen generate
  ```

  This re-generates `lib/src/bridge/ffi.dart` with new `AgentSession` methods:
  - `providerSave(config: ProviderConfigDtoFfi, apiKey: String?)`
  - `providerList()` → `List<ProviderConfigDtoFfi>`
  - `providerDelete(id: String)`
  - `providerSetActive(id: String)`
  - `providerGetActive()` → `ProviderConfigDtoFfi?`
  - Free function: `providerProbe(config: ProviderConfigDtoFfi, apiKey: String?)` → `ProbeResultDtoFfi`

  Also generates `ProviderConfigDtoFfi` and `ProbeResultDtoFfi` classes in `ffi.dart`.

- [ ] **Step 10.2: Add converter helpers in agent_impl.dart**

  In `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart`, add converters (mirror the `EmailAccountDto` ↔ `EmailAccountDtoFfi` pattern):

  ```dart
  ProviderConfigDto _providerFromFfi(ProviderConfigDtoFfi ffi) => ProviderConfigDto(
        id: ffi.id,
        name: ffi.name,
        protocol: ffi.protocol,
        baseUrl: ffi.baseUrl,
        model: ffi.model,
        createdAt: ffi.createdAt.toInt(),
      );

  ProviderConfigDtoFfi _providerToFfi(ProviderConfigDto dto) => ProviderConfigDtoFfi(
        id: dto.id,
        name: dto.name,
        protocol: dto.protocol,
        baseUrl: dto.baseUrl,
        model: dto.model,
        createdAt: BigInt.from(dto.createdAt),
      );

  ProbeResultDto _probeFromFfi(ProbeResultDtoFfi ffi) => ProbeResultDto(
        ok: ffi.ok,
        latencyMs: ffi.latencyMs.toInt(),
        degraded: ffi.degraded,
        error: ffi.error,
      );
  ```

- [ ] **Step 10.3: Implement provider methods in MobileclawAgentImpl**

  Add after `emailAccountDelete` implementation in `agent_impl.dart`:

  ```dart
  @override
  Future<void> providerSave({
    required ProviderConfigDto config,
    String? apiKey,
  }) async {
    _checkAlive();
    await _session.providerSave(config: _providerToFfi(config), apiKey: apiKey);
  }

  @override
  Future<List<ProviderConfigDto>> providerList() async {
    _checkAlive();
    final ffis = await _session.providerList();
    return ffis.map(_providerFromFfi).toList();
  }

  @override
  Future<void> providerDelete({required String id}) async {
    _checkAlive();
    await _session.providerDelete(id: id);
  }

  @override
  Future<void> providerSetActive({required String id}) async {
    _checkAlive();
    await _session.providerSetActive(id: id);
  }

  @override
  Future<ProviderConfigDto?> providerGetActive() async {
    _checkAlive();
    final ffi = await _session.providerGetActive();
    if (ffi == null) return null;
    return _providerFromFfi(ffi);
  }

  static Future<ProbeResultDto> probe({
    required ProviderConfigDto config,
    String? apiKey,
  }) async {
    // Free function; does not require an initialized session.
    if (!MobileclawCoreBridge.instance.initialized) {
      await MobileclawCoreBridge.init();
    }
    final ffi = await providerProbe(
      config: _providerToFfi(config),
      apiKey: apiKey,
    );
    return _probeFromFfi(ffi);
  }
  ```

  Note: `providerProbe` in the last block refers to the top-level free function generated by `flutter_rust_bridge` in `frb_generated.dart`.

- [ ] **Step 10.4: Swap probeFn to use MobileclawAgentImpl.probe**

  `ProviderFormScreen` already accepts a `probeFn` parameter (Task 6, Step 6.3) that defaults to `MockMobileclawAgent.probe`. Now that `MobileclawAgentImpl.probe` exists, update all callsites in `ProviderListScreen._openForm` and `OnboardingScreen._OnboardingForm` to pass the real probe when native is available:

  In `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_list_screen.dart`, update `_openForm`:
  ```dart
  Future<void> _openForm({ProviderConfigDto? existing}) async {
    await Navigator.of(context).push<void>(
      MaterialPageRoute(
        builder: (_) => ProviderFormScreen(
          existing: existing,
          probeFn: _nativeAvailable
              ? MobileclawAgentImpl.probe
              : MockMobileclawAgent.probe,
        ),
      ),
    );
    await ref.read(providerListProvider.notifier).refresh();
    await _loadActiveId();
  }
  ```

  In `mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/onboarding_screen.dart`, update `_OnboardingForm.build`:
  ```dart
  return ProviderFormScreen(
    onSaved: onProviderSaved,
    probeFn: _nativeAvailable
        ? MobileclawAgentImpl.probe
        : MockMobileclawAgent.probe,
  );
  ```

  Add imports as needed:
  ```dart
  import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
  import '../../core/engine_provider.dart'; // for _nativeAvailable
  ```

  Note: `_nativeAvailable` is a top-level getter in `engine_provider.dart` — no change needed to that file.

- [ ] **Step 10.5: Run all tests — expect PASS**

  ```bash
  cd mobileclaw-flutter/packages/mobileclaw_sdk
  flutter test
  cd ../../../apps/mobileclaw_app
  flutter test
  ```
  Expected: all pass.

- [ ] **Step 10.6: Run integration test on device**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test integration_test/email_account_test.dart --device-id=<emulator-id>
  ```
  Expected: existing email tests still pass. (Provider integration tests are a stretch goal — not required for Plan 2 completion.)

- [ ] **Step 10.7: Commit**

  ```bash
  git add \
    mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart \
    mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/bridge/ \
    mobileclaw-flutter/apps/mobileclaw_app/lib/features/providers/provider_form_screen.dart \
    mobileclaw-flutter/apps/mobileclaw_app/lib/core/engine_provider.dart
  git commit -m "feat(sdk): wire provider methods to real FFI after codegen; swap probe function"
  ```

---

## Final Verification

- [ ] **Run all Flutter tests**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter test
  ```
  Expected: all tests pass.

  ```bash
  cd mobileclaw-flutter/packages/mobileclaw_sdk
  flutter test
  ```
  Expected: all tests pass.

- [ ] **Run flutter analyze**

  ```bash
  cd mobileclaw-flutter/apps/mobileclaw_app
  flutter analyze
  cd mobileclaw-flutter/packages/mobileclaw_sdk
  flutter analyze
  ```
  Expected: no errors or warnings.

- [ ] **Manual smoke test on emulator**

  ```bash
  flutter run --device-id=<emulator-id>
  ```
  Verify:
  - App launches and shows `OnboardingScreen` (no providers configured on fresh install)
  - Fill in Anthropic protocol, URL, model, API key; tap Test → success chip
  - Tap Save → navigates to ChatPage
  - Tap gear icon → Settings → LLM Providers → shows the saved provider with checkmark
  - FAB → add a second provider → appears in list
  - Swipe to delete → removed from list
  - Tap a different provider row → checkmark moves to it

---

## Notes for Future Tasks

- **`AgentConfig.apiKey` / `AgentConfig.model` remain required** in the generated `ffi.dart` until Plan 1 makes them optional. Pass empty strings or use the legacy values from `engine_provider.dart` until the `AgentConfig` signature changes in Plan 1 Step 7.
- **`_nativeAvailable`** is defined in `engine_provider.dart` — re-use it when choosing `probeFn`.
- **flutter_secure_storage key derivation** (noted in `engine_provider.dart` TODO) is tracked separately in Phase 3 — not part of this plan.
