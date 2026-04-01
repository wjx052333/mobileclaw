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
