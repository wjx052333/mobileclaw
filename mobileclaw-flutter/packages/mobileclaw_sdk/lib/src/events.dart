/// Sealed base class for all events emitted during a chat turn.
sealed class AgentEvent {
  const AgentEvent();
}

/// A fragment of assistant text is available to display.
final class TextDeltaEvent extends AgentEvent {
  const TextDeltaEvent({required this.text});
  final String text;
}

/// The agent is about to execute a tool.
final class ToolCallEvent extends AgentEvent {
  const ToolCallEvent({required this.toolName});
  final String toolName;
}

/// A tool execution has completed.
final class ToolResultEvent extends AgentEvent {
  const ToolResultEvent({required this.toolName, required this.success});
  final String toolName;
  final bool success;
}

/// The turn is complete. No further events will be emitted on this stream.
final class DoneEvent extends AgentEvent {
  const DoneEvent();
}
