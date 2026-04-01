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
