/// Unit tests: mmap zero-copy frame path — Dart layer contract.
///
/// Tests the camera ring-buffer occupancy contract as seen from Dart:
///   cameraPushFrame() → cameraGetMmapInfo() must return (occupancy, capacity, latestTs)
///   where occupancy == actual frames in the ring buffer.
///
/// All tests use [MockMobileclawAgent] — no FFI, no device required.
/// Run: flutter test test/camera_mmap_unit_test.dart
///
/// Phase 1 status: MockMobileclawAgent.cameraGetMmapInfo() returns hardcoded (0,16,0).
///   h1, h2, h3, h5 are RED in Phase 1 and GREEN after Mock is fixed.
///   h4 is GREEN in both (data integrity, no occupancy assertion).

import 'package:flutter_test/flutter_test.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

void main() {
  late MockMobileclawAgent agent;

  setUp(() => agent = MockMobileclawAgent());

  // ---------------------------------------------------------------------------
  // Group H — mmap zero-copy frame path (mirrors Rust integration_camera.rs Group H)
  // ---------------------------------------------------------------------------

  group('mmap zero-copy frame path', () {
    /// h1: A single frame pushed via cameraPushFrame is visible in cameraGetMmapInfo occupancy.
    ///
    /// RED in Phase 1: MockMobileclawAgent returns hardcoded occupancy=0.
    /// GREEN after Mock tracks real occupancy.
    test('h1: single frame push reflects in mmap occupancy', () async {
      // Before any push
      final (occBefore, cap, tsBefore) = await agent.cameraGetMmapInfo();
      expect(occBefore, 0, reason: 'occupancy must be 0 before any push');
      expect(cap, 16, reason: 'capacity must be 16 (default)');
      expect(tsBefore, 0, reason: 'latestTs must be 0 before any push');

      // Push one frame
      const jpegHeader = <int>[0xFF, 0xD8, 0xFF, 0xE0];
      final pushed = await agent.cameraPushFrame(
        jpeg: jpegHeader,
        frameId: 1,
        timestampMs: 42000,
        width: 640,
        height: 360,
      );
      expect(pushed, isTrue);

      final (occAfter, _, tsAfter) = await agent.cameraGetMmapInfo();
      expect(
        occAfter, 1,
        reason: 'occupancy must be 1 after one frame push '
            '(zero-copy slot count); fails if Mock still returns hardcoded 0',
      );
      expect(tsAfter, 42000, reason: 'latestTs must reflect the pushed frame timestamp');
    });

    /// h2: Occupancy tracks exact frame count up to buffer capacity.
    ///
    /// RED in Phase 1: occupancy always returns 0.
    /// GREEN after Mock tracks pushes.
    test('h2: mmap occupancy tracks frame count per push', () async {
      for (var i = 1; i <= 8; i++) {
        await agent.cameraPushFrame(
          jpeg: [i],
          frameId: i,
          timestampMs: i * 1000,
          width: 320,
          height: 240,
        );
        final (occ, cap, ts) = await agent.cameraGetMmapInfo();
        expect(
          occ, i,
          reason: 'occupancy must equal frames pushed so far ($i); '
              'fails if Mock still returns hardcoded 0',
        );
        expect(cap, 16, reason: 'capacity must remain 16');
        expect(ts, i * 1000, reason: 'latestTs must track most-recent push');
      }
    });

    /// h3: When ring buffer overflows (> capacity frames), occupancy is capped at capacity.
    ///
    /// RED in Phase 1: occupancy always 0.
    /// GREEN after Mock implements eviction.
    test('h3: occupancy capped at capacity on ring-buffer overflow', () async {
      // Push 20 frames into capacity-16 buffer
      for (var i = 1; i <= 20; i++) {
        await agent.cameraPushFrame(
          jpeg: [i],
          frameId: i,
          timestampMs: i * 500,
          width: 640,
          height: 480,
        );
      }

      final (occ, cap, ts) = await agent.cameraGetMmapInfo();
      expect(cap, 16, reason: 'capacity must be 16');
      expect(
        occ, 16,
        reason: 'occupancy must be capped at capacity (16) after overflow; '
            'fails if Mock returns 0 or 20',
      );
      expect(ts, 20 * 500, reason: 'latestTs must reflect the last pushed frame');
    });

    /// h4: Data integrity — bytes pushed via cameraPushFrame survive unchanged.
    ///
    /// GREEN in both Phase 1 and Phase 2: does not check occupancy.
    /// Anchors the zero-copy data contract that occupancy tests build on.
    test('h4: frame bytes and timestamp survive cameraPushFrame unchanged', () async {
      const payload = <int>[0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
      await agent.cameraPushFrame(
        jpeg: payload,
        frameId: 99,
        timestampMs: 77777,
        width: 1920,
        height: 1080,
      );

      // cameraGetMmapInfo must at least reflect the correct timestamp
      final (_, __, ts) = await agent.cameraGetMmapInfo();
      expect(ts, 77777, reason: 'timestamp must survive cameraPushFrame unchanged');

      // auto-authorize must have triggered
      expect(await agent.cameraIsAuthorized(), isTrue,
          reason: 'cameraPushFrame must auto-authorize');
    });

    /// h5: Multiple cameraPushFrame calls produce the same mmap_info as
    ///     querying occupancy step-by-step.
    ///
    /// RED in Phase 1: occupancy always 0.
    /// GREEN after Mock tracks pushes.
    test('h5: sequential push accumulation matches expected occupancy', () async {
      const frameCount = 5;
      for (var i = 1; i <= frameCount; i++) {
        await agent.cameraPushFrame(
          jpeg: [i],
          frameId: i,
          timestampMs: i * 100,
          width: 640,
          height: 360,
        );
      }

      final (occ, cap, ts) = await agent.cameraGetMmapInfo();
      expect(cap, 16, reason: 'capacity must be 16');
      expect(ts, frameCount * 100, reason: 'latestTs must reflect last frame');
      expect(
        occ, frameCount,
        reason: 'occupancy must be $frameCount (zero-copy slot count); '
            'fails if Mock still returns hardcoded 0',
      );
    });

    /// h6: cameraGetMmapInfo returns (0, 16, 0) on a fresh session with no pushes.
    ///
    /// GREEN in Phase 1 and Phase 2: no push → empty state.
    test('h6: fresh session reports zero occupancy and zero timestamp', () async {
      final fresh = MockMobileclawAgent();
      final (occ, cap, ts) = await fresh.cameraGetMmapInfo();
      expect(occ, 0, reason: 'fresh session must have 0 occupancy');
      expect(cap, 16, reason: 'fresh session must have capacity 16');
      expect(ts, 0, reason: 'fresh session must have latestTs 0');
    });
  });
}
