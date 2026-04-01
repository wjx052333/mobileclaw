use async_trait::async_trait;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

use crate::{memory::Memory, ClawResult};

use super::permission::{Permission, PermissionChecker};

pub struct ToolContext {
    pub memory: Arc<dyn Memory>,
    pub sandbox_dir: PathBuf,
    pub http_allowlist: Vec<String>,
    pub permissions: Arc<PermissionChecker>,
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
