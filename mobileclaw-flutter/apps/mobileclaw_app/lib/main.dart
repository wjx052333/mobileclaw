import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
import 'package:path_provider/path_provider.dart';

import 'core/engine_provider.dart';
import 'core/logger.dart';
import 'features/chat/chat_page.dart';
import 'features/providers/onboarding_screen.dart';

FileLogger? _flutterLogger;

void main() {
  runZonedGuarded(() async {
    WidgetsFlutterBinding.ensureInitialized();

    // Initialise the Flutter-side log file.
    final dir = await getApplicationSupportDirectory();
    _flutterLogger = await FileLogger.init('${dir.path}/flutter.log');

    // Catch all unhandled Flutter errors and write to log file instead of
    // showing a raw stack trace on screen.
    FlutterError.onError = (details) {
      final msg = 'FlutterError: ${details.exception}';
      debugPrint(msg);
      _flutterLogger?.error('$msg\n${details.stack}');
      // Don't call presentError — we log to file instead of showing a red screen.
    };

    // Catch errors from non-Future zones (e.g. FFI panics that leak through).
    PlatformDispatcher.instance.onError = (error, stack) {
      final msg = 'Platform error: $error';
      debugPrint(msg);
      _flutterLogger?.error('$msg\n$stack');
      return true; // suppress the default error overlay
    };

    runApp(const ProviderScope(child: MobileClawApp()));
  }, (error, stack) {
    _flutterLogger?.error('Unhandled zone error: $error\n$stack');
  });
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
      error: (e, st) {
        // Log the error to file; show a user-friendly message on screen.
        _flutterLogger?.error('Agent init failed: $e\n$st');
        return Scaffold(
          body: Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                const Text('Failed to initialise agent.'),
                const SizedBox(height: 8),
                Text(
                  'See ${_flutterLogger != null ? "flutter.log" : "logs"} for details.',
                  style: const TextStyle(fontSize: 12, color: Colors.grey),
                ),
              ],
            ),
          ),
        );
      },
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
