/// Sealed base class for all events emitted during a chat turn.
sealed class AgentEvent {
  const AgentEvent();
}

/// A fragment of assistant text is available to display.
final class TextDeltaEvent extends AgentEvent {
  const TextDeltaEvent({required this.text});
  final String text;

  @override
  bool operator ==(Object other) =>
      other is TextDeltaEvent && other.text == text;

  @override
  int get hashCode => text.hashCode;
}

/// The agent is about to execute a tool.
final class ToolCallEvent extends AgentEvent {
  const ToolCallEvent({required this.toolName});
  final String toolName;

  @override
  bool operator ==(Object other) =>
      other is ToolCallEvent && other.toolName == toolName;

  @override
  int get hashCode => toolName.hashCode;
}

/// A tool execution has completed.
final class ToolResultEvent extends AgentEvent {
  const ToolResultEvent({required this.toolName, required this.success});
  final String toolName;
  final bool success;

  @override
  bool operator ==(Object other) =>
      other is ToolResultEvent &&
      other.toolName == toolName &&
      other.success == success;

  @override
  int get hashCode => Object.hash(toolName, success);
}

/// The turn is complete. No further events will be emitted on this stream.
final class DoneEvent extends AgentEvent {
  const DoneEvent();

  @override
  bool operator ==(Object other) => other is DoneEvent;

  @override
  int get hashCode => 0;
}
