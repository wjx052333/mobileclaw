# Testing — mobileclaw-core

This document describes the test suite for `mobileclaw-core`: how to run tests, what every test covers, the infrastructure used, and guidance for adding new tests.

---

## 1. How to Run Tests

### Standard unit and integration tests

```bash
cargo test -p mobileclaw-core
```

Runs all unit tests (in `src/**` under `#[cfg(test)]`) and the integration tests in `tests/integration_memory.rs` and `tests/integration_tools.rs`. Does **not** run `integration_agent.rs` because that file requires `MockLlmClient`, which is only compiled under the `test-utils` feature flag.

### Include integration_agent (requires MockLlmClient)

```bash
cargo test -p mobileclaw-core --features test-utils
```

Why `--features test-utils` is needed: `MockLlmClient` is a concrete implementation of `LlmClient` that returns a configurable fixed text stream. It lives in `src/llm/client.rs` behind `#[cfg(feature = "test-utils")]`. Using `#[cfg(test)]` instead would not work for files in `tests/`, because `tests/integration_agent.rs` is a separate compilation unit — it is compiled as an external crate and cannot see items guarded by `#[cfg(test)]` in the library under test. The feature flag is the correct mechanism to expose test helpers to integration tests.

### Run a specific integration test file

```bash
cargo test -p mobileclaw-core --test integration_memory
cargo test -p mobileclaw-core --test integration_tools
cargo test -p mobileclaw-core --features test-utils --test integration_agent
```

### Run with output (do not suppress stdout)

```bash
cargo test -p mobileclaw-core --features test-utils -- --nocapture
```

### Strict Clippy lint check

```bash
cargo clippy -p mobileclaw-core --features test-utils -- -D warnings
```

---

## 2. Test Inventory

### Unit tests (in `src/` files under `#[cfg(test)]`)

| Test Name | Location | Type | What It Tests |
|---|---|---|---|
| `builtin_names_are_protected` | `src/tools/registry.rs` | unit | Registering a builtin then calling `register_extension` with the same name returns `ClawError::ToolNameConflict` |
| `extension_tool_registers_successfully` | `src/tools/registry.rs` | unit | A non-conflicting extension name is accepted and retrievable |
| `get_unknown_tool_returns_none` | `src/tools/registry.rs` | unit | `ToolRegistry::get` returns `None` for an unregistered name |
| `list_tools_returns_all` | `src/tools/registry.rs` | unit | `ToolRegistry::list` returns all registered tools |
| `file_write_and_read_roundtrip` | `src/tools/builtin/file.rs` | unit (async) | `FileWriteTool` then `FileReadTool` round-trips content correctly |
| `path_traversal_is_rejected` | `src/tools/builtin/file.rs` | unit (async) | `../../../etc/passwd` path returns `ClawError::PathTraversal` |
| `no_path_traversal_escapes_sandbox` | `src/tools/builtin/file.rs` | proptest | 256 random path inputs prefixed with `../../` either resolve inside the sandbox or are rejected — never resolve outside |
| `allowed_domain_passes` | `src/tools/builtin/http.rs` | unit | A URL whose host exactly matches the allowlist entry is permitted |
| `disallowed_domain_blocked` | `src/tools/builtin/http.rs` | unit | A URL for an unlisted host is blocked |
| `empty_allowlist_blocks_all` | `src/tools/builtin/http.rs` | unit | Any URL is blocked when the allowlist is empty |
| `url_with_userinfo_is_rejected` | `src/tools/builtin/http.rs` | unit | `https://user:pass@api.github.com/` is blocked even when the host is listed |
| `host_spoofing_is_rejected` | `src/tools/builtin/http.rs` | unit | `https://api.github.com.evil.com/` does not match `https://api.github.com` |
| `http_scheme_blocked_when_allowlist_requires_https` | `src/tools/builtin/http.rs` | unit | Plain `http://` scheme is blocked when the allowlist entry uses `https://` |
| `arbitrary_url_never_panics` | `src/tools/builtin/http.rs` | proptest | 256 arbitrary strings passed to `is_url_allowed` never cause a panic |
| `memory_write_stores_document` | `src/tools/builtin/memory_tools.rs` | unit (async) | `MemoryWriteTool` succeeds and returns the stored path |
| `memory_search_returns_matching_doc` | `src/tools/builtin/memory_tools.rs` | unit (async) | A document written with `MemoryWriteTool` is found by `MemorySearchTool` |
| `memory_search_empty_returns_empty` | `src/tools/builtin/memory_tools.rs` | unit (async) | A search that matches nothing returns an empty results array |
| `memory_write_missing_content_errors` | `src/tools/builtin/memory_tools.rs` | unit (async) | `MemoryWriteTool` with no `content` field returns an error |
| `parse_single_tool_call` | `src/agent/parser.rs` | unit | `extract_tool_calls` parses one `<tool_call>` block correctly, including Chinese preamble text |
| `parse_multiple_tool_calls` | `src/agent/parser.rs` | unit | Two consecutive `<tool_call>` blocks are both extracted |
| `no_tool_calls_returns_empty` | `src/agent/parser.rs` | unit | Plain text with no `<tool_call>` tags returns an empty list |
| `malformed_json_is_skipped` | `src/agent/parser.rs` | unit | A `<tool_call>` block containing non-JSON text is silently skipped |
| `serialize_tool_result_ok` | `src/agent/parser.rs` | unit | `format_tool_result` produces XML with `status="ok"` and the tool name |
| `serialize_tool_result_error` | `src/agent/parser.rs` | unit | `format_tool_result` produces XML with `status="error"` |
| `extract_text_strips_tool_calls` | `src/agent/parser.rs` | unit | `extract_text_without_tool_calls` removes `<tool_call>` blocks and concatenates surrounding text |
| `load_valid_skill` | `src/skill/loader.rs` | unit (async) | A directory with a valid `skill.yaml` and `skill.md` is loaded and parsed |
| `skip_invalid_skill_yaml` | `src/skill/loader.rs` | unit (async) | A directory with malformed YAML is silently skipped; the function still succeeds |
| `empty_dir_returns_empty` | `src/skill/loader.rs` | unit (async) | An empty directory returns an empty `Vec<Skill>` |
| `keyword_match_is_case_insensitive` | `src/skill/manager.rs` | unit | `SkillManager::match_skills` matches regardless of ASCII case, and also matches multi-byte Chinese keywords |
| `build_system_prompt_appends_skill_prompts` | `src/skill/manager.rs` | unit | `build_system_prompt` returns the base prompt followed by the skill prompt section |
| `message_serializes_correctly` | `src/llm/types.rs` | unit | `Message` serializes to JSON with the expected `role` and `content[0].type` fields |
| `stream_event_text_delta` | `src/llm/types.rs` | unit | `StreamEvent::TextDelta` matches the correct variant |

### Integration tests (in `tests/`)

| Test Name | Location | Type | What It Tests |
|---|---|---|---|
| `store_and_get_roundtrip` | `tests/integration_memory.rs` | integration (async) | `SqliteMemory::store` then `get` recovers the stored content and category |
| `full_text_search_finds_document` | `tests/integration_memory.rs` | integration (async) | FTS5 trigram search returns only the document that contains the query substring (Chinese text) |
| `store_overwrites_existing_path` | `tests/integration_memory.rs` | integration (async) | Storing to the same path twice replaces the document; `count()` stays at 1 |
| `forget_removes_document` | `tests/integration_memory.rs` | integration (async) | `forget` returns `true` and the document is no longer accessible via `get` |
| `category_filter_works` | `tests/integration_memory.rs` | integration (async) | A `SearchQuery` with a `category` filter returns only documents of that category |
| `all_builtins_registered_with_unique_names` | `tests/integration_tools.rs` | integration (async) | After calling `register_all_builtins`, every tool name is unique in the registry |
| `time_tool_returns_unix_timestamp` | `tests/integration_tools.rs` | integration (async) | The `time` tool returns a JSON object with a positive `unix_timestamp` |
| `memory_write_then_search` | `tests/integration_tools.rs` | integration (async) | `memory_write` tool stores a document; `memory_search` tool finds it by keyword |
| `extension_cannot_override_builtin` | `tests/integration_tools.rs` | integration (async) | Attempting to register an extension tool whose name collides with a builtin returns `ClawError::ToolNameConflict` |
| `simple_conversation_returns_text` | `tests/integration_agent.rs` | integration (async) | `AgentLoop::chat` with a `MockLlmClient` returns `AgentEvent::TextDelta` events containing the mocked response |
| `tool_call_in_response_is_executed` | `tests/integration_agent.rs` | integration (async) | A mocked LLM response containing a `<tool_call>` block causes the agent to emit at least one `AgentEvent::ToolCall` event |
| `message_history_grows_with_turns` | `tests/integration_agent.rs` | integration (async) | After two calls to `chat`, `AgentLoop::history()` contains four messages (user + assistant per turn) |

**Total: 32 unit tests + 12 integration tests = 44 test functions, plus 2 proptest properties (each exercising 256 random cases by default).**

---

## 3. Test Categories

### Unit Tests

**`src/tools/registry.rs`** — `ToolRegistry` protection logic. Verifies that builtins are added to a protected set at registration time, that calling `register_extension` with a protected name fails with `ToolNameConflict`, that non-conflicting names succeed, that `get` returns `None` for absent names, and that `list` enumerates all registered tools.

**`src/tools/builtin/file.rs`** — Filesystem sandbox. Covers the happy-path round-trip (write then read), the explicit `../../../etc/passwd` traversal case, and a property-based test that runs 256 generated paths to assert that `resolve_sandbox_path` never returns a path outside the sandbox.

**`src/tools/builtin/http.rs`** — HTTP allowlist enforcement. Covers allowed domains, blocked domains, empty allowlist, userinfo in URL, hostname spoofing via suffix, and scheme downgrade. The proptest exercises 256 arbitrary URL-like strings to confirm `is_url_allowed` never panics.

**`src/tools/builtin/memory_tools.rs`** — `MemoryWriteTool` and `MemorySearchTool`. Covers storing a document, searching for it, receiving an empty result when nothing matches, and returning an error when required arguments are absent.

**`src/agent/parser.rs`** — Tool call XML parsing and serialization. Covers single and multiple `<tool_call>` blocks, plain text with no tags, malformed JSON inside a tag, `<tool_result>` serialization for both success and error cases, and text extraction that strips tool call blocks.

**`src/skill/loader.rs`** — Skill YAML loader. Covers loading a valid skill directory (verifying name and prompt content), silently skipping a directory with malformed YAML, and returning an empty list from an empty directory.

**`src/skill/manager.rs`** — Skill keyword matching and system-prompt construction. Covers case-insensitive ASCII matching, Chinese keyword matching, no-match for unrelated input, and verifying that `build_system_prompt` appends skill sections after the base prompt.

**`src/llm/types.rs`** — LLM message and event types. Covers JSON serialization of `Message` and the `StreamEvent::TextDelta` variant pattern.

### Integration Tests

#### `tests/integration_memory.rs` (5 tests)

Each test creates a fresh `SqliteMemory` backed by a temporary file using `TempDir`. The tests exercise the full `Memory` trait against a real SQLite database with FTS5 enabled:

- `store_and_get_roundtrip` — basic persistence.
- `full_text_search_finds_document` — Chinese-language FTS with trigram tokenizer, checking that the correct document is returned and the irrelevant one is not.
- `store_overwrites_existing_path` — upsert semantics confirmed via `count()`.
- `forget_removes_document` — delete and confirm absence.
- `category_filter_works` — `SearchQuery.category` field restricts results to a single category.

#### `tests/integration_tools.rs` (4 tests)

Each test initializes a full `ToolContext` (real `SqliteMemory`, real sandbox directory, `PermissionChecker::allow_all()`) and a `ToolRegistry` populated by `register_all_builtins`. The tests exercise tool registration and execution end-to-end:

- `all_builtins_registered_with_unique_names` — registration invariant.
- `time_tool_returns_unix_timestamp` — live tool execution.
- `memory_write_then_search` — cross-tool data flow through shared memory.
- `extension_cannot_override_builtin` — security invariant at the integration level.

#### `tests/integration_agent.rs` (3 tests, requires `--features test-utils`)

These tests exercise the complete `AgentLoop` using `MockLlmClient`. The mock returns a configurable static string as the LLM response stream, so no network call is made:

- `simple_conversation_returns_text` — verifies `AgentEvent::TextDelta` events are emitted and contain the expected text.
- `tool_call_in_response_is_executed` — verifies that a `<tool_call>` embedded in the mock response causes the agent to dispatch the tool and emit `AgentEvent::ToolCall`.
- `message_history_grows_with_turns` — verifies that multi-turn conversation correctly accumulates message history.

---

## 4. Security-Focused Tests

The following tests are specifically designed to verify security boundaries. They must continue to pass whenever the affected modules are modified.

### Path traversal sandbox

| Test | Mechanism | What it proves |
|---|---|---|
| `path_traversal_is_rejected` | Unit, hand-crafted input `../../../etc/passwd` | The canonical traversal attack string is rejected with `ClawError::PathTraversal` before any filesystem access |
| `no_path_traversal_escapes_sandbox` | Proptest, 256 random paths prepended with `../../` | For every possible path segment combination generated by proptest, either the result is inside the sandbox or an error is returned — a resolved path outside the sandbox is never returned |

The sandbox resolution logic in `resolve_sandbox_path` (in `src/tools/builtin/file.rs`) performs manual component-by-component normalization rather than relying on filesystem `canonicalize`, so it works even when the target path does not exist.

### HTTP allowlist bypass

| Test | Mechanism | What it proves |
|---|---|---|
| `host_spoofing_is_rejected` | Unit, input `https://api.github.com.evil.com/` against allowlist `https://api.github.com` | Exact host comparison in `is_url_allowed` prevents a suffix-appended hostname from being mistaken for the allowed host |
| `url_with_userinfo_is_rejected` | Unit, input `https://user:pass@api.github.com/` | Userinfo in the URL authority is detected and rejected before host comparison, preventing credential injection |
| `http_scheme_blocked_when_allowlist_requires_https` | Unit, input `http://` scheme | Only `https` scheme is permitted; plain HTTP is always blocked |
| `arbitrary_url_never_panics` | Proptest, 256 random strings drawn from URL-legal characters | `is_url_allowed` handles any syntactically plausible input without panicking — all errors return `false` |

The `is_url_allowed` function uses the `url` crate for structured parsing so that host, scheme, and path are compared as typed fields, not as raw string prefixes.

### Tool name hijacking

| Test | Mechanism | What it proves |
|---|---|---|
| `builtin_names_are_protected` | Unit in `src/tools/registry.rs` | A name registered via `register_builtin` is added to a protected set; calling `register_extension` with that name returns `ClawError::ToolNameConflict` |
| `extension_cannot_override_builtin` | Integration in `tests/integration_tools.rs` | The same protection holds after a full `register_all_builtins` call — an adversarial extension named `file_read` cannot replace the real tool |

---

## 5. Test Infrastructure

### `tempfile::TempDir`

Used in every test that touches the filesystem or SQLite. `TempDir::new()` creates an isolated temporary directory in the system's temp location. The directory and all contents are deleted automatically when the `TempDir` value is dropped (at the end of each test). This ensures tests never share state through the filesystem.

### `SqliteMemory::open(path)`

Tests use a real SQLite database opened in the temporary directory, not an in-memory database or a mock. This ensures that the FTS5 trigram tokenizer, the trigger-maintained virtual table, and the WAL mode settings all behave exactly as they do in production. There is no memory-backend variant specifically for tests.

### `MockLlmClient`

`MockLlmClient` is defined in `src/llm/client.rs` behind `#[cfg(feature = "test-utils")]`. It implements the `LlmClient` trait and returns a fixed `response: String` as a single-event stream. Because it is compiled as part of the library (not under `#[cfg(test)]`), it is accessible from `tests/integration_agent.rs` when the crate is built with `--features test-utils`.

Why `#[cfg(test)]` cannot be used here: items guarded by `#[cfg(test)]` are only visible within the same crate's test compilation. Files under `tests/` are separate crates that link against the library in its non-test compilation — they cannot see `#[cfg(test)]` items. The `test-utils` feature is therefore the correct and only mechanism.

### `PermissionChecker::allow_all()`

Tests construct a `ToolContext` with `permissions: Arc::new(PermissionChecker::allow_all())`. This bypasses permission checks so that tests can exercise tool behavior without needing to configure specific permission grants. Production code must configure permissions explicitly.

### `proptest` configuration

Proptest uses its default configuration, which runs 256 random cases per property. The `proptest!` macro integrates with the standard `#[test]` harness so property tests run with `cargo test` like ordinary tests.

---

## 6. TDD Approach

The MVP (Tasks 1–15) was developed using a strict red-green-refactor cycle:

1. **Red** — write a test for the new behavior before any implementation. At this stage the test either fails to compile or fails at runtime.
2. **Green** — write the minimal implementation required to make the test pass.
3. **Refactor** — clean up the implementation and review for security and correctness, keeping tests passing throughout.

Each task was committed independently with its tests, so the commit history tracks which tests were introduced alongside each feature. Security-critical modules (`file.rs`, `http.rs`, `registry.rs`) received property-based tests in addition to unit tests as part of the same task commit.

---

## 7. Adding New Tests

### Unit tests

Add a `#[cfg(test)]` module at the bottom of the relevant `src/` file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]          // for synchronous tests
    fn my_test() { ... }

    #[tokio::test]   // for async tests
    async fn my_async_test() { ... }
}
```

Use `tempfile::TempDir` for any test that requires filesystem or SQLite access.

### Integration tests

Add an `async fn` with `#[tokio::test]` to the appropriate file in `tests/`:

- `tests/integration_memory.rs` — tests for `Memory` trait implementations.
- `tests/integration_tools.rs` — tests for tool execution and registration.
- `tests/integration_agent.rs` — tests for `AgentLoop` (requires `--features test-utils`).

Follow the existing helper pattern (`make_memory()`, `make_ctx()`, `make_loop()`) to keep boilerplate minimal.

### Property-based tests for security-critical code

Any code that validates or sanitizes user-controlled input must have a proptest in addition to hand-crafted cases. Use the `proptest!` macro:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn invariant_holds_for_all_inputs(input in r"<regex_pattern>") {
        // assert invariant
    }
}
```

Place proptests in the same `#[cfg(test)]` module as the related unit tests.

### Before committing

Always run the full test suite including the agent integration tests:

```bash
cargo test -p mobileclaw-core --features test-utils
```

Also run Clippy to catch warnings before they accumulate:

```bash
cargo clippy -p mobileclaw-core --features test-utils -- -D warnings
```
