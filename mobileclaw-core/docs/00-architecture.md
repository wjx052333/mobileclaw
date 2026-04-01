# Architecture: mobileclaw-core

## Overview

`mobileclaw-core` is the Rust core of the mobileclaw mobile AI agent engine. MVP Phase 1
delivers a self-contained async agent loop that connects a streaming LLM (Claude API) to a
sandboxed tool execution layer and a persistent SQLite memory store. The crate is designed
to be embedded in a mobile application shell (iOS / Android via FFI or a CLI harness).

## Layer Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                        Caller / App Shell                        │
└──────────────────────────┬───────────────────────────────────────┘
                           │ user_input: &str
                           ▼
┌──────────────────────────────────────────────────────────────────┐
│                          AgentLoop                               │
│   chat(user_input, base_system) -> Vec<AgentEvent>               │
│                                                                  │
│   ┌────────────┐   ┌────────────────┐   ┌──────────────────────┐│
│   │SkillManager│   │  ToolRegistry  │   │     ToolContext       ││
│   │keyword     │   │ builtin +      │   │  memory / sandbox /  ││
│   │match +     │   │ extension      │   │  http_allowlist /    ││
│   │prompt build│   │ tools          │   │  permissions         ││
│   └────────────┘   └────────────────┘   └──────────────────────┘│
│                           │                                      │
│   ┌───────────────────────▼──────────────────────────────────┐  │
│   │              LlmClient trait (ClaudeClient)               │  │
│   │   stream_messages(system, history, max_tokens)            │  │
│   └───────────────────────┬──────────────────────────────────┘  │
└───────────────────────────┼──────────────────────────────────────┘
                            │ HTTPS SSE
                            ▼
                    ┌───────────────┐
                    │   Claude API  │
                    │ (Anthropic)   │
                    └───────────────┘

Side store:
┌───────────────────────────────────────┐
│       SqliteMemory (WAL + FTS5)       │
│  documents table + docs_fts virtual   │
└───────────────────────────────────────┘
```

## Data Flow

```
user input
    │
    ▼
SkillManager::match_skills()          -- keyword scan, O(skills × keywords)
    │ matched skills
    ▼
SkillManager::build_system_prompt()   -- append skill prompt blocks
    │ system: String
    ▼
history.push(Message::user(input))
    │
    ▼
[loop: up to MAX_TOOL_ROUNDS = 10]
    │
    ├─► LlmClient::stream_messages()  -- *** PERF CRITICAL: network I/O ***
    │       SSE stream -> TextDelta events -> full_text: String
    │
    ├─► agent::parser::extract_tool_calls(full_text)
    │       scan for <tool_call>…</tool_call> blocks, parse JSON
    │
    │   if no tool calls:
    │       history.push(Message::assistant(full_text))
    │       emit AgentEvent::Done  →  break
    │
    │   for each ToolCall:
    │       ├─► ToolRegistry::get(name)
    │       ├─► Tool::execute(args, ctx)  -- *** PERF CRITICAL: tool I/O ***
    │       └─► format_tool_result() -> XML string
    │
    ├─► history.push(Message::assistant(clean_text + tool_results_xml))
    └─► history.push(Message::user("[tool results provided above, please continue]"))
```

## Module Responsibility Table

| Module | Path | Responsibility |
|--------|------|----------------|
| `error` | `src/error.rs` | `ClawError` enum and `ClawResult<T>` type alias; all error variants |
| `llm::types` | `src/llm/types.rs` | `Message`, `StreamEvent` data types |
| `llm::client` | `src/llm/client.rs` | `LlmClient` trait; `ClaudeClient` SSE implementation |
| `agent::parser` | `src/agent/parser.rs` | XML `<tool_call>` extraction; `format_tool_result` serializer |
| `agent::loop_impl` | `src/agent/loop_impl.rs` | `AgentLoop` struct; multi-round tool execution orchestration |
| `memory::types` | `src/memory/types.rs` | `MemoryDoc`, `MemoryCategory`, `SearchQuery`, `SearchResult` |
| `memory::sqlite` | `src/memory/sqlite.rs` | `SqliteMemory`: WAL SQLite backend, FTS5 search, UPSERT logic |
| `skill::types` | `src/skill/types.rs` | `Skill`, `SkillManifest`, `SkillTrust`, `SkillActivation` |
| `skill::loader` | `src/skill/loader.rs` | Load skill TOML/directory bundles from filesystem |
| `skill::manager` | `src/skill/manager.rs` | Keyword matching; system prompt assembly |
| `tools::traits` | `src/tools/traits.rs` | `Tool` async trait; `ToolContext`; `ToolResult` |
| `tools::permission` | `src/tools/permission.rs` | `Permission` enum; `PermissionChecker` |
| `tools::registry` | `src/tools/registry.rs` | `ToolRegistry`: builtin protection + extension registration |
| `tools::builtin::file` | `src/tools/builtin/file.rs` | `FileReadTool`, `FileWriteTool`; `resolve_sandbox_path` |
| `tools::builtin::http` | `src/tools/builtin/http.rs` | `HttpTool`; `is_url_allowed` allowlist check |
| `tools::builtin::memory_tools` | `src/tools/builtin/memory_tools.rs` | `MemorySearchTool`, `MemoryWriteTool` |
| `tools::builtin::system` | `src/tools/builtin/system.rs` | `TimeTool`, `GrepTool`, `GlobTool` |

## Performance-Critical Paths

| Path | Why Critical | Notes |
|------|-------------|-------|
| `LlmClient::stream_messages` | Network round-trip dominates total latency | Streams SSE to avoid buffering full response |
| `AgentLoop` round loop | Up to 10 LLM calls per `chat()` invocation | Short-circuit on zero tool calls; limit is configurable |
| `Tool::execute` | File / HTTP / SQLite I/O | All async via Tokio; timeouts enforced per tool (`timeout_ms()`) |
| `SqliteMemory::recall` (FTS5 BM25) | Full-text search on every `memory_search` call | Covered by FTS5 index + MMAP page cache |
| `agent::parser::extract_tool_calls` | Called on every LLM response | Linear scan, not regex-compiled per call — acceptable at this token scale |
