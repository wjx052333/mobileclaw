# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Project Is

**mobileclaw** is a mobile AI agent engine: a Rust core library (`mobileclaw-core`) exposed to Flutter/Dart via FFI (`flutter_rust_bridge` v2.x). The Rust layer handles all agent logic, LLM streaming, tool execution, memory, and encrypted secrets. The Flutter layer provides UI and passes config down at session creation.

## Commands

### Rust Core

```bash
# Build
cargo build -p mobileclaw-core

# Run tests (unit + integration, no MockLlmClient)
cargo test -p mobileclaw-core

# Run full test suite including integration_agent tests
cargo test -p mobileclaw-core --features test-utils

# Run a specific integration test file
cargo test -p mobileclaw-core --features test-utils --test integration_agent

# Lint (must pass with zero warnings)
cargo clippy -p mobileclaw-core --features test-utils -- -D warnings

# Coverage (hard floor: 85% line coverage)
cargo llvm-cov --package mobileclaw-core --features test-utils --all-targets --fail-under-lines 85
```

### Flutter

```bash
flutter pub get
flutter build apk   # or ios
```

## Architecture

### Layered Design

```
Flutter App (UI)
    ↓ FFI via flutter_rust_bridge
AgentSession (src/ffi.rs)  ← opaque handle; all public Rust APIs live here
    ↓
AgentLoop (src/agent/loop_impl.rs)
    ├── LlmClient (src/llm/)         ← ClaudeClient; SSE streaming; hardcoded to api.anthropic.com
    ├── ToolRegistry (src/tools/)    ← 8 builtins + extension tools
    ├── SkillManager (src/skill/)    ← YAML+Markdown skill bundles; keyword matching
    ├── SqliteMemory (src/memory/)   ← FTS5 full-text search; WAL + MMAP
    └── SecretStore (src/secrets/)   ← AES-256-GCM encrypted email credentials
```

### FFI Boundary (`src/ffi.rs`)

`AgentSession` is the single opaque handle crossing the FFI boundary. It is created with `AgentConfig` (api_key, model, db_path, sandbox_dir, http_allowlist, skills_dir, secrets_db_path) and exposes:
- `chat(input, system)` → `Vec<AgentEventDto>` (streaming events: TextDelta, ToolCall, ToolResult, Done)
- Memory API: `store`, `recall`, `get`, `forget`, `count`
- Email account API: `email_account_save`, `email_account_load`, `email_account_delete`
- Skill API: `skills()`, `load_skills_from_dir()`

The `AgentConfig.api_key` and `AgentConfig.model` are passed at construction time from Dart. The LLM URL (`https://api.anthropic.com/v1/messages`) is hardcoded in `src/llm/client.rs`.

### Agent Loop (`src/agent/loop_impl.rs`)

Multi-round loop with `MAX_TOOL_ROUNDS = 10`. Each round:
1. SkillManager keyword-matches user input → appends to system prompt
2. LlmClient streams SSE response
3. `parser.rs` extracts XML tool calls from response text
4. ToolRegistry dispatches each tool call (with sandbox + allowlist enforcement)
5. Tool results formatted back as XML, appended to message history
6. Repeats until no more tool calls or round limit hit

### Security — Three Inviolable Lifelines

These must never be bypassed. All three have `proptest` coverage (256+ cases each):

1. **Path traversal** (`src/tools/builtin/file.rs::resolve_sandbox_path`): manual component normalization (no `canonicalize()`), rejects `../`, absolute paths, null bytes.
2. **URL allowlist** (`src/tools/builtin/http.rs::is_url_allowed`): structural `url` crate parsing, exact host equality (not prefix), HTTPS-only, no userinfo.
3. **Tool name protection** (`src/tools/registry.rs`): builtin names in a `protected` HashSet; extensions cannot shadow builtins.

FTS5 injection: user queries double-quoted before passing to SQLite.

### Memory (`src/memory/sqlite.rs`)

SQLite with WAL + MMAP (64 MiB) + `synchronous=NORMAL`. Schema: `documents` table (PK: id, UNIQUE: path) + `docs_fts` FTS5 virtual table with trigram tokenizer (supports CJK). `recall()` wraps user queries in double-quotes before FTS5.

### Secrets (`src/secrets/store.rs`)

AES-256-GCM encrypted storage in SQLite. **Phase 1 uses a hardcoded dev key** — there is a `compile_error!` guard that blocks release builds until this is replaced with a platform keystore. Passwords are `Zeroize`d on drop.

### Skills (`src/skill/`)

Skill bundles are directory pairs: `skill.yaml` (manifest with keywords) + `skill.md` (prompt content). `SkillManager::match_skills()` does case-insensitive keyword matching on user input and prepends matched skills' content to the system prompt.

## Code Standards (from `docs/dev-standards.md`)

- No `unwrap()` in non-test code without a safety comment explaining why it can't fail
- No `#[allow(...)]` without an explanatory comment
- No `.clone()` on hot paths without justification
- Property-based tests (`proptest`) required for: path validation, URL parsing, all functions that process untrusted string input
- TDD: write failing test first for all new features and bug fixes
- Commit format: `<type>(<scope>): <description>` (e.g., `fix(tools): enforce URL allowlist`)

## Test Infrastructure

- Unit tests: `#[cfg(test)]` modules inside source files
- Integration tests: `tests/` directory; require `--features test-utils` for `MockLlmClient`
- `MockLlmClient`: returns fixed response text; available behind `test-utils` feature gate
- `NullSecretStore` and `PermissionChecker::allow_all()`: test helpers for dependency injection
- Security coverage floors: path/URL/registry files ≥ 90% line coverage

## Phase Status

- **Phase 1 (complete):** Rust core — agent loop, memory, tools, skills, secrets, FFI boundary
- **Phase 2 (in progress, branch `feature/mobileclaw-flutter`):** Flutter plugin + Dart bindings; `MockMobileclawAgent` available for UI work before bindings are complete
- **Phase 3 (planned):** Android/iOS platform keystore integration (see `docs/mobileclaw-phase3-android.md`)
