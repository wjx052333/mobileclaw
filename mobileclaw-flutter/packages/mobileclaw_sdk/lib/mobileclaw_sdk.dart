/// MobileClaw SDK — Flutter Plugin for the MobileClaw AI Agent engine.
///
/// Phase 1 (current): Use [MockMobileclawAgent] for all development.
/// Phase 2: Replace with the real FFI-backed [MobileclawAgent.create].
library mobileclaw_sdk;

export 'src/engine.dart';
export 'src/events.dart';
export 'src/exceptions.dart';
export 'src/memory.dart';
export 'src/models.dart';
export 'src/mock.dart';
