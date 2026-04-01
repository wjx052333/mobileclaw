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
    await tester.tap(find.widgetWithText(OutlinedButton, 'Test'));
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
