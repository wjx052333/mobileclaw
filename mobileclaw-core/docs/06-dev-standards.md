# 06 — Developer Standards

This document defines coding and architectural standards for `mobileclaw-core`. All contributors must adhere to these guidelines.

---

## 1. Code Quality

### 1.1 Rust Idioms

- Use `Result<T, ClawError>` for all fallible operations; never use `.unwrap()` in library code except in tests or startup (where panicking is acceptable).
- Prefer `?` operator over manual error handling.
- Use `#[derive(thiserror::Error)]` for all error types.
- All public types should have rustdoc comments with examples.

### 1.2 Testing

- All public functions must have unit tests (in `#[cfg(test)]` modules within the same file).
- Integration tests live in `tests/` and test subsystem behaviour end-to-end.
- Use `proptest` for property-based testing of boundary conditions (e.g. path traversal, URL parsing).
- No test should rely on external resources (network, real files). Mock all I/O.

### 1.3 Clippy

All code must pass:

```bash
cargo clippy -p mobileclaw-core --features test-utils -- -D warnings
```

Warnings are treated as errors. If a lint is a false positive, document the exception with `#[allow(...)]` and a comment explaining why.

---

## 2. Security

### 2.1 Path and URL Sandboxing

- All file operations must go through `ToolContext::sandbox_dir()` and be checked by `PermissionChecker::check_path_traversal()`.
- All HTTP requests must be checked by `PermissionChecker::check_url_allowed()` before attempting the fetch.
- Never trust user input; always validate and reject rather than auto-correct.

### 2.2 Error Messages

- Error messages must not leak sensitive information (API keys, file contents, passwords, or internal paths).
- Use `thiserror::Error` to ensure error messages are consistent and scrubbed of secrets.
- When building error strings that include user input, ensure the input is truncated and sanitized.

### 2.3 Logging and Tracing

- Use `tracing::*!` macros (not `println!` or `eprintln!`).
- Never log personally identifiable information, credentials, or large blobs.
- Fields logged in spans must be stripped of sensitive data. Use log redaction where needed.

---

## 3. Data Handling

### 3.1 Ownership and Lifetimes

- Prefer owned types (`String`, `Vec<T>`) over borrowed types in public APIs to avoid lifetime complexity.
- Opaque Rust types exposed via FFI must be wrapped in `Arc<Mutex<T>>` or similar.
- All `Arc` handles must be documented with their thread-safety guarantees.

### 3.2 Serialization

- All types sent to Dart via FFI must be serializable to JSON (implement `serde::Serialize`).
- DTOs should be separate types (e.g. `SkillManifestDto`) to decouple FFI contracts from internal structs.
- Do not expose opaque Rust types as JSON fields; use token handles (e.g. `session_id: String`).

### 3.3 Memory Safety

- All multi-threaded code must use standard library synchronisation primitives (`Mutex`, `RwLock`, `Arc`) with careful attention to deadlock risk.
- Use `clippy::all` and `clippy::pedantic` to catch subtle bugs.
- Use `miri` on a nightly toolchain to detect undefined behaviour in tests.

### 3.4 Configuration

- All user-configurable settings must be grouped in a single `AgentConfig` struct with sane defaults.
- Configuration must be immutable after `AgentSession::create()` (the agent loop does not support runtime reconfiguration).
- Hard-coded defaults are acceptable; runtime configuration via environment variables is not (it complicates testing and deployment).

---

### 3.5 密钥存储安全 (Secret Store Security)

- All credentials (email passwords, API keys) must be stored exclusively via `SecretStore::put()` — never in `ToolContext`, `AgentConfig`, logs, or memory without `SecretString`
- `SecretString` must never be passed to `tracing::*!` macros, formatted into error messages, or serialized to JSON
- The AES-256-GCM key passed to `SqliteSecretStore::open()` must originate from the platform keystore (Android Keystore / iOS Keychain) in production builds. The placeholder key in `ffi.rs` must be replaced before Phase 2 release
- `FFI`: no `get_password` or equivalent method may ever be added to the Flutter API surface. If the user needs to change a password, they call `email_account_save` again with the new password

---

## 4. Testing Checklist

Before submitting a pull request:

1. All unit tests pass: `cargo test -p mobileclaw-core --features test-utils`
2. All integration tests pass: `cargo test -p mobileclaw-core --features test-utils --test '*'`
3. Clippy passes with no warnings: `cargo clippy -p mobileclaw-core --features test-utils -- -D warnings`
4. No `unsafe` blocks unless absolutely necessary (and documented with a SAFETY comment)
5. All public APIs have doctests or examples

---

## 5. Documentation

- Update `05-flutter-interface.md` whenever the Dart API contract changes.
- Update `03-tool-design.md` when adding or modifying tools.
- Keep `01-security-model.md` in sync with threat model changes.
- Use markdown for all docs and check spelling with a standard English dictionary.

---

## 6. Git Hygiene

- One logical change per commit.
- Commit messages use imperative mood: "add feature X", "fix bug Y", not "added", "fixed".
- Tag security-critical commits with `[SECURITY]` in the message.
- Never commit credentials, keys, or `.env` files.

---
