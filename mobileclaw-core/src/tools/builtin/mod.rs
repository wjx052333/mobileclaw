pub mod file;
pub mod http;
pub mod memory_tools;
pub mod system;

use crate::tools::ToolRegistry;
use std::sync::Arc;

/// Register all builtin tools into a registry
pub fn register_all_builtins(registry: &mut ToolRegistry) {
    registry.register_builtin(Arc::new(file::FileReadTool));
    registry.register_builtin(Arc::new(file::FileWriteTool));
    registry.register_builtin(Arc::new(http::HttpTool));
    registry.register_builtin(Arc::new(memory_tools::MemorySearchTool));
    registry.register_builtin(Arc::new(memory_tools::MemoryWriteTool));
    registry.register_builtin(Arc::new(system::TimeTool));
    registry.register_builtin(Arc::new(system::GrepTool));
    registry.register_builtin(Arc::new(system::GlobTool));
}
