/// Test environment helper for integration tests that require real credentials.
///
/// Values are injected at compile time via `--dart-define` by the test runner
/// script [scripts/run_integration_tests.sh]. If a required variable is
/// missing, [TestEnv.require] fails immediately with a message that points
/// to the script.
///
/// Usage in a test file:
/// ```dart
/// setUpAll(() => TestEnv.require());
///
/// setUp(() async {
///   tmpDir = Directory.systemTemp.createTempSync('mclaw_test_');
///   // Copy the pre-populated secrets.db so each test gets an isolated copy.
///   await File(TestEnv.secretsDbPath).copy('${tmpDir.path}/secrets.db');
///   agent = await MobileclawAgentImpl.create(
///     apiKey: null,   // API key is stored in secrets.db via provider system
///     dbPath: '${tmpDir.path}/mem.db',
///     secretsDbPath: '${tmpDir.path}/secrets.db',
///     ...
///   );
/// });
/// ```
library test_env;

import 'package:flutter_test/flutter_test.dart';

// --dart-define value injected by scripts/run_integration_tests.sh.
// This is a compile-time constant — cannot be read from the OS environment
// at runtime on a device. Use the shell script to pass it.
const _secretsDbPath = String.fromEnvironment('MCLAW_SECRETS_DB_PATH');

/// Compile-time credentials for integration tests.
abstract final class TestEnv {
  /// Absolute path to the pre-populated `secrets.db` ON THE DEVICE.
  ///
  /// Provided by:
  ///   `flutter test --dart-define=MCLAW_SECRETS_DB_PATH=/data/local/tmp/mclaw_secrets.db`
  ///
  /// The secrets.db must contain an active LLM provider with API key.
  ///
  /// Throws [TestFailure] if not set.
  static String get secretsDbPath {
    if (_secretsDbPath.isEmpty) {
      fail(
        'MCLAW_SECRETS_DB_PATH is not set.\n'
        '\n'
        'Run tests through the provided script:\n'
        '  export MCLAW_SECRET=/path/to/secrets.db\n'
        '  bash scripts/run_integration_tests.sh\n'
        '\n'
        'Or pass directly:\n'
        '  flutter test --dart-define=MCLAW_SECRETS_DB_PATH=/data/local/tmp/mclaw_secrets.db',
      );
    }
    return _secretsDbPath;
  }

  /// Call once in [setUpAll] to fail-fast if any required credential is missing.
  static void require() {
    secretsDbPath;
  }
}
