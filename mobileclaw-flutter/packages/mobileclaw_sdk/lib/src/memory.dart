/// Category of a stored memory document.
sealed class MemoryCategory {
  const MemoryCategory();

  /// Persistent facts about the user or application. Never auto-expired.
  static const core = _NamedCategory('core');

  /// Notes generated today; may be summarised or pruned after 24 h.
  static const daily = _NamedCategory('daily');

  /// Transient notes from the current session.
  static const conversation = _NamedCategory('conversation');

  /// User-defined category with an arbitrary [label].
  const factory MemoryCategory.custom(String label) = _CustomCategory;
}

final class _NamedCategory extends MemoryCategory {
  const _NamedCategory(this._name);
  final String _name;

  @override
  String toString() => _name;

  @override
  bool operator ==(Object other) =>
      other is _NamedCategory && other._name == _name;

  @override
  int get hashCode => _name.hashCode;
}

final class _CustomCategory extends MemoryCategory {
  const _CustomCategory(this.label);
  final String label;

  @override
  String toString() => 'custom:$label';

  @override
  bool operator ==(Object other) =>
      other is _CustomCategory && other.label == label;

  @override
  int get hashCode => label.hashCode;
}

/// A stored memory document.
class MemoryDoc {
  const MemoryDoc({
    required this.id,
    required this.path,
    required this.content,
    required this.category,
    required this.createdAt,
    required this.updatedAt,
  });

  final String id;
  final String path;
  final String content;
  final MemoryCategory category;
  final int createdAt;
  final int updatedAt;

  DateTime get createdAtDt =>
      DateTime.fromMillisecondsSinceEpoch(createdAt * 1000);
  DateTime get updatedAtDt =>
      DateTime.fromMillisecondsSinceEpoch(updatedAt * 1000);

  @override
  bool operator ==(Object other) =>
      other is MemoryDoc &&
      other.id == id &&
      other.path == path &&
      other.content == content &&
      other.category == category &&
      other.createdAt == createdAt &&
      other.updatedAt == updatedAt;

  @override
  int get hashCode =>
      Object.hash(id, path, content, category, createdAt, updatedAt);
}

/// A memory document returned by a search query, with a relevance score.
class SearchResult {
  const SearchResult({required this.doc, required this.score});
  final MemoryDoc doc;
  final double score;

  @override
  bool operator ==(Object other) =>
      other is SearchResult && other.doc == doc && other.score == score;

  @override
  int get hashCode => Object.hash(doc, score);
}

/// Memory subsystem accessed through [MobileclawAgent.memory].
abstract class MobileclawMemory {
  Future<MemoryDoc> store(
    String path,
    String content,
    MemoryCategory category,
  );

  Future<List<SearchResult>> recall(
    String query, {
    int limit = 10,
    MemoryCategory? category,
    int? since,
    int? until,
  });

  Future<MemoryDoc?> get(String path);

  Future<bool> forget(String path);

  Future<int> count();
}
