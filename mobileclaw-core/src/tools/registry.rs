use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::{ClawError, ClawResult};

use super::traits::Tool;

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    protected: HashSet<String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            protected: HashSet::new(),
        }
    }

    pub fn register_builtin(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.protected.insert(name.clone());
        self.tools.insert(name, tool);
    }

    pub fn register_extension(&mut self, tool: Arc<dyn Tool>) -> ClawResult<()> {
        let name = tool.name().to_string();
        if self.protected.contains(&name) {
            return Err(ClawError::ToolNameConflict(name));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn list(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.values().cloned().collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::traits::{Tool, ToolContext, ToolResult};
    use async_trait::async_trait;

    struct FakeTool(String);

    #[async_trait]
    impl Tool for FakeTool {
        fn name(&self) -> &str {
            &self.0
        }

        fn description(&self) -> &str {
            "fake"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }

        async fn execute(
            &self,
            _: serde_json::Value,
            _: &ToolContext,
        ) -> ClawResult<ToolResult> {
            Ok(ToolResult::ok("ok"))
        }
    }

    #[test]
    fn builtin_names_are_protected() {
        let mut reg = ToolRegistry::new();
        reg.register_builtin(Arc::new(FakeTool("file_read".into())));
        let result = reg.register_extension(Arc::new(FakeTool("file_read".into())));
        assert!(matches!(result, Err(ClawError::ToolNameConflict(_))));
    }

    #[test]
    fn extension_tool_registers_successfully() {
        let mut reg = ToolRegistry::new();
        reg.register_builtin(Arc::new(FakeTool("file_read".into())));
        let result = reg.register_extension(Arc::new(FakeTool("my_custom_tool".into())));
        assert!(result.is_ok());
        assert!(reg.get("my_custom_tool").is_some());
    }

    #[test]
    fn get_unknown_tool_returns_none() {
        let reg = ToolRegistry::new();
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn list_tools_returns_all() {
        let mut reg = ToolRegistry::new();
        reg.register_builtin(Arc::new(FakeTool("a".into())));
        reg.register_builtin(Arc::new(FakeTool("b".into())));
        assert_eq!(reg.list().len(), 2);
    }
}
