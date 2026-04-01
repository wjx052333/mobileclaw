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
