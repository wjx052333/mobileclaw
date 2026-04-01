use async_trait::async_trait;
use serde_json::{json, Value};
use crate::{ClawResult, tools::{Tool, ToolContext, ToolResult}};

pub struct TimeTool;
#[async_trait]
impl Tool for TimeTool {
    fn name(&self) -> &str { "time" }
    fn description(&self) -> &str { "Returns current UTC unix timestamp" }
    fn parameters_schema(&self) -> Value { json!({"type": "object", "properties": {}}) }
    async fn execute(&self, _: Value, _: &ToolContext) -> ClawResult<ToolResult> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        Ok(ToolResult::ok(json!({"unix_timestamp": secs})))
    }
}

pub struct GrepTool;
#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str { "grep" }
    fn description(&self) -> &str { "Search for regex pattern in a sandbox file" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regular expression"},
                "path":    {"type": "string", "description": "Relative file path"}
            },
            "required": ["pattern", "path"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult> {
        use regex::Regex;
        use crate::{ClawError, tools::builtin::file::resolve_sandbox_path};
        let pattern = args["pattern"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'pattern'".into() })?;
        let path_str = args["path"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'path'".into() })?;
        let resolved = resolve_sandbox_path(&ctx.sandbox_dir, path_str)?;
        let content = tokio::fs::read_to_string(&resolved).await
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;
        let re = Regex::new(pattern)
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: format!("invalid regex: {}", e) })?;
        let matches: Vec<Value> = content.lines().enumerate()
            .filter(|(_, line)| re.is_match(line))
            .map(|(i, line)| json!({"line": i + 1, "content": line}))
            .collect();
        Ok(ToolResult::ok(json!({"matches": matches})))
    }
}

pub struct GlobTool;
#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str { "glob" }
    fn description(&self) -> &str { "List files matching a glob pattern in the sandbox" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob pattern like '**/*.md'"}
            },
            "required": ["pattern"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult> {
        use crate::ClawError;
        let pattern = args["pattern"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'pattern'".into() })?;
        // Scope pattern to sandbox to prevent escaping
        let full_pattern = ctx.sandbox_dir.join(pattern);
        let full_pattern_str = full_pattern.to_string_lossy();
        let paths: Vec<Value> = glob::glob(&full_pattern_str)
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?
            .filter_map(|entry| entry.ok())
            .filter_map(|p| {
                // Strip sandbox prefix, return relative path
                p.strip_prefix(&ctx.sandbox_dir).ok()
                    .map(|rel| Value::String(rel.to_string_lossy().to_string()))
            })
            .collect();
        Ok(ToolResult::ok(json!({"paths": paths})))
    }
}
