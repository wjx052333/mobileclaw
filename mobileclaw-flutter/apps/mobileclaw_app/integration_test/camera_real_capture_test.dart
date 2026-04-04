/// Real camera capture integration tests.
///
/// These tests open the device/emulator camera, capture actual frames,
/// encode them as JPEG, and push them through the Flutter → Rust FFI pipeline.
///
/// What is tested end-to-end:
///   Real camera → CameraImage (YUV420/BGRA) → JPEG bytes
///     → cameraPushFrame FFI → Rust ring buffer
///     → LLM chat(camera_capture) → ToolResult success
///
/// Prerequisites:
///   - Android emulator with a virtual camera (AVD default) or physical device
///   - CAMERA permission granted (AndroidManifest.xml includes it)
///   - secrets.db with active LLM provider
///
/// Run:
///   export MCLAW_SECRET=/path/to/secrets.db
///   bash scripts/run_integration_tests.sh integration_test/camera_real_capture_test.dart

import 'dart:async';
import 'dart:io';

import 'package:camera/camera.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;
import 'package:integration_test/integration_test.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

import 'helpers/test_env.dart';

// ---------------------------------------------------------------------------
// JPEG encoding from CameraImage
// ---------------------------------------------------------------------------

/// Convert a [CameraImage] (YUV420 or BGRA8888) to a JPEG byte list.
///
/// Returns null if the format is unsupported or conversion fails.
List<int>? _cameraImageToJpeg(CameraImage image, {int quality = 75}) {
  try {
    img.Image? decoded;

    if (image.format.group == ImageFormatGroup.yuv420) {
      // YUV420 has 3 planes: Y, U, V
      final yPlane = image.planes[0];
      final uPlane = image.planes[1];
      final vPlane = image.planes[2];

      final width = image.width;
      final height = image.height;

      decoded = img.Image(width: width, height: height);

      for (int y = 0; y < height; y++) {
        for (int x = 0; x < width; x++) {
          final yIndex = y * yPlane.bytesPerRow + x;
          // UV planes are half-resolution (one sample per 2×2 block)
          final uvIndex =
              (y ~/ 2) * uPlane.bytesPerRow + (x ~/ 2) * uPlane.bytesPerPixel!;

          final yVal = yPlane.bytes[yIndex];
          final uVal = uPlane.bytes[uvIndex];
          final vVal = vPlane.bytes[uvIndex];

          // BT.601 YUV → RGB
          final r = (yVal + 1.402 * (vVal - 128)).clamp(0, 255).toInt();
          final g = (yVal - 0.344136 * (uVal - 128) - 0.714136 * (vVal - 128))
              .clamp(0, 255)
              .toInt();
          final b = (yVal + 1.772 * (uVal - 128)).clamp(0, 255).toInt();

          decoded.setPixelRgb(x, y, r, g, b);
        }
      }
    } else if (image.format.group == ImageFormatGroup.bgra8888) {
      final plane = image.planes[0];
      decoded = img.Image.fromBytes(
        width: image.width,
        height: image.height,
        bytes: plane.bytes.buffer,
        order: img.ChannelOrder.bgra,
      );
    } else {
      // Unsupported format
      return null;
    }

    return img.encodeJpg(decoded, quality: quality);
  } catch (_) {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Camera helper: open first available camera, grab N frames, dispose.
// ---------------------------------------------------------------------------

/// Open the first available camera, wait until [count] frames arrive,
/// convert each to JPEG, then close the camera.
///
/// Throws [StateError] if no cameras are available.
Future<List<List<int>>> _captureRealFrames({
  required int count,
  Duration timeout = const Duration(seconds: 10),
}) async {
  final cameras = await availableCameras();
  if (cameras.isEmpty) throw StateError('No cameras available on this device');

  // Prefer back camera; fall back to whatever is available.
  final desc = cameras.firstWhere(
    (c) => c.lensDirection == CameraLensDirection.back,
    orElse: () => cameras.first,
  );

  final controller = CameraController(
    desc,
    ResolutionPreset.low, // 320×240 or similar — sufficient for tests
    enableAudio: false,
    imageFormatGroup: ImageFormatGroup.yuv420,
  );

  await controller.initialize();

  final frames = <List<int>>[];
  final done = Completer<void>();

  await controller.startImageStream((image) {
    if (frames.length >= count) return;

    final jpeg = _cameraImageToJpeg(image);
    if (jpeg != null) {
      frames.add(jpeg);
      if (frames.length >= count) {
        if (!done.isCompleted) done.complete();
      }
    }
  });

  await done.future.timeout(timeout, onTimeout: () {
    if (!done.isCompleted) done.complete();
  });

  await controller.stopImageStream();
  await controller.dispose();

  return frames;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(TestEnv.require);

  // Dev encryption key matching the CLI session.rs key:
  // b"mobileclaw-dev-key-32bytes000000"
  const devKey = <int>[
    0x6d, 0x6f, 0x62, 0x69, 0x6c, 0x65, 0x63, 0x6c,
    0x61, 0x77, 0x2d, 0x64, 0x65, 0x76, 0x2d, 0x6b,
    0x65, 0x79, 0x2d, 0x33, 0x32, 0x62, 0x79, 0x74,
    0x65, 0x73, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
  ];

  late Directory tmpDir;
  late MobileclawAgentImpl agent;

  setUp(() async {
    tmpDir = Directory.systemTemp.createTempSync('mclaw_real_camera_test_');
    await File(TestEnv.secretsDbPath).copy('${tmpDir.path}/secrets.db');

    agent = await MobileclawAgentImpl.create(
      apiKey: null,
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
  // Group: Real camera frames
  // ---------------------------------------------------------------------------

  group('real camera capture', () {
    testWidgets(
      'device has at least one camera',
      (tester) async {
        final cameras = await tester.runAsync(() => availableCameras());
        expect(
          cameras,
          isNotEmpty,
          reason: 'No cameras found on this device/emulator. '
              'Ensure the AVD has a virtual camera configured.',
        );
      },
    );

    testWidgets(
      'can capture a real frame and encode it as JPEG',
      (tester) async {
        final frames = await tester.runAsync(
          () => _captureRealFrames(count: 1),
        );
        expect(frames, isNotEmpty, reason: 'No frames captured within timeout');

        final jpeg = frames!.first;
        // All valid JPEG files start with FF D8
        expect(jpeg.length, greaterThan(2));
        expect(jpeg[0], 0xFF);
        expect(jpeg[1], 0xD8);
      },
    );

    testWidgets(
      'real JPEG frame pushes successfully via FFI',
      (tester) async {
        final frames = await tester.runAsync(
          () => _captureRealFrames(count: 1),
        );
        expect(frames, isNotEmpty);

        final jpeg = frames!.first;
        final pushed = await agent.cameraPushFrame(
          jpeg: jpeg,
          frameId: 1,
          timestampMs: DateTime.now().millisecondsSinceEpoch,
          width: 320,
          height: 240,
        );

        expect(pushed, isTrue);
        expect(await agent.cameraIsAuthorized(), isTrue);
      },
    );

    testWidgets(
      'push 5 real frames, ring buffer holds them',
      (tester) async {
        final frames = await tester.runAsync(
          () => _captureRealFrames(count: 5),
        );
        expect(frames!.length, greaterThanOrEqualTo(1),
            reason: 'Expected at least 1 frame within timeout');

        for (var i = 0; i < frames.length; i++) {
          final pushed = await agent.cameraPushFrame(
            jpeg: frames[i],
            frameId: i + 1,
            timestampMs: DateTime.now().millisecondsSinceEpoch + i * 33,
            width: 320,
            height: 240,
          );
          expect(pushed, isTrue, reason: 'Frame $i push failed');
        }

        expect(await agent.cameraIsAuthorized(), isTrue);
      },
    );

    testWidgets(
      'LLM camera_capture tool succeeds with real JPEG frame in ring buffer',
      (tester) async {
        // Capture and push a real frame first.
        final frames = await tester.runAsync(
          () => _captureRealFrames(count: 1),
        );
        expect(frames, isNotEmpty);

        await agent.cameraPushFrame(
          jpeg: frames!.first,
          frameId: 1,
          timestampMs: DateTime.now().millisecondsSinceEpoch,
          width: 320,
          height: 240,
        );
        expect(await agent.cameraIsAuthorized(), isTrue);

        // Now ask the LLM to call camera_capture.
        final events = await agent
            .chat(
              'Call camera_capture now.',
              system: 'You MUST call the camera_capture tool immediately. '
                  'Do not explain. Do not ask. Just call it.',
            )
            .toList();

        // Should not require auth (already authorized).
        expect(
          events.any((e) => e is CameraAuthRequiredEvent),
          isFalse,
          reason: 'No CameraAuthRequiredEvent expected — camera is authorized',
        );

        // camera_capture ToolResult must be successful.
        final cameraResults = events
            .whereType<ToolResultEvent>()
            .where((e) => e.toolName == 'camera_capture')
            .toList();

        expect(cameraResults, isNotEmpty,
            reason: 'Expected camera_capture ToolResult');
        expect(
          cameraResults.any((e) => e.success),
          isTrue,
          reason: 'camera_capture must succeed with a real frame in the buffer',
        );
        expect(events.last, isA<DoneEvent>());
      },
    );
  });
}
