use async_trait::async_trait;
use serde_json::{json, Value};
use crate::{ClawError, ClawResult,
    memory::{MemoryCategory, SearchQuery},
    tools::{Permission, Tool, ToolContext, ToolResult},
};

pub struct MemorySearchTool;
#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str { "memory_search" }
    fn description(&self) -> &str { "Full-text search in Memory" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 5}
            },
            "required": ["query"]
        })
    }
    fn required_permissions(&self) -> Vec<Permission> { vec![Permission::MemoryRead] }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult> {
        let query = args["query"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'query'".into() })?;
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;
        let results = ctx.memory.recall(&SearchQuery { text: query.into(), limit, ..Default::default() }).await?;
        let items: Vec<Value> = results.iter().map(|r| json!({
            "path": r.doc.path,
            "content": r.doc.content,
            "score": r.score,
        })).collect();
        Ok(ToolResult::ok(json!({"results": items})))
    }
}

pub struct MemoryWriteTool;
#[async_trait]
impl Tool for MemoryWriteTool {
    fn name(&self) -> &str { "memory_write" }
    fn description(&self) -> &str { "Write a document to Memory" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path":     {"type": "string"},
                "content":  {"type": "string"},
                "category": {"type": "string", "enum": ["core", "daily", "conversation"], "default": "core"}
            },
            "required": ["path", "content"]
        })
    }
    fn required_permissions(&self) -> Vec<Permission> { vec![Permission::MemoryWrite] }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult> {
        let path = args["path"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'path'".into() })?;
        let content = args["content"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'content'".into() })?;
        let category = match args["category"].as_str().unwrap_or("core") {
            "daily" => MemoryCategory::Daily,
            "conversation" => MemoryCategory::Conversation,
            _ => MemoryCategory::Core,
        };
        ctx.memory.store(path, content, category).await?;
        Ok(ToolResult::ok(json!({"stored": path})))
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
            secrets: Arc::new(crate::secrets::store::test_helpers::NullSecretStore),
        }
    }

    #[tokio::test]
    async fn memory_write_stores_document() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let tool = MemoryWriteTool;
        let result = tool.execute(
            serde_json::json!({"path": "foo.md", "content": "hello", "category": "core"}),
            &ctx,
        ).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output["stored"], "foo.md");
    }

    #[tokio::test]
    async fn memory_search_returns_matching_doc() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        MemoryWriteTool.execute(
            serde_json::json!({"path": "notes.md", "content": "Tokio async runtime", "category": "core"}),
            &ctx,
        ).await.unwrap();
        let result = MemorySearchTool.execute(
            serde_json::json!({"query": "async", "limit": 5}),
            &ctx,
        ).await.unwrap();
        assert!(result.success);
        let items = result.output["results"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["path"], "notes.md");
    }

    #[tokio::test]
    async fn memory_search_empty_returns_empty() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = MemorySearchTool.execute(
            serde_json::json!({"query": "nonexistent12345"}),
            &ctx,
        ).await.unwrap();
        assert!(result.output["results"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn memory_write_missing_content_errors() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = MemoryWriteTool.execute(
            serde_json::json!({"path": "foo.md"}),
            &ctx,
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn memory_search_missing_query_errors() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = MemorySearchTool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn memory_write_daily_category() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = MemoryWriteTool.execute(
            serde_json::json!({"path": "day.md", "content": "standup notes", "category": "daily"}),
            &ctx,
        ).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn memory_write_conversation_category() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = MemoryWriteTool.execute(
            serde_json::json!({"path": "chat.md", "content": "dialogue", "category": "conversation"}),
            &ctx,
        ).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn memory_write_missing_path_errors() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = MemoryWriteTool.execute(
            serde_json::json!({"content": "orphan"}),
            &ctx,
        ).await;
        assert!(result.is_err());
    }

    #[test]
    fn tool_metadata() {
        let s = MemorySearchTool;
        assert_eq!(s.name(), "memory_search");
        assert!(!s.description().is_empty());
        assert!(!s.parameters_schema().is_null());
        assert!(s.required_permissions().contains(&crate::tools::Permission::MemoryRead));

        let w = MemoryWriteTool;
        assert_eq!(w.name(), "memory_write");
        assert!(w.required_permissions().contains(&crate::tools::Permission::MemoryWrite));
    }
}
