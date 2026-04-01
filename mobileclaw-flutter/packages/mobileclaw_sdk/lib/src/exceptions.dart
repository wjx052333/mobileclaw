/// Exception thrown when the Rust core returns an Err(ClawError).
class ClawException implements Exception {
  const ClawException({required this.type, required this.message});

  /// Rust variant name, e.g. 'PathTraversal', 'UrlNotAllowed', 'Llm', etc.
  final String type;

  /// Human-readable description from the Rust Display impl.
  final String message;

  factory ClawException.pathTraversal(String path) => ClawException(
        type: 'PathTraversal',
        message: "path traversal attempt: '$path'",
      );

  factory ClawException.urlNotAllowed(String url) => ClawException(
        type: 'UrlNotAllowed',
        message: "url not in allowlist: '$url'",
      );

  factory ClawException.permissionDenied(String reason) => ClawException(
        type: 'PermissionDenied',
        message: 'permission denied: $reason',
      );

  factory ClawException.tool(String tool, String message) => ClawException(
        type: 'Tool',
        message: 'tool error: $tool — $message',
      );

  factory ClawException.llm(String message) => ClawException(
        type: 'Llm',
        message: 'llm error: $message',
      );

  factory ClawException.memory(String message) => ClawException(
        type: 'Memory',
        message: 'memory error: $message',
      );

  factory ClawException.skillLoad(String message) => ClawException(
        type: 'SkillLoad',
        message: 'skill load error: $message',
      );

  @override
  String toString() => 'ClawException($type): $message';
}
