import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

import 'core/engine_provider.dart';
import 'features/chat/chat_page.dart';
import 'features/providers/onboarding_screen.dart';

void main() {
  runApp(const ProviderScope(child: MobileClawApp()));
}

class MobileClawApp extends ConsumerWidget {
  const MobileClawApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return MaterialApp(
      title: 'MobileClaw',
      theme: ThemeData(colorSchemeSeed: Colors.deepPurple, useMaterial3: true),
      home: const _AppShell(),
    );
  }
}

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
