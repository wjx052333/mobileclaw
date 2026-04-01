/// A single turn in the conversation history.
class ChatMessage {
  const ChatMessage({required this.role, required this.content});

  /// 'user' or 'assistant'
  final String role;
  final String content;
}

/// Trust level of a loaded skill bundle.
enum SkillTrust {
  /// Shipped with the app binary. Granted full tool access by default.
  bundled,

  /// Downloaded by the user at runtime. Restricted to [allowedTools].
  installed,
}

/// Metadata loaded from a skill's skill.yaml manifest.
class SkillManifest {
  const SkillManifest({
    required this.name,
    required this.description,
    required this.trust,
    required this.keywords,
    this.allowedTools,
  });

  final String name;
  final String description;
  final SkillTrust trust;
  final List<String> keywords;
  final List<String>? allowedTools;
}
