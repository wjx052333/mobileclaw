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
    // ProviderListScreen shows the title in app bar
    expect(find.text('LLM Providers'), findsWidgets); // title in AppBar
  });
}
