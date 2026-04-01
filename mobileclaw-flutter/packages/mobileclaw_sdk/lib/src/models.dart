/// A single turn in the conversation history.
class ChatMessage {
  const ChatMessage({required this.role, required this.content});

  /// 'user' or 'assistant'
  final String role;
  final String content;

  @override
  bool operator ==(Object other) =>
      other is ChatMessage && other.role == role && other.content == content;

  @override
  int get hashCode => Object.hash(role, content);
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

  @override
  bool operator ==(Object other) =>
      other is SkillManifest &&
      other.name == name &&
      other.description == description &&
      other.trust == trust &&
      _listEq(other.keywords, keywords) &&
      _listEq(other.allowedTools, allowedTools);

  @override
  int get hashCode => Object.hash(
        name,
        description,
        trust,
        Object.hashAll(keywords),
        allowedTools == null ? null : Object.hashAll(allowedTools!),
      );
}

bool _listEq<T>(List<T>? a, List<T>? b) {
  if (a == null && b == null) return true;
  if (a == null || b == null) return false;
  if (a.length != b.length) return false;
  for (var i = 0; i < a.length; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}
