pub mod loop_impl;
pub mod parser;

pub use parser::{ToolCall, extract_tool_calls, format_tool_result};
// AgentLoop re-export added in Task 12 after loop_impl is implemented
