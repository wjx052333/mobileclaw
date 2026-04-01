import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
import 'package:path_provider/path_provider.dart';

/// `true` when the native library is present (Linux desktop / device builds).
bool get _nativeAvailable {
  if (Platform.isLinux) {
    // The .so is bundled alongside the Flutter app binary.
    return true;
  }
  // iOS / Android native support lands in Phase 3.
  return false;
}

/// Singleton [MobileclawAgent] for the app.
///
/// Uses [MobileclawAgentImpl] when the native library is available,
/// otherwise falls back to [MockMobileclawAgent] (dev / unsupported platforms).
final agentProvider = FutureProvider<MobileclawAgent>((ref) async {
  final dir = await getApplicationSupportDirectory();

  if (_nativeAvailable) {
    final agent = await MobileclawAgentImpl.create(
      apiKey: const String.fromEnvironment('ANTHROPIC_API_KEY', defaultValue: ''),
      dbPath: '${dir.path}/claw.db',
      sandboxDir: '${dir.path}/workspace',
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
