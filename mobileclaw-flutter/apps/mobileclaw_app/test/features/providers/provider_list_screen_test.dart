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
