import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
import 'package:path_provider/path_provider.dart';

/// Singleton [MobileclawAgent] for the app.
///
/// Phase 1: creates a [MockMobileclawAgent].
/// Phase 2: replace with [MobileclawAgent.create] backed by FFI.
final agentProvider = FutureProvider<MobileclawAgent>((ref) async {
  final dir = await getApplicationSupportDirectory();
  return MockMobileclawAgent.create(
    apiKey: 'not-used-in-mock',
    dbPath: '${dir.path}/claw.db',
    sandboxDir: '${dir.path}/workspace',
    httpAllowlist: [],
  );
});
