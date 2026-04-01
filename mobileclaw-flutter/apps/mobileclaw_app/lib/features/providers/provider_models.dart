/// App-side enum mirroring the Rust `ProviderProtocol`.
/// Carries UI metadata: display name and URL hint.
enum ProviderProtocol {
  anthropic(
    value: 'anthropic',
    displayName: 'Anthropic (Claude)',
    urlHint: 'https://api.anthropic.com',
  ),
  openAiCompat(
    value: 'openai_compat',
    displayName: 'OpenAI-compatible',
    urlHint: 'https://api.openai.com',
  ),
  ollama(
    value: 'ollama',
    displayName: 'Ollama (local)',
    urlHint: 'http://localhost:11434',
  );

  const ProviderProtocol({
    required this.value,
    required this.displayName,
    required this.urlHint,
  });

  /// The wire string sent to / received from Rust FFI.
  final String value;

  /// Human-readable name shown in the protocol picker.
  final String displayName;

  /// Placeholder URL shown in the URL field when this protocol is selected.
  final String urlHint;

  static ProviderProtocol fromValue(String value) => values.firstWhere(
        (p) => p.value == value,
        orElse: () => throw ArgumentError('Unknown protocol: $value'),
      );
}
