import 'dart:async';
import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
import 'package:path_provider/path_provider.dart';

/// `true` when the native library is available.
/// Phase 3 adds Android. iOS requires a Mac build — not yet supported.
bool get _nativeAvailable => Platform.isLinux || Platform.isAndroid;

/// Recreate the agent session, picking up the active provider from secrets.db.
///
/// Call after the user configures and activates a provider during onboarding.
/// The old agent is disposed, a new FFI session is created, and the new agent
/// is returned. [ChatPage] will automatically use it via [agentProvider].
Future<MobileclawAgent> reinitializeAgent(WidgetRef ref) async {
  final completer = Completer<MobileclawAgent>();
  // Listen for the refreshed provider to complete — NOT fireImmediately,
  // because that would fire with the stale pre-refresh value.
  final subscription = ref.listenManual<AsyncValue<MobileclawAgent>>(
    agentProvider,
    (previous, next) {
      if (next is AsyncData<MobileclawAgent>) {
        completer.complete(next.value);
      } else if (next is AsyncError) {
        final err = next.error;
        if (err != null) {
          completer.completeError(err, next.stackTrace);
        }
      }
    },
  );
  ref.refresh(agentProvider);
  final agent = await completer.future;
  subscription.close();
  return agent;
}

/// Singleton [MobileclawAgent] for the app.
///
/// Uses [MobileclawAgentImpl] when the native library is available,
/// otherwise falls back to [MockMobileclawAgent] (dev / unsupported platforms).
final agentProvider = FutureProvider<MobileclawAgent>((ref) async {
  final dir = await getApplicationSupportDirectory();

  if (_nativeAvailable) {
    final workspaceDir = Directory('${dir.path}/workspace');
    await workspaceDir.create(recursive: true);
    // TODO(Phase 2): derive encryptionKey from flutter_secure_storage.
    // The fixed dev key below is safe only for local development and emulator testing.
    const devKey = <int>[
      0x6d, 0x63, 0x6c, 0x61, 0x77, 0x2d, 0x64, 0x65, // mobileclaw-de
      0x76, 0x2d, 0x6b, 0x65, 0x79, 0x2d, 0x33, 0x32, // v-key-32
      0x62, 0x79, 0x74, 0x65, 0x73, 0x00, 0x00, 0x00, // bytes...
      0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // padding
    ];
    final agent = await MobileclawAgentImpl.create(
      apiKey: null,
      dbPath: '${dir.path}/claw.db',
      secretsDbPath: '${dir.path}/secrets.db',
      encryptionKey: devKey,
      sandboxDir: workspaceDir.path,
      httpAllowlist: ['https://api.anthropic.com/'],
      logDir: dir.path,
    );
    ref.onDispose(agent.dispose);
    return agent;
  }

  // Fallback for development / unsupported platforms.
  final agent = await MockMobileclawAgent.create(
    apiKey: null,
    dbPath: '${dir.path}/claw.db',
    sandboxDir: '${dir.path}/workspace',
    httpAllowlist: [],
  );
  ref.onDispose(agent.dispose);
  return agent;
});
