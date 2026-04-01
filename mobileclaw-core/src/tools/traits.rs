use async_trait::async_trait;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

use crate::{memory::Memory, secrets::SecretStore, ClawResult};

use super::permission::{Permission, PermissionChecker};

pub struct ToolContext {
    pub memory: Arc<dyn Memory>,
    pub sandbox_dir: PathBuf,
    pub http_allowlist: Vec<String>,
    pub permissions: Arc<PermissionChecker>,
    pub secrets: Arc<dyn SecretStore>,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub success: bool,
    pub output: Value,
}

impl ToolResult {
    pub fn ok(output: impl Into<Value>) -> Self {
        Self {
            success: true,
            output: output.into(),
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            output: Value::String(msg.into()),
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult>;

    fn required_permissions(&self) -> Vec<Permission> {
        vec![]
    }

    fn timeout_ms(&self) -> u64 {
        10_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_result_ok_sets_success_true() {
        let r = ToolResult::ok("hello");
        assert!(r.success);
        assert_eq!(r.output, serde_json::json!("hello"));
    }

    #[test]
    fn tool_result_err_sets_success_false() {
        let r = ToolResult::err("something went wrong");
        assert!(!r.success);
        assert_eq!(r.output, serde_json::json!("something went wrong"));
    }

    #[test]
    fn tool_result_ok_with_object() {
        let r = ToolResult::ok(serde_json::json!({"key": "value"}));
        assert!(r.success);
        assert_eq!(r.output["key"], "value");
    }

    #[test]
    fn tool_default_required_permissions_is_empty() {
        use crate::tools::builtin::system::TimeTool;
        // TimeTool doesn't override required_permissions, so it returns the default []
        let perms = TimeTool.required_permissions();
        assert!(perms.is_empty());
    }

    #[test]
    fn tool_default_timeout_ms() {
        use crate::tools::builtin::system::TimeTool;
        // TimeTool doesn't override timeout_ms, so it returns the default 10_000
        assert_eq!(TimeTool.timeout_ms(), 10_000);
    }
}
