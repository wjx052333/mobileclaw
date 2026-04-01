# Tool Design: mobileclaw-core

## Overview

The tool layer provides a uniform interface for the LLM to perform side-effecting
operations. All tools are async, permission-gated, and sandbox-constrained. The design
goals are:

- **Uniform interface** — the LLM calls all tools the same way regardless of their
  implementation.
- **Dependency injection** — all runtime dependencies are passed through `ToolContext`
  at construction time; there are no global singletons.
- **Namespace protection** — builtin tool names are immutable; extensions cannot shadow
  them.

---

## Tool Trait Interface

Defined in `src/tools/traits.rs`:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;       // JSON Schema object
    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult>;

    fn required_permissions(&self) -> Vec<Permission> { vec![] }  // default: none
    fn timeout_ms(&self) -> u64 { 10_000 }                        // default: 10 s
}
```

| Method | Purpose |
|--------|---------|
| `name()` | Unique identifier used in XML `<tool_call>` dispatch |
| `description()` | Shown to the LLM in the system prompt tool list |
| `parameters_schema()` | JSON Schema describing accepted `args`; used for LLM prompt construction |
| `execute()` | Async execution; receives parsed JSON args and the shared `ToolContext` |
| `required_permissions()` | Permissions the caller must grant; checked by `PermissionChecker` |
| `timeout_ms()` | Per-tool timeout hint; `HttpTool` overrides to `15_000` ms |

`ToolResult` carries a `success: bool` flag and a `serde_json::Value` output payload.
`ToolResult::ok(v)` and `ToolResult::err(msg)` are the canonical constructors.

---

## ToolContext: Dependency Injection

```rust
pub struct ToolContext {
    pub memory: Arc<dyn Memory>,
    pub sandbox_dir: PathBuf,
    pub http_allowlist: Vec<String>,
    pub permissions: Arc<PermissionChecker>,
}
```

`ToolContext` is constructed once by the application shell and passed by reference to
every `execute()` call. No tool accesses global state.

| Field | Type | Purpose |
|-------|------|---------|
| `memory` | `Arc<dyn Memory>` | Shared memory backend (SqliteMemory in production) |
| `sandbox_dir` | `PathBuf` | Absolute root for all file operations; enforced by `resolve_sandbox_path` |
| `http_allowlist` | `Vec<String>` | Allowed base URLs; enforced by `is_url_allowed` |
| `permissions` | `Arc<PermissionChecker>` | Runtime permission grants; checked before execution |

This design makes tools trivially testable: tests construct a `ToolContext` with a
`TempDir` sandbox, an in-memory SQLite instance, and `PermissionChecker::allow_all()`.

---

## ToolRegistry Protection Mechanism

Defined in `src/tools/registry.rs`:

```
ToolRegistry {
    tools:     HashMap<String, Arc<dyn Tool>>,
    protected: HashSet<String>,
}
```

**Builtin registration** (`register_builtin`):
1. Insert the tool name into `protected`.
2. Insert the tool into `tools`.

**Extension registration** (`register_extension`):
1. If the name is in `protected`, return `Err(ClawError::ToolNameConflict(name))`.
2. Otherwise, insert the tool into `tools` and return `Ok(())`.

This is a one-way ratchet: once a name is protected it can never be overwritten by an
extension. The application shell calls `register_all_builtins(registry)` during startup,
which registers all eight builtins atomically before any extension loading occurs.

---

## Builtin Tools

| Struct | Tool Name | Required Permission | Timeout |
|--------|-----------|-------------------|---------|
| `FileReadTool` | `file_read` | `FileRead` | 10 s |
| `FileWriteTool` | `file_write` | `FileWrite` | 10 s |
| `HttpTool` | `http_request` | `HttpFetch` | 15 s |
| `MemorySearchTool` | `memory_search` | `MemoryRead` | 10 s |
| `MemoryWriteTool` | `memory_write` | `MemoryWrite` | 10 s |
| `TimeTool` | `time` | (none) | 10 s |
| `GrepTool` | `grep` | `FileRead` | 10 s |
| `GlobTool` | `glob` | `FileRead` | 10 s |

All builtins are registered by `tools::builtin::register_all_builtins` in
`src/tools/builtin/mod.rs`.

---

## Extension Tool Registration Flow

```
Application shell
    │
    ├── let mut registry = ToolRegistry::new();
    ├── register_all_builtins(&mut registry);    // protected set populated
    │
    ├── registry.register_extension(Arc::new(MyCustomTool))
    │       │
    │       ├── name in protected?
    │       │     yes → Err(ToolNameConflict)  ← returned to caller
    │       │     no  → tools.insert(name, tool)
    │       │           Ok(())  ← returned to caller
    │       │
    └── AgentLoop::new(llm, registry, ctx, skill_mgr)
```

Extension tools must implement the full `Tool` trait. There is no separate "extension
trait" — the same interface is used for builtins and extensions. The only behavioral
difference is that extensions cannot use protected names.

---

## XML Protocol

### Tool Call (LLM → agent)

The LLM signals a tool invocation by emitting an XML block in its text output:

```xml
<tool_call>{"name":"file_read","args":{"path":"notes.md"}}</tool_call>
```

The JSON object inside the tag must have a `name` field (string) and optionally an `args`
field (any JSON value; defaults to `null` if absent). The parser
(`agent::parser::extract_tool_calls`) scans the full LLM response for all such blocks
and returns them as `Vec<ToolCall>`. Malformed JSON is skipped with a `tracing::warn!`
log.

Multiple tool calls may appear in a single LLM response; all are executed before the
next LLM round:

```xml
<tool_call>{"name":"time","args":{}}</tool_call>
<tool_call>{"name":"memory_search","args":{"query":"rust"}}</tool_call>
```

### Tool Result (agent → LLM)

After execution, results are serialized back into the conversation history as XML:

```xml
<tool_result name="file_read" status="ok">{"content":"hello world","path":"notes.md"}</tool_result>
<tool_result name="memory_search" status="error">"tool not found"</tool_result>
```

The `status` attribute is either `"ok"` or `"error"`. The body is a JSON-serialized
`serde_json::Value` — the `output` field of `ToolResult`. On execution error the body
is `{"error": "<ClawError display string>"}`.

Tool results are appended to the assistant message, followed by a user-turn continuation
prompt:

```
[tool results provided above, please continue]
```

This gives the LLM full visibility into both the tool call it made and the result, in a
single assistant turn, before it generates the next response.
