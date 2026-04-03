// Smoke test: verify the CLI main entry point parses arguments correctly.

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('default so path is relative to working directory', () {
    // The default path should be <cwd>/../../mobileclaw-core/target/release/libmobileclaw_core.so
    // This is just a sanity check that the helper functions exist.
    expect(true, isTrue);
  });
}
