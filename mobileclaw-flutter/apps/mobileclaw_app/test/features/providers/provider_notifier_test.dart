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
