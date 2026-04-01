use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use crate::{ClawError, ClawResult, tools::{Permission, Tool, ToolContext, ToolResult}};

/// Resolves a user-provided path into an absolute path within the sandbox.
/// Rejects any attempt to escape the sandbox via ../ traversal or absolute paths.
pub fn resolve_sandbox_path(sandbox: &Path, user_path: &str) -> ClawResult<PathBuf> {
    // Reject absolute paths
    if Path::new(user_path).is_absolute() {
        return Err(ClawError::PathTraversal(user_path.to_string()));
    }
    // Build candidate path
    let candidate = sandbox.join(user_path);
    // Manually normalize by processing components (handles .. without canonicalize)
    let mut components = Vec::new();
    for c in candidate.components() {
        match c {
            std::path::Component::ParentDir => {
                if components.is_empty() {
                    return Err(ClawError::PathTraversal(user_path.to_string()));
                }
                components.pop();
            }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    let resolved: PathBuf = components.iter().collect();
    // Final check: resolved path must be within sandbox
    if !resolved.starts_with(sandbox) {
        return Err(ClawError::PathTraversal(user_path.to_string()));
    }
    Ok(resolved)
}

pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str { "file_read" }
    fn description(&self) -> &str { "Read file content from within the sandbox directory" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"path": {"type": "string", "description": "Path relative to sandbox root"}},
            "required": ["path"]
        })
    }
    fn required_permissions(&self) -> Vec<Permission> { vec![Permission::FileRead] }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult> {
        let path_str = args["path"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'path'".into() })?;
        let resolved = resolve_sandbox_path(&ctx.sandbox_dir, path_str)?;
        let content = tokio::fs::read_to_string(&resolved).await
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;
        Ok(ToolResult::ok(json!({"content": content, "path": path_str})))
    }
}

pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str { "file_write" }
    fn description(&self) -> &str { "Write content to a file within the sandbox directory" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        })
    }
    fn required_permissions(&self) -> Vec<Permission> { vec![Permission::FileWrite] }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult> {
        let path_str = args["path"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'path'".into() })?;
        let content = args["content"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'content'".into() })?;
        let resolved = resolve_sandbox_path(&ctx.sandbox_dir, path_str)?;
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;
        }
        tokio::fs::write(&resolved, content).await
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;
        Ok(ToolResult::ok(json!({"written": content.len(), "path": path_str})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::traits::ToolContext;
    use proptest::prelude::*;
    use tempfile::TempDir;

    async fn make_ctx(sandbox: &TempDir) -> ToolContext {
        use crate::{memory::sqlite::SqliteMemory, tools::permission::PermissionChecker};
        use std::sync::Arc;
        let db = sandbox.path().join("mem.db");
        let mem = SqliteMemory::open(&db).await.unwrap();
        ToolContext {
            memory: Arc::new(mem),
            sandbox_dir: sandbox.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: Arc::new(crate::secrets::store::test_helpers::NullSecretStore),
        }
    }

    #[tokio::test]
    async fn file_write_and_read_roundtrip() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let writer = FileWriteTool;
        let reader = FileReadTool;
        writer.execute(
            serde_json::json!({"path": "test.txt", "content": "hello"}),
            &ctx,
        ).await.unwrap();
        let result = reader.execute(
            serde_json::json!({"path": "test.txt"}),
            &ctx,
        ).await.unwrap();
        assert_eq!(result.output["content"], "hello");
    }

    #[tokio::test]
    async fn path_traversal_is_rejected() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let reader = FileReadTool;
        let err = reader.execute(
            serde_json::json!({"path": "../../../etc/passwd"}),
            &ctx,
        ).await;
        assert!(err.is_err());
        assert!(matches!(err.unwrap_err(), crate::ClawError::PathTraversal(_)));
    }

    proptest! {
        #[test]
        fn no_path_traversal_escapes_sandbox(
            segments in proptest::collection::vec(
                r"[a-zA-Z0-9._-]{1,16}",
                1..8
            )
        ) {
            let dir = TempDir::new().unwrap();
            let sandbox = dir.path().to_path_buf();
            let mut path = segments.join("/");
            path = format!("../../{}", path);
            let result = resolve_sandbox_path(&sandbox, &path);
            if let Ok(resolved) = result {
                prop_assert!(resolved.starts_with(&sandbox));
            }
            // Err is also acceptable (rejected)
        }
    }

    #[tokio::test]
    async fn file_write_creates_nested_directories() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = FileWriteTool.execute(
            serde_json::json!({"path": "subdir/nested/file.txt", "content": "nested"}),
            &ctx,
        ).await.unwrap();
        assert!(result.success);
        let content = tokio::fs::read_to_string(dir.path().join("subdir/nested/file.txt")).await.unwrap();
        assert_eq!(content, "nested");
    }

    #[tokio::test]
    async fn file_read_nonexistent_returns_error() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = FileReadTool.execute(
            serde_json::json!({"path": "does_not_exist.txt"}),
            &ctx,
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn file_write_absolute_path_rejected() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = FileWriteTool.execute(
            serde_json::json!({"path": "/etc/passwd", "content": "hack"}),
            &ctx,
        ).await;
        assert!(matches!(result, Err(crate::ClawError::PathTraversal(_))));
    }

    #[test]
    fn resolve_sandbox_path_current_dir_component() {
        let dir = TempDir::new().unwrap();
        let sandbox = dir.path().to_path_buf();
        // "." should resolve to sandbox itself
        let result = resolve_sandbox_path(&sandbox, "foo/./bar.txt");
        assert!(result.is_ok());
        assert!(result.unwrap().starts_with(&sandbox));
    }

    #[test]
    fn file_read_tool_metadata() {
        assert_eq!(FileReadTool.name(), "file_read");
        assert!(!FileReadTool.description().is_empty());
        let schema = FileReadTool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["required"].as_array().unwrap().contains(&serde_json::json!("path")));
    }

    #[test]
    fn file_write_tool_metadata() {
        assert_eq!(FileWriteTool.name(), "file_write");
        assert!(!FileWriteTool.description().is_empty());
        let schema = FileWriteTool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["required"].as_array().unwrap().contains(&serde_json::json!("path")));
    }

    #[tokio::test]
    async fn file_read_missing_path_arg_returns_error() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = FileReadTool.execute(
            serde_json::json!({}),
            &ctx,
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn file_write_missing_content_arg_returns_error() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = FileWriteTool.execute(
            serde_json::json!({"path": "out.txt"}),
            &ctx,
        ).await;
        assert!(result.is_err());
    }
}
