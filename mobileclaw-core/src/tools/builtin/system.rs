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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        memory::sqlite::SqliteMemory,
        tools::{ToolContext, PermissionChecker},
    };
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn make_ctx(dir: &TempDir) -> ToolContext {
        let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
        ToolContext {
            memory: mem,
            sandbox_dir: dir.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
        }
    }

    #[tokio::test]
    async fn time_tool_returns_positive_timestamp() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = TimeTool.execute(serde_json::json!({}), &ctx).await.unwrap();
        assert!(result.success);
        let ts = result.output["unix_timestamp"].as_u64().expect("unix_timestamp should be u64");
        assert!(ts > 0, "unix timestamp must be positive");
    }

    #[tokio::test]
    async fn grep_tool_finds_matching_lines() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        // Write a file to the sandbox
        let file_path = dir.path().join("sample.txt");
        tokio::fs::write(&file_path, "hello world\nrust is fast\nhello again").await.unwrap();
        let result = GrepTool.execute(
            serde_json::json!({"pattern": "hello", "path": "sample.txt"}),
            &ctx,
        ).await.unwrap();
        assert!(result.success);
        let matches = result.output["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0]["line"], 1);
        assert_eq!(matches[1]["line"], 3);
    }

    #[tokio::test]
    async fn grep_tool_no_matches_returns_empty() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        tokio::fs::write(dir.path().join("empty.txt"), "nothing here").await.unwrap();
        let result = GrepTool.execute(
            serde_json::json!({"pattern": "zzz_not_found", "path": "empty.txt"}),
            &ctx,
        ).await.unwrap();
        assert!(result.output["matches"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn grep_tool_invalid_regex_returns_error() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        tokio::fs::write(dir.path().join("f.txt"), "content").await.unwrap();
        let result = GrepTool.execute(
            serde_json::json!({"pattern": "[invalid(regex", "path": "f.txt"}),
            &ctx,
        ).await;
        assert!(result.is_err() || !result.unwrap().success);
    }

    #[tokio::test]
    async fn glob_tool_finds_files() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        tokio::fs::write(dir.path().join("a.md"), "doc a").await.unwrap();
        tokio::fs::write(dir.path().join("b.md"), "doc b").await.unwrap();
        tokio::fs::write(dir.path().join("c.txt"), "not md").await.unwrap();
        let result = GlobTool.execute(
            serde_json::json!({"pattern": "*.md"}),
            &ctx,
        ).await.unwrap();
        assert!(result.success);
        let paths = result.output["paths"].as_array().unwrap();
        assert_eq!(paths.len(), 2);
    }

    #[tokio::test]
    async fn glob_tool_empty_pattern_returns_nothing() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = GlobTool.execute(
            serde_json::json!({"pattern": "*.nonexistent"}),
            &ctx,
        ).await.unwrap();
        assert!(result.output["paths"].as_array().unwrap().is_empty());
    }

    #[test]
    fn tool_metadata_time() {
        assert_eq!(TimeTool.name(), "time");
        assert!(!TimeTool.description().is_empty());
        let schema = TimeTool.parameters_schema();
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn tool_metadata_grep() {
        assert_eq!(GrepTool.name(), "grep");
        assert!(!GrepTool.description().is_empty());
        let schema = GrepTool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["required"].as_array().unwrap().contains(&serde_json::json!("pattern")));
    }

    #[test]
    fn tool_metadata_glob() {
        assert_eq!(GlobTool.name(), "glob");
        assert!(!GlobTool.description().is_empty());
        let schema = GlobTool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["required"].as_array().unwrap().contains(&serde_json::json!("pattern")));
    }

    #[tokio::test]
    async fn grep_tool_missing_pattern_returns_error() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = GrepTool.execute(
            serde_json::json!({"path": "f.txt"}),
            &ctx,
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn grep_tool_missing_path_returns_error() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = GrepTool.execute(
            serde_json::json!({"pattern": "hello"}),
            &ctx,
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn glob_tool_missing_pattern_returns_error() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = GlobTool.execute(
            serde_json::json!({}),
            &ctx,
        ).await;
        assert!(result.is_err());
    }
}
