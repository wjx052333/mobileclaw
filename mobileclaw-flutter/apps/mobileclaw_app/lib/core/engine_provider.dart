import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
import 'package:path_provider/path_provider.dart';

/// `true` when the native library is available.
/// Phase 3 adds Android. iOS requires a Mac build — not yet supported.
bool get _nativeAvailable => Platform.isLinux || Platform.isAndroid;

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
      apiKey: const String.fromEnvironment('ANTHROPIC_API_KEY', defaultValue: ''),
      dbPath: '${dir.path}/claw.db',
      secretsDbPath: '${dir.path}/secrets.db',
      encryptionKey: devKey,
      sandboxDir: workspaceDir.path,
      httpAllowlist: ['https://api.anthropic.com/'],
    );
    ref.onDispose(agent.dispose);
    return agent;
  }

  // Fallback for development / unsupported platforms.
  final agent = await MockMobileclawAgent.create(
    apiKey: '',
    dbPath: '${dir.path}/claw.db',
    sandboxDir: '${dir.path}/workspace',
    httpAllowlist: [],
  );
  ref.onDispose(agent.dispose);
  return agent;
});
