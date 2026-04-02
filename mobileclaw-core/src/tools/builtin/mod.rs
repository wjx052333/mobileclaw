pub mod email;
pub mod file;
pub mod http;
pub mod memory_tools;
pub mod system;

use crate::tools::ToolRegistry;
use std::sync::Arc;

/// Register all non-email builtin tools. Always called at session creation.
pub fn register_core_builtins(registry: &mut ToolRegistry) {
    registry.register_builtin(Arc::new(file::FileReadTool));
    registry.register_builtin(Arc::new(file::FileWriteTool));
    registry.register_builtin(Arc::new(http::HttpTool));
    registry.register_builtin(Arc::new(memory_tools::MemorySearchTool));
    registry.register_builtin(Arc::new(memory_tools::MemoryWriteTool));
    registry.register_builtin(Arc::new(system::TimeTool));
    registry.register_builtin(Arc::new(system::GrepTool));
    registry.register_builtin(Arc::new(system::GlobTool));
}

/// Register email tools. Only call when at least one email account is configured.
pub fn register_email_builtins(registry: &mut ToolRegistry) {
    registry.register_builtin(Arc::new(email::EmailSendTool));
    registry.register_builtin(Arc::new(email::EmailFetchTool));
}

/// Register all builtins (core + email). Kept for test convenience.
pub fn register_all_builtins(registry: &mut ToolRegistry) {
    register_core_builtins(registry);
    register_email_builtins(registry);
}
