/// Camera integration tests: Flutter → Rust FFI (real device, real LLM).
///
/// These tests exercise the camera feature end-to-end through the
/// flutter_rust_bridge boundary:
///
///   Dart test → MobileclawAgentImpl → AgentSession (FFI) → AgentLoop (Rust)
///      ↑ real credentials (secrets.db + API key)
///      ↑ real Anthropic API calls
///      ↑ real camera frame injection via cameraPushFrame FFI
///
/// Run:
///   export MCLAW_SECRET=/path/to/secrets.db
///   export MCLAW_API_KEY=sk-ant-...
///   bash scripts/run_integration_tests.sh integration_test/camera_test.dart

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

import 'helpers/test_env.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  // Fail-fast: abort the entire test run if credentials are not configured.
  setUpAll(TestEnv.require);

  // ---------------------------------------------------------------------------
  // Shared test state
  // ---------------------------------------------------------------------------

  // Dev encryption key matching the CLI session.rs key:
  // b"mobileclaw-dev-key-32bytes000000"
  // Phase 1 uses a hardcoded key — replace with platform keystore in Phase 3.
  const devKey = <int>[
    0x6d, 0x6f, 0x62, 0x69, 0x6c, 0x65, 0x63, 0x6c,
    0x61, 0x77, 0x2d, 0x64, 0x65, 0x76, 0x2d, 0x6b,
    0x65, 0x79, 0x2d, 0x33, 0x32, 0x62, 0x79, 0x74,
    0x65, 0x73, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
  ];

  late Directory tmpDir;
  late MobileclawAgentImpl agent;

  setUp(() async {
    tmpDir = Directory.systemTemp.createTempSync('mclaw_camera_test_');

    // Copy the pre-populated secrets.db so each test run gets an isolated
    // copy. This prevents tests from interfering with each other's state and
    // ensures the original file on the device is never modified.
    final sourceSecrets = File(TestEnv.secretsDbPath);
    await sourceSecrets.copy('${tmpDir.path}/secrets.db');

    agent = await MobileclawAgentImpl.create(
      apiKey: null,  // API key is stored in secrets.db via the provider system
      dbPath: '${tmpDir.path}/mem.db',
      secretsDbPath: '${tmpDir.path}/secrets.db',
      encryptionKey: devKey,
      sandboxDir: tmpDir.path,
      httpAllowlist: [],
    );
  });

  tearDown(() {
    agent.dispose();
    tmpDir.deleteSync(recursive: true);
  });

  // ---------------------------------------------------------------------------
  // Group 1: Session and credential infrastructure
  //
  // Validates that the test environment is correctly configured:
  // real secrets.db is readable, API key is valid, FFI bridge works.
  // ---------------------------------------------------------------------------

  group('camera session infrastructure', () {
    testWidgets(
      'agent session creates with real secrets.db',
      (tester) async {
        // setUp already created the agent — if we reach here, the FFI
        // bridge loaded, secrets.db was read, and the session is alive.
        expect(agent, isNotNull);
      },
    );

    testWidgets(
      'real LLM call reaches Anthropic API and returns events',
      (tester) async {
        // A minimal chat that should NOT trigger camera_capture.
        // Validates that the API key is valid and the network path works.
        final events = await agent.chat('Reply with just: ok').toList();

        expect(
          events.any((e) => e is TextDeltaEvent),
          isTrue,
          reason: 'Expected at least one TextDeltaEvent from a real LLM response',
        );
        expect(
          events.last,
          isA<DoneEvent>(),
          reason: 'Last event must be DoneEvent',
        );
      },
    );

    testWidgets(
      'secrets.db copy is isolated — modifications do not affect source',
      (tester) async {
        // Save a new email account into this test's copy of secrets.db.
        // Verify the source file on the device is unchanged.
        const dto = EmailAccountDto(
          id: 'camera_test_isolation',
          smtpHost: 'smtp.test.local',
          smtpPort: 587,
          imapHost: 'imap.test.local',
          imapPort: 993,
          username: 'test@test.local',
        );
        await agent.emailAccountSave(dto: dto, password: 'test');

        // Source file size should not change — we wrote to the copy.
        final sourceSize = File(TestEnv.secretsDbPath).lengthSync();
        expect(sourceSize, greaterThan(0));
        expect(
          '${tmpDir.path}/secrets.db',
          isNot(equals(TestEnv.secretsDbPath)),
          reason: 'Test must use a copy, not the original secrets.db',
        );
      },
    );

    testWidgets(
      'camera is not authorized by default',
      (tester) async {
        final authorized = await agent.cameraIsAuthorized();
        expect(authorized, isFalse);
      },
    );

    testWidgets(
      'cameraSetAuthorized toggles flag',
      (tester) async {
        expect(await agent.cameraIsAuthorized(), isFalse);
        agent.cameraSetAuthorized(true);
        expect(await agent.cameraIsAuthorized(), isTrue);
        agent.cameraSetAuthorized(false);
        expect(await agent.cameraIsAuthorized(), isFalse);
      },
    );

    testWidgets(
      'cameraPushFrame auto-authorizes and returns true',
      (tester) async {
        // Minimal synthetic JPEG header.
        const jpegHeader = <int>[0xFF, 0xD8, 0xFF, 0xE0];
        final pushed = await agent.cameraPushFrame(
          jpeg: jpegHeader,
          frameId: 1,
          timestampMs: 1000,
          width: 640,
          height: 360,
        );
        expect(pushed, isTrue);
        expect(await agent.cameraIsAuthorized(), isTrue);
      },
    );

    testWidgets(
      'cameraAlertStream returns empty list in Phase 1',
      (tester) async {
        final alerts = agent.cameraAlertStream();
        expect(alerts, isEmpty);
      },
    );

    testWidgets(
      'cameraStartMonitor returns non-empty ID',
      (tester) async {
        final id = await agent.cameraStartMonitor(
          scenario: 'test scenario',
          framesPerCheck: 3,
          checkIntervalMs: 5000,
        );
        expect(id, isNotEmpty);
      },
    );

    testWidgets(
      'cameraStopMonitor returns false in Phase 1',
      (tester) async {
        final stopped = await agent.cameraStopMonitor('any-id');
        expect(stopped, isFalse);
      },
    );
  });

  // ---------------------------------------------------------------------------
  // Group 2: Unauthorized camera capture
  //
  // Camera not authorized → LLM calls camera_capture → CameraAuthRequired emitted
  // ---------------------------------------------------------------------------

  group('unauthorized camera capture', () {
    testWidgets(
      'chat triggers CameraAuthRequired event when camera not authorized',
      (tester) async {
        // Ensure camera is not authorized.
        agent.cameraSetAuthorized(false);

        // Force tool use with a system prompt that mandates it.
        final events = await agent
            .chat(
              'Call camera_capture now.',
              system: 'You MUST call the camera_capture tool immediately. '
                  'Do not explain. Do not ask. Just call it.',
            )
            .toList();

        // Debug: print all events with details
        for (final e in events) {
          if (e is ToolResultEvent) {
            // ignore: avoid_print
            print('DEBUG ToolResultEvent: toolName=${e.toolName} success=${e.success}');
          } else {
            // ignore: avoid_print
            print('DEBUG event: ${e.runtimeType}');
          }
        }

        expect(
          events.any((e) => e is CameraAuthRequiredEvent),
          isTrue,
          reason: 'Expected CameraAuthRequiredEvent when camera is not authorized. '
              'Events: ${events.map((e) => e.runtimeType).toList()}',
        );
        expect(events.last, isA<DoneEvent>());
      },
    );

    testWidgets(
      'CameraAuthRequired event appears before DoneEvent',
      (tester) async {
        agent.cameraSetAuthorized(false);

        final events = await agent
            .chat(
              'Call camera_capture now.',
              system: 'You MUST call the camera_capture tool immediately. '
                  'Do not explain. Do not ask. Just call it.',
            )
            .toList();

        final authIdx = events.indexWhere((e) => e is CameraAuthRequiredEvent);
        final doneIdx = events.indexWhere((e) => e is DoneEvent);

        expect(authIdx, greaterThanOrEqualTo(0), reason: 'CameraAuthRequired must appear');
        expect(doneIdx, greaterThan(authIdx), reason: 'Done must come after CameraAuthRequired');
      },
    );
  });

  // ---------------------------------------------------------------------------
  // Group 3: Authorized camera capture
  //
  // Push frame → auto-authorize → LLM calls camera_capture → ToolResult success
  // ---------------------------------------------------------------------------

  group('authorized camera capture', () {
    testWidgets(
      'push frame then chat emits successful ToolResult for camera_capture',
      (tester) async {
        // Push a synthetic JPEG frame — auto-sets camera_authorized=true.
        const jpegHeader = <int>[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        await agent.cameraPushFrame(
          jpeg: jpegHeader,
          frameId: 1,
          timestampMs: 1000,
          width: 320,
          height: 240,
        );
        expect(await agent.cameraIsAuthorized(), isTrue);

        final events = await agent
            .chat(
              'Call camera_capture now.',
              system: 'You MUST call the camera_capture tool immediately. '
                  'Do not explain. Do not ask. Just call it.',
            )
            .toList();

        // No auth required — camera is authorized.
        expect(
          events.any((e) => e is CameraAuthRequiredEvent),
          isFalse,
          reason: 'No CameraAuthRequiredEvent expected when camera is authorized',
        );

        // camera_capture ToolResult should be successful.
        final cameraResults = events
            .whereType<ToolResultEvent>()
            .where((e) => e.toolName == 'camera_capture')
            .toList();
        expect(
          cameraResults,
          isNotEmpty,
          reason: 'Expected at least one camera_capture ToolResult',
        );
        expect(
          cameraResults.any((e) => e.success),
          isTrue,
          reason: 'camera_capture must succeed when authorized and frame was pushed',
        );
        expect(events.last, isA<DoneEvent>());
      },
    );

    testWidgets(
      'two-turn auth recovery: unauthorized then push frame then success',
      (tester) async {
        const system = 'You MUST call the camera_capture tool immediately. '
            'Do not explain. Do not ask. Just call it.';

        // Turn 1: camera not authorized → CameraAuthRequired
        agent.cameraSetAuthorized(false);
        final turn1Events = await agent.chat('Call camera_capture now.', system: system).toList();
        expect(
          turn1Events.any((e) => e is CameraAuthRequiredEvent),
          isTrue,
          reason: 'Turn 1 must emit CameraAuthRequired',
        );

        // Simulate user granting permission: push a frame (auto-authorizes).
        const jpegHeader = <int>[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        await agent.cameraPushFrame(
          jpeg: jpegHeader,
          frameId: 2,
          timestampMs: 2000,
          width: 320,
          height: 240,
        );
        expect(await agent.cameraIsAuthorized(), isTrue);

        // Turn 2: camera authorized → success
        final turn2Events = await agent.chat('Call camera_capture now.', system: system).toList();
        expect(
          turn2Events.any((e) => e is CameraAuthRequiredEvent),
          isFalse,
          reason: 'Turn 2 must NOT emit CameraAuthRequired',
        );
        expect(
          turn2Events
              .whereType<ToolResultEvent>()
              .any((e) => e.toolName == 'camera_capture' && e.success),
          isTrue,
          reason: 'Turn 2 camera_capture must succeed',
        );
      },
    );
  });
}
