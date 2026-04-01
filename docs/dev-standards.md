# mobileclaw Development Standards

> **Mission:** Build a mobile AI agent engine where extreme performance and security are the lifeline of every line of code, enforced by tests that can never be skipped.

---

## 1. 核心原则 (Core Principles)

Three principles are non-negotiable across all phases of mobileclaw. They are not guidelines — they are hard constraints that block merge if violated.

### 1.1 极致性能 (Extreme Performance)

Mobile devices have constrained CPUs, limited RAM, and no tolerance for perceptible latency. A blocked UI thread, an unnecessary heap allocation on a hot path, or a synchronous disk read in async context will degrade user experience in ways that cannot be patched later.

Targets:
- Agent turn-around latency (first token): < 200 ms on mid-range hardware
- Memory search (`FTS5`): < 10 ms for 10 k documents
- Tool execution overhead (excluding I/O): < 5 ms

Every performance rule in Section 2 exists to keep these targets achievable as the codebase grows.

### 1.2 安全即设计 (Security by Design)

Security is not added after a feature is complete. It is designed in from the first test. The three security lifelines (path traversal, URL allowlist, tool name protection) protect the user's device from malicious agent instructions. Any bypass — intentional or accidental — is a critical defect.

Rules:
- No feature that touches the filesystem, network, or tool registry ships without the corresponding security check wired in
- Security tests are written before or alongside the implementation (TDD, see Section 4.4)
- A failing security test always blocks merge, no exceptions

### 1.3 测试即质量门 (Tests as Quality Gate)

Coverage floors are enforced by CI. A PR that drops coverage below the floor does not merge. This is not a suggestion.

The test suite is the proof of correctness. Code that is not tested is code that does not work.

---

## 2. 性能规范 (Performance Standards)

### 2.1 设计规则 (Design Rules)

**Heap allocation:**
- Avoid heap allocation on hot paths; prefer stack allocation
- Use `&str` over `String` wherever the lifetime permits
- Zero-copy XML parsing in `agent/parser.rs`: use `find()` + slice; never call `String::new()` inside a parsing loop

**SQLite:**
- Always enable WAL mode and MMAP at connection time
- Never run DDL or schema changes on a hot path; schema migrations happen at startup only

**Shared state:**
- Use `Arc<T>` for shared read-only state
- Use `Mutex<T>` only when write access is required
- Never use `RwLock<T>` unless reads outweigh writes by at least 100:1 (document the ratio in a comment)

**Async correctness:**
- `async fn` must not block the executor
- Never call `std::thread::sleep` inside async code
- Never call `std::fs` inside async context — use `tokio::fs` instead

**Streaming:**
- LLM responses are delivered via SSE and must be processed as a stream
- Never buffer an entire LLM response before processing; this defeats streaming and spikes memory

**Cloning:**
- Avoid unnecessary `.clone()` calls
- Every `.clone()` that cannot be eliminated by extending a lifetime must carry a comment explaining why

### 2.2 基准测试 (Benchmarking)

- Every new performance-critical path must ship with a benchmark under `benches/`
- A regression greater than 10% on any tracked benchmark blocks merge
- Benchmark names follow `<module>_<operation>` (e.g., `memory_fts5_search`, `parser_xml_round_trip`)

### 2.3 性能分析工具 (Profiling Tools)

| Tool | Purpose |
|------|---------|
| `cargo flamegraph` | CPU hot-spot identification |
| `perf` | System-level profiling on Linux |
| `tokio-console` | Async task visibility; detecting executor stalls |

---

## 3. 安全规范 (Security Standards)

The three lifelines below are inviolable. Never bypass them. Never weaken them. Never add a `skip_security: bool` parameter.

### 3.1 路径穿越防护 (Path Traversal Protection)

- All file paths that originate from external input (agent instructions, user input, LLM responses) **must** pass through `resolve_sandbox_path()` before any filesystem operation
- `resolve_sandbox_path` uses manual component-by-component normalization; it does **not** call `std::fs::canonicalize` because the file may not exist yet
- Patterns that must be rejected unconditionally:
  - `../` in any position
  - Absolute paths (e.g., `/etc/passwd`, `/proc/self`)
  - Null bytes (`\0`) anywhere in the path
  - Encoded variants (`%2e%2e`, `..%2f`, etc.)

**Property-based test requirement:** The file path validation module must have a proptest suite with a minimum of **256 cases** covering random segment counts, depth combinations, and unicode characters. This test must live in the same file as the implementation.

### 3.2 URL 白名单 (URL Allowlist)

- All outbound HTTP requests **must** check `is_url_allowed()` before the request is dispatched
- URL parsing uses the `url` crate for structural analysis — **never** string prefix matching
- Host comparison must be **exact** (`host == allowed_host`), never `starts_with`, to prevent `allowed.com.evil.com` bypass
- Scheme must be `https` exclusively; HTTP is rejected
- URLs containing userinfo (`user:pass@host`) are rejected
- Query strings and fragments are allowed but must not influence host/scheme validation

**Property-based test requirement:** `arbitrary_url_never_panics` proptest covering a minimum of **256 random URL strings** must always exist and pass. It tests that `is_url_allowed()` never panics and always returns a definite `bool`.

### 3.3 工具名保护 (Tool Name Protection)

- Built-in tool names are registered via `ToolRegistry::register_builtin` at initialization time; they are permanently protected
- Extension tools **must** use `ToolRegistry::register_extension`, which enforces name uniqueness and rejects any name that conflicts with a built-in
- The integration test `extension_cannot_override_builtin` must always exist and must always pass

### 3.4 附加安全规则 (Additional Security Rules)

- FTS5 user queries must be wrapped in double-quotes before being passed to SQLite to prevent FTS5 injection
- `unsafe` code is forbidden without an explicit security review. Any `unsafe` block must carry a comment that (a) names the reviewer, (b) states the invariant that makes it safe, and (c) links to the review record
- Before upgrading any dependency, run `cargo audit` to check for known advisories
- API keys and secrets must never be logged. Use `tracing::debug!` only for sanitized context fields; never log raw secret values

---

## 4. 测试规范 (Testing Standards)

### 4.1 覆盖率要求 (Coverage Requirements)

Coverage floors are **hard** — CI fails if they are not met.

| Scope | Line Coverage | Function Coverage |
|-------|-------------|-----------------|
| Overall crate | ≥ 85% | ≥ 75% |
| Security-critical files (`file.rs`, `http.rs`, `registry.rs`) | ≥ 90% | ≥ 85% |
| Error handling paths | ≥ 80% | ≥ 80% |

**How to measure:**

```bash
cargo llvm-cov --package mobileclaw-core --features test-utils --all-targets
```

**How to enforce in CI:**

```bash
cargo llvm-cov --package mobileclaw-core --features test-utils --all-targets --fail-under-lines 85
```

### 4.2 测试类型要求 (Test Type Requirements)

Every new feature must include all three of the following:

**1. Unit tests** (in `#[cfg(test)]` module in the same file):
- Happy path — proves the feature works
- Error and edge cases — proves failures are handled
- For security-sensitive functions: proptest in addition to hand-written cases

**2. Integration tests** (in `tests/*.rs`):
- End-to-end behavior exercised through the public API
- Any behavior that crosses module boundaries

**3. Property-based tests** (proptest) — **required** for:
- Any function that accepts untrusted string input (paths, URLs, search queries)
- Any parsing function
- Any security boundary function

### 4.3 测试设计原则 (Test Design Principles)

**Tests must be deterministic:**
- No `sleep()` in tests
- No dependency on system time (if unavoidable, document with `#[allow]` and an explanation)
- No network calls in unit tests; use `MockLlmClient` for LLM interactions and mock HTTP for network

**Tests must be isolated:**
- Always use `tempfile::TempDir` for filesystem tests — never hardcode paths
- Each test gets its own SQLite database instance
- No shared mutable state between tests

**Tests must be meaningful:**
- Do not trivially test getters and setters
- Test observable behavior, not internal implementation
- Assertion failure messages must be informative: `assert!(cond, "meaningful message describing what failed and why")`

**Test naming convention:** `<subject>_<condition>_<expected>`

Examples:
- `path_traversal_is_rejected`
- `memory_search_returns_matching_doc`
- `url_http_scheme_is_rejected`
- `tool_extension_cannot_override_builtin`

**Coverage strategy order:**
1. Happy path — proves the feature works at all
2. Error paths — proves failures are handled gracefully
3. Boundary conditions — empty input, maximum length, unicode
4. Adversarial input — proptest + hand-crafted attack vectors (required for security code)

### 4.4 TDD 工作流 (TDD Workflow)

All new features must follow the Red-Green-Refactor cycle:

```
1. Write failing test   → cargo test: FAIL (compile error or assertion failure)   ← "Red"
2. Write minimal impl   → cargo test: PASS                                         ← "Green"
3. Refactor if needed   → cargo test: still PASS                                   ← "Refactor"
4. Commit               → git commit -m "feat(...): ..."
```

**PR checklist — every PR must satisfy all items:**

- [ ] Tests written before or alongside implementation (TDD)
- [ ] Coverage does not drop below floor (`cargo llvm-cov --fail-under-lines 85`)
- [ ] `cargo clippy -- -D warnings` passes with zero warnings
- [ ] `cargo test --features test-utils` — all tests pass
- [ ] No new `#[allow(dead_code)]` without a comment explaining why
- [ ] No `unwrap()` in non-test code without a comment explaining why the invariant holds (use `?` or explicit error handling instead)
- [ ] Security-sensitive changes include a proptest

---

## 5. 代码风格规范 (Code Style Standards)

### 5.1 Rust 规则 (Rust Rules)

**Error propagation:**
- Always use `?` for propagation in production code paths
- `unwrap()` is forbidden in non-test code without a comment explaining the invariant that guarantees it cannot panic
- Never `expect()` in production code unless the message is actionable and the invariant is documented

**Annotations:**
- `#[allow(...)]` annotations must include a comment with the reason; bare `#[allow]` is rejected in code review

**Module design:**
- `mod.rs` re-exports only — no logic in `mod.rs`
- One primary type per file (e.g., `sqlite.rs` owns `SqliteMemory`)
- `traits.rs` for trait definitions; implementations live in separate files

**Async:**
- Prefer `async fn` over manual `Future` boxing
- `Box<dyn Future>` only where trait objects require it

**Logging with `tracing`:**

| Level | When to use |
|-------|------------|
| `error!` | Unrecoverable error; the operation cannot continue |
| `warn!` | Unexpected condition that was handled; operation continues |
| `info!` | Normal operational events (startup, shutdown, significant state changes) |
| `debug!` | Developer detail; never include sensitive data |

Never log API keys, file contents from user data, or any credential.

### 5.2 命名规范 (Naming Conventions)

| Category | Convention | Example |
|----------|-----------|---------|
| Types | `UpperCamelCase` | `SqliteMemory`, `ToolRegistry` |
| Functions / methods | `snake_case` | `resolve_sandbox_path`, `is_url_allowed` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_SEARCH_RESULTS`, `SANDBOX_ROOT` |
| Test functions | `snake_case` with `<subject>_<condition>_<expected>` | `path_traversal_is_rejected` |

---

## 6. Git 规范 (Git Standards)

### 6.1 提交消息格式 (Commit Message Format)

```
<type>(<scope>): <short description>

[optional body explaining why, not what]
```

**Types:**

| Type | Use for |
|------|---------|
| `feat` | New feature |
| `fix` | Bug fix |
| `test` | Test additions or corrections |
| `docs` | Documentation only |
| `refactor` | Code restructuring without behavior change |
| `chore` | Tooling, dependencies, CI |
| `perf` | Performance improvement |
| `security` | Security fix or hardening |

**Scopes:** `agent`, `memory`, `tools`, `skill`, `llm`, `ffi`, `docs`

**Examples:**

```
feat(tools): add FileReadTool with sandbox enforcement and proptest
fix(memory): escape FTS5 query to prevent injection
test(tools): expand system.rs coverage from 12% to 82%
security(tools): fix host-exact comparison in URL allowlist
perf(memory): switch FTS5 ranking to BM25 to reduce query latency
```

### 6.2 分支策略 (Branch Strategy)

- `main` — always green: all tests pass, all coverage floors met, no failing CI
- Feature branches: `feat/<scope>-<description>` (e.g., `feat/tools-file-read`)
- Fix branches: `fix/<scope>-<description>` (e.g., `fix/memory-fts5-injection`)
- No direct commits to `main`

### 6.3 PR 要求 (PR Requirements)

- All CI checks must pass before merge
- At least one reviewer approval required
- Coverage floors verified by CI (`--fail-under-lines 85`)
- PR description must include: what changed, why, and test strategy

---

## 7. 依赖管理 (Dependency Management)

- Run `cargo audit` before every release and after adding any new dependency
- All dependencies must use `workspace = true` (single version source of truth in `Cargo.toml` at workspace root)
- Every new dependency added in a PR must be justified in the PR description:
  - Why this dependency is needed
  - What the alternative would be (and why it was rejected)
- Avoid any dependency with an open security advisory
- Use `rustls-tls` everywhere; never `native-tls` (consistent, auditable TLS implementation)
- For `reqwest`: never `default-features = true`; explicitly list only the features that are actually used

---

## 8. Flutter 侧规范 (Flutter Standards — Phase 2)

When the Flutter binding layer is added in Phase 2:

- All types that cross the Rust-Dart boundary must have Dart `operator ==` and `hashCode` implemented
- `MobileclawAgent` is **not** safe to share across Dart isolates (Rust `&mut self` ownership semantics); document this on the class
- Use `MockMobileclawAgent` (see `docs/design/05-flutter-interface.md`) for UI development before the FFI layer is ready
- Every Dart API method must carry a `// throws ClawException` comment listing the error conditions
- Flutter integration tests must cover the full agent lifecycle: create agent, send chat, execute tool, handle error

---

## 9. 持续集成要求 (CI Requirements — Roadmap)

Add the following steps to the CI pipeline (GitHub Actions or equivalent). All steps are required; none may be skipped.

```yaml
steps:
  - name: Test (all features)
    run: cargo test -p mobileclaw-core --features test-utils

  - name: Coverage gate
    run: cargo llvm-cov --package mobileclaw-core --features test-utils --all-targets --fail-under-lines 85

  - name: Clippy (strict)
    run: cargo clippy -p mobileclaw-core --features test-utils -- -D warnings

  - name: Security audit
    run: cargo audit
```

CI is the final enforcer of all standards in this document. A green CI run is a necessary condition for merge — it is not sufficient on its own. Code review remains required.
