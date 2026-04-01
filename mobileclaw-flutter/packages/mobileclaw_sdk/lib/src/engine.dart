import 'dart:async';

import 'events.dart';
import 'memory.dart';
import 'models.dart';

export 'events.dart';
export 'memory.dart';
export 'models.dart';
export 'exceptions.dart';

/// Top-level agent handle. Wraps the Rust AgentSession (Phase 2).
/// Opaque: do not store internal fields; use method calls only.
///
/// Thread safety: not safe to share across Flutter isolates.
/// Create one instance per isolate, or serialize all calls from a single isolate.
abstract class MobileclawAgent {
  /// Create and initialise an agent.
  ///
  /// - [apiKey]        Anthropic API key.
  /// - [dbPath]        Absolute path to the SQLite database file.
  /// - [sandboxDir]    Root directory for file-system tools.
  /// - [httpAllowlist] URL prefixes the HTTP tool may fetch.
  /// - [model]         LLM model identifier.
  /// - [skillsDir]     Optional directory of skill bundles.
  static Future<MobileclawAgent> create({
    required String apiKey,
    required String dbPath,
    required String sandboxDir,
    required List<String> httpAllowlist,
    String model = 'claude-opus-4-6',
    String? skillsDir,
  }) {
    throw UnimplementedError(
      'Phase 2: replace with real FFI implementation. '
      'Use MockMobileclawAgent for development.',
    );
  }

  /// Release all Rust-side resources. Must not be used after this call.
  void dispose();

  /// Stream all events for one user turn.
  ///
  /// Completes when [DoneEvent] is emitted or an error is thrown as [ClawException].
  Stream<AgentEvent> chat(String userInput, {String system = ''});

  /// Convenience wrapper: collects all [TextDeltaEvent] fragments into a string.
  Future<String> chatText(String userInput, {String system = ''});

  /// The full conversation history for the current session.
  List<ChatMessage> get history;

  /// Memory subsystem.
  MobileclawMemory get memory;

  /// Load all skill bundles found under [dirPath].
  Future<void> loadSkillsFromDir(String dirPath);

  /// Manifests of all currently loaded skills, in load order.
  List<SkillManifest> get skills;

  /// Save an email account configuration and its password.
  ///
  /// The password is encrypted with AES-256-GCM on the Rust side before
  /// storage. After this call the plaintext password is no longer accessible.
  /// Call this once from the app settings screen when the user provides
  /// their credentials.
  ///
  /// Throws [ClawException] on storage error.
  Future<void> emailAccountSave({
    required EmailAccountDto dto,
    required String password,
  });

  /// Load an email account's configuration.
  ///
  /// Returns null if the account does not exist. The password is NOT returned
  /// — there is no way to retrieve it after saving. This is intentional.
  ///
  /// Throws [ClawException] on storage error.
  Future<EmailAccountDto?> emailAccountLoad({required String id});

  /// Delete an email account and its stored password.
  ///
  /// No-op if the account does not exist.
  /// Throws [ClawException] on storage error.
  Future<void> emailAccountDelete({required String id});
}
