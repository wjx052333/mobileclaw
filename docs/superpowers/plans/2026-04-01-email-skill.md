# Email Skill Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add built-in email send/receive tools with encrypted credential storage, and a Flutter-facing FFI API for account configuration.

**Architecture:** Credentials (SMTP/IMAP host, port, username, password) are encrypted with AES-256-GCM and stored in a dedicated SQLite table via a new `SecretStore` trait. The raw password is held in a `SecretString` that zeroes memory on drop and is never logged. Two new built-in tools (`email_send`, `email_fetch`) pull credentials from `SecretStore` at call time. Flutter configures accounts through three new FFI methods (`email_account_save`, `email_account_load`, `email_account_delete`) — Dart passes plaintext once during setup; thereafter only the account ID is needed.

**Tech Stack:**
- `lettre 0.11` — SMTP client (async, TLS via rustls)
- `async-imap 0.9` + `tokio-rustls` — IMAP client
- `aes-gcm 0.10` — AES-256-GCM authenticated encryption
- `zeroize 1` — zero secret bytes on drop
- `base64 0.22` — encode ciphertext for SQLite TEXT storage
- Existing: `rusqlite` (bundled), `thiserror`, `tokio`

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/secrets/mod.rs` | Create | Re-export `SecretStore`, `SecretString`, `EmailAccount` |
| `src/secrets/types.rs` | Create | `EmailAccount` config DTO; `SecretString` (zeroize wrapper) |
| `src/secrets/store.rs` | Create | `SecretStore` trait + `SqliteSecretStore` AES-GCM impl |
| `src/tools/builtin/email.rs` | Create | `EmailSendTool`, `EmailFetchTool` |
| `src/tools/builtin/mod.rs` | Modify | Add `pub mod email`; register two new tools |
| `src/tools/traits.rs` | Modify | Add `secrets: Arc<dyn SecretStore>` field to `ToolContext` |
| `src/tools/permission.rs` | Modify | Add `EmailSend`, `EmailReceive` variants |
| `src/error.rs` | Modify | Add `SecretStore(String)`, `Email(String)` error variants |
| `src/lib.rs` | Modify | Add `pub mod secrets` |
| `src/ffi.rs` | Modify | Add `EmailAccountDto`; add `email_account_save/load/delete` FFI methods on `AgentSession` |
| `Cargo.toml` (workspace) | Modify | Add `lettre`, `async-imap`, `tokio-rustls`, `aes-gcm`, `zeroize`, `base64` |
| `mobileclaw-core/Cargo.toml` | Modify | Wire new workspace deps |
| `mobileclaw-core/docs/05-flutter-interface.md` | Modify | Document `EmailAccountDto` and three new FFI methods |
| `mobileclaw-core/docs/06-dev-standards.md` | Modify | Add `EmailSend`/`EmailReceive` to security lifelines |
| `tests/integration_email.rs` | Create | Integration tests for tools + secret store round-trip |

---

## Task 1: SecretString and EmailAccount types

**Files:**
- Create: `mobileclaw-core/src/secrets/types.rs`
- Create: `mobileclaw-core/src/secrets/mod.rs`

- [ ] **Step 1: Write failing test**

```rust
// mobileclaw-core/src/secrets/types.rs  (bottom, #[cfg(test)] mod tests)
#[test]
fn secret_string_zeroes_on_drop() {
    let ptr;
    let len;
    {
        let s = SecretString::new("hunter2".into());
        ptr = s.0.as_ptr();
        len = s.0.len();
        // while alive, exposes value
        assert_eq!(s.expose(), "hunter2");
    }
    // After drop: bytes at ptr should be 0x00
    // Safety: memory is still accessible in the same stack frame; UB in real code
    // but acceptable in a test that runs synchronously.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    assert!(slice.iter().all(|&b| b == 0));
}

#[test]
fn email_account_roundtrip_serialization() {
    let acc = EmailAccount {
        id: "work".into(),
        smtp_host: "smtp.example.com".into(),
        smtp_port: 587,
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        username: "alice@example.com".into(),
    };
    let json = serde_json::to_string(&acc).unwrap();
    let back: EmailAccount = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "work");
    assert_eq!(back.smtp_port, 587);
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p mobileclaw-core 2>&1 | grep "error\[E"
```
Expected: compile error — `SecretString` and `EmailAccount` not found.

- [ ] **Step 3: Implement types**

Create `mobileclaw-core/src/secrets/types.rs`:

```rust
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// A heap string that zeroes its bytes on drop.
/// Never implement `Debug`, `Display`, or `Clone` — callers must use `expose()`.
pub struct SecretString(pub(crate) String);

impl SecretString {
    pub fn new(s: String) -> Self {
        Self(s)
    }
    /// Return the secret value. Call site must not log the return value.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Non-secret configuration for one email account.
/// Password is stored separately in SecretStore under the key `email:<id>:password`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAccount {
    /// Stable identifier chosen by the user (e.g., "work", "personal").
    pub id: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub imap_host: String,
    pub imap_port: u16,
    pub username: String,
}
```

Create `mobileclaw-core/src/secrets/mod.rs`:

```rust
pub mod store;
pub mod types;

pub use store::SecretStore;
pub use types::{EmailAccount, SecretString};
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p mobileclaw-core --features test-utils 2>&1 | grep -E "secret_string|email_account"
```
Expected: `test secret_string_zeroes_on_drop ... ok`, `test email_account_roundtrip_serialization ... ok`

- [ ] **Step 5: Commit**

```bash
git add mobileclaw-core/src/secrets/
git commit -m "feat(secrets): add SecretString (zeroize) and EmailAccount types"
```

---

## Task 2: SecretStore trait and SqliteSecretStore

**Files:**
- Create: `mobileclaw-core/src/secrets/store.rs`
- Modify: `Cargo.toml` (workspace) — add `aes-gcm`, `zeroize`, `base64`
- Modify: `mobileclaw-core/Cargo.toml` — wire new workspace deps

- [ ] **Step 1: Add dependencies**

In `Cargo.toml` (workspace `[workspace.dependencies]`):
```toml
aes-gcm  = "0.10"
zeroize  = { version = "1", features = ["derive"] }
base64   = "0.22"
```

In `mobileclaw-core/Cargo.toml` `[dependencies]`:
```toml
aes-gcm  = { workspace = true }
zeroize  = { workspace = true }
base64   = { workspace = true }
```

Verify compile:
```bash
cargo check -p mobileclaw-core 2>&1 | grep "^error"
```
Expected: no errors.

- [ ] **Step 2: Write failing tests**

```rust
// mobileclaw-core/src/secrets/store.rs  (bottom, #[cfg(test)] mod tests)
use tempfile::TempDir;

async fn open_store(dir: &TempDir) -> SqliteSecretStore {
    SqliteSecretStore::open(
        dir.path().join("secrets.db"),
        b"0123456789abcdef0123456789abcdef", // 32-byte test key
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn store_and_retrieve_secret() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir).await;
    store.put("mykey", "s3cr3t").await.unwrap();
    let val = store.get("mykey").await.unwrap().unwrap();
    assert_eq!(val.expose(), "s3cr3t");
}

#[tokio::test]
async fn get_missing_returns_none() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir).await;
    assert!(store.get("nokey").await.unwrap().is_none());
}

#[tokio::test]
async fn delete_removes_secret() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir).await;
    store.put("k", "v").await.unwrap();
    store.delete("k").await.unwrap();
    assert!(store.get("k").await.unwrap().is_none());
}

#[tokio::test]
async fn ciphertext_in_db_is_not_plaintext() {
    // Open a second raw connection and check the raw stored value
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir).await;
    store.put("pw", "my_password").await.unwrap();

    let conn = rusqlite::Connection::open(dir.path().join("secrets.db")).unwrap();
    let raw: String = conn
        .query_row("SELECT value FROM secrets WHERE key = 'pw'", [], |r| r.get(0))
        .unwrap();
    // The raw value must NOT be the plaintext
    assert!(!raw.contains("my_password"));
}

#[tokio::test]
async fn wrong_key_fails_to_decrypt() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir).await;
    store.put("pw", "secret").await.unwrap();

    // Open with a different key
    let store2 = SqliteSecretStore::open(
        dir.path().join("secrets.db"),
        b"ffffffffffffffffffffffffffffffff",
    )
    .await
    .unwrap();
    let result = store2.get("pw").await;
    assert!(result.is_err());
}
```

- [ ] **Step 3: Run to verify failure**

```bash
cargo test -p mobileclaw-core --features test-utils -- store_and_retrieve_secret 2>&1 | tail -5
```
Expected: compile error — `SqliteSecretStore` not found.

- [ ] **Step 4: Implement SecretStore trait and SqliteSecretStore**

Create `mobileclaw-core/src/secrets/store.rs`:

```rust
use std::path::PathBuf;

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit, OsRng, rand_core::RngCore},
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::{ClawError, ClawResult, secrets::types::SecretString};

/// Trait for opaque secret storage. Implementors must never expose values in logs.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Store a secret under `key`. Overwrites any existing value.
    async fn put(&self, key: &str, value: &str) -> ClawResult<()>;
    /// Retrieve a secret. Returns `None` if the key does not exist.
    async fn get(&self, key: &str) -> ClawResult<Option<SecretString>>;
    /// Delete a secret. No-op if key does not exist.
    async fn delete(&self, key: &str) -> ClawResult<()>;
}

/// SQLite-backed secret store. Values are encrypted with AES-256-GCM.
///
/// Schema: one table `secrets(key TEXT PRIMARY KEY, value TEXT NOT NULL)`.
/// Stored value format: base64(nonce || ciphertext), where nonce is 12 random bytes.
///
/// The 32-byte encryption key must be derived from a device-specific secret
/// (e.g., Android Keystore-protected key, iOS Keychain-protected key) by the caller.
/// This struct does not manage key derivation.
pub struct SqliteSecretStore {
    conn: Mutex<Connection>,
    cipher: Aes256Gcm,
}

impl SqliteSecretStore {
    pub async fn open(path: PathBuf, key_bytes: &[u8; 32]) -> ClawResult<Self> {
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS secrets (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );",
        )?;
        let key = Key::<Aes256Gcm>::from_slice(key_bytes);
        let cipher = Aes256Gcm::new(key);
        Ok(Self { conn: Mutex::new(conn), cipher })
    }

    fn encrypt(&self, plaintext: &str) -> ClawResult<String> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self.cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| ClawError::SecretStore(e.to_string()))?;
        let mut combined = nonce_bytes.to_vec();
        combined.extend_from_slice(&ciphertext);
        Ok(B64.encode(&combined))
    }

    fn decrypt(&self, encoded: &str) -> ClawResult<SecretString> {
        let combined = B64
            .decode(encoded)
            .map_err(|e| ClawError::SecretStore(e.to_string()))?;
        if combined.len() < 12 {
            return Err(ClawError::SecretStore("ciphertext too short".into()));
        }
        let (nonce_bytes, ciphertext) = combined.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let plaintext = self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| ClawError::SecretStore("decryption failed (wrong key?)".into()))?;
        let s = String::from_utf8(plaintext)
            .map_err(|e| ClawError::SecretStore(e.to_string()))?;
        Ok(SecretString::new(s))
    }
}

#[async_trait]
impl SecretStore for SqliteSecretStore {
    async fn put(&self, key: &str, value: &str) -> ClawResult<()> {
        let encoded = self.encrypt(value)?;
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO secrets(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, encoded],
        )?;
        Ok(())
    }

    async fn get(&self, key: &str) -> ClawResult<Option<SecretString>> {
        let conn = self.conn.lock().await;
        let result: Option<String> = conn
            .query_row(
                "SELECT value FROM secrets WHERE key = ?1",
                rusqlite::params![key],
                |r| r.get(0),
            )
            .optional()
            .map_err(ClawError::Sql)?;
        match result {
            None => Ok(None),
            Some(encoded) => Ok(Some(self.decrypt(&encoded)?)),
        }
    }

    async fn delete(&self, key: &str) -> ClawResult<()> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM secrets WHERE key = ?1", rusqlite::params![key])?;
        Ok(())
    }
}
```

Note: `rusqlite::OptionalExtension` must be in scope — add `use rusqlite::OptionalExtension;` at top.

- [ ] **Step 5: Add `SecretStore` error variant and re-export**

In `src/error.rs`, add:
```rust
#[error("secret store error: {0}")]
SecretStore(String),
```

In `src/lib.rs`, add:
```rust
pub mod secrets;
```

- [ ] **Step 6: Run tests**

```bash
cargo test -p mobileclaw-core --features test-utils -- store 2>&1 | grep -E "ok|FAILED"
```
Expected: 5 tests pass — `store_and_retrieve_secret`, `get_missing_returns_none`, `delete_removes_secret`, `ciphertext_in_db_is_not_plaintext`, `wrong_key_fails_to_decrypt`.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml mobileclaw-core/Cargo.toml mobileclaw-core/src/secrets/ mobileclaw-core/src/error.rs mobileclaw-core/src/lib.rs
git commit -m "feat(secrets): SqliteSecretStore with AES-256-GCM encryption"
```

---

## Task 3: EmailAccount persistence helpers

**Files:**
- Modify: `mobileclaw-core/src/secrets/store.rs` — add `put_email_account` / `get_email_account` / `delete_email_account` helper methods on `SqliteSecretStore`
- Modify: `mobileclaw-core/src/secrets/mod.rs` — re-export new trait extension

The account config (non-sensitive fields) is stored as JSON in the regular `secrets` table under key `email:<id>:config`. The password goes under `email:<id>:password`.

- [ ] **Step 1: Write failing test**

```rust
// In src/secrets/store.rs  #[cfg(test)] mod tests:
#[tokio::test]
async fn email_account_save_and_load() {
    use crate::secrets::types::EmailAccount;
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir).await;
    let acc = EmailAccount {
        id: "work".into(),
        smtp_host: "smtp.example.com".into(),
        smtp_port: 587,
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        username: "alice@example.com".into(),
    };
    store.put_email_account(&acc, "s3cr3t").await.unwrap();

    let (loaded_acc, loaded_pw) = store.get_email_account("work").await.unwrap().unwrap();
    assert_eq!(loaded_acc.username, "alice@example.com");
    assert_eq!(loaded_pw.expose(), "s3cr3t");
}

#[tokio::test]
async fn email_account_delete_removes_both_entries() {
    use crate::secrets::types::EmailAccount;
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir).await;
    let acc = EmailAccount {
        id: "tmp".into(),
        smtp_host: "s".into(), smtp_port: 25,
        imap_host: "i".into(), imap_port: 143,
        username: "u".into(),
    };
    store.put_email_account(&acc, "pw").await.unwrap();
    store.delete_email_account("tmp").await.unwrap();
    assert!(store.get_email_account("tmp").await.unwrap().is_none());
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p mobileclaw-core --features test-utils -- email_account_save_and_load 2>&1 | tail -3
```
Expected: compile error — method not found.

- [ ] **Step 3: Implement helper methods on SqliteSecretStore**

Add to the `impl SqliteSecretStore` block in `store.rs`:

```rust
/// Store an email account's config and password atomically.
/// Config (non-secret) is JSON-encrypted; password is separately encrypted.
pub async fn put_email_account(&self, acc: &EmailAccount, password: &str) -> ClawResult<()> {
    let config_json = serde_json::to_string(acc)
        .map_err(|e| ClawError::SecretStore(e.to_string()))?;
    self.put(&format!("email:{}:config", acc.id), &config_json).await?;
    self.put(&format!("email:{}:password", acc.id), password).await?;
    Ok(())
}

/// Load an email account config and its password. Returns `None` if not found.
pub async fn get_email_account(
    &self,
    id: &str,
) -> ClawResult<Option<(EmailAccount, SecretString)>> {
    let config_key = format!("email:{}:config", id);
    let Some(config_secret) = self.get(&config_key).await? else { return Ok(None) };
    let acc: EmailAccount = serde_json::from_str(config_secret.expose())
        .map_err(|e| ClawError::SecretStore(e.to_string()))?;
    let pw_key = format!("email:{}:password", id);
    let Some(pw) = self.get(&pw_key).await? else {
        return Err(ClawError::SecretStore(format!("password missing for {}", id)));
    };
    Ok(Some((acc, pw)))
}

/// Delete all entries for an email account.
pub async fn delete_email_account(&self, id: &str) -> ClawResult<()> {
    self.delete(&format!("email:{}:config", id)).await?;
    self.delete(&format!("email:{}:password", id)).await?;
    Ok(())
}
```

Add `use crate::secrets::types::EmailAccount;` at the top of `store.rs`.

- [ ] **Step 4: Run tests**

```bash
cargo test -p mobileclaw-core --features test-utils -- email_account 2>&1 | grep -E "ok|FAILED"
```
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add mobileclaw-core/src/secrets/store.rs
git commit -m "feat(secrets): email account save/load/delete helpers"
```

---

## Task 4: ToolContext and Permission updates

**Files:**
- Modify: `mobileclaw-core/src/tools/traits.rs` — add `secrets: Arc<dyn SecretStore>` to `ToolContext`
- Modify: `mobileclaw-core/src/tools/permission.rs` — add `EmailSend`, `EmailReceive`
- Modify: `mobileclaw-core/src/ffi.rs` — pass secrets to `ToolContext` in `AgentSession::create`

- [ ] **Step 1: Write failing test**

```rust
// Add to src/tools/permission.rs  #[cfg(test)] mod tests:
#[test]
fn email_permissions_are_distinct() {
    let checker = PermissionChecker::new([Permission::EmailSend]);
    assert!(checker.check(&Permission::EmailSend));
    assert!(!checker.check(&Permission::EmailReceive));
}
```

And in `src/tools/traits.rs` tests, update the `make_ctx` helper (referenced in other test files) to include `secrets`.

- [ ] **Step 2: Add `EmailSend` and `EmailReceive` to Permission enum**

In `src/tools/permission.rs`:
```rust
pub enum Permission {
    FileRead,
    FileWrite,
    HttpFetch,
    MemoryRead,
    MemoryWrite,
    SystemInfo,
    Notifications,
    EmailSend,      // new
    EmailReceive,   // new
}
```

Update `allow_all()` to include both:
```rust
allowed: [
    FileRead, FileWrite, HttpFetch,
    MemoryRead, MemoryWrite, SystemInfo, Notifications,
    EmailSend, EmailReceive,           // new
].into_iter().collect(),
```

- [ ] **Step 3: Add `secrets` field to ToolContext**

In `src/tools/traits.rs`:
```rust
use crate::secrets::SecretStore;  // new import

pub struct ToolContext {
    pub memory: Arc<dyn Memory>,
    pub sandbox_dir: PathBuf,
    pub http_allowlist: Vec<String>,
    pub permissions: Arc<PermissionChecker>,
    pub secrets: Arc<dyn SecretStore>,  // new
}
```

- [ ] **Step 4: Fix all ToolContext construction sites**

Every place that constructs `ToolContext` must now supply `secrets`.

**`src/ffi.rs`** — `AgentSession::create`:
- The `AgentConfig` struct gains a new field: `pub secrets_db_path: String`
- The session key is derived from a fixed dev key for now (Phase 1); replace with platform keystore in Phase 2:

```rust
// In AgentSession::create, after opening memory:
let secrets = Arc::new(
    crate::secrets::store::SqliteSecretStore::open(
        std::path::Path::new(&config.secrets_db_path).to_path_buf(),
        // TODO Phase 2: derive from platform keystore
        b"mobileclaw-dev-key-32bytes000000",
    ).await?
);

let ctx = ToolContext {
    memory: memory.clone() as Arc<dyn Memory>,
    sandbox_dir: config.sandbox_dir.into(),
    http_allowlist: config.http_allowlist,
    permissions: Arc::new(PermissionChecker::allow_all()),
    secrets: secrets.clone(),  // new
};
```

Also store `secrets` in `AgentSession`:
```rust
pub struct AgentSession {
    inner: AgentLoop<ClaudeClient>,
    memory: Arc<SqliteMemory>,
    secrets: Arc<crate::secrets::store::SqliteSecretStore>,  // new
}
```

**Guard the placeholder key against accidental production use:**

The `b"mobileclaw-dev-key-32bytes000000"` placeholder must not ship in release builds. Add a compile-time guard in `ffi.rs`:

```rust
#[cfg(not(debug_assertions))]
compile_error!(
    "AgentSession uses a hardcoded AES key. \
     Replace with platform keystore derivation before building in release mode. \
     See mobileclaw-core/docs/05-flutter-interface.md § Security Contract."
);
```

This makes the plan compile in debug (tests, development) and hard-fail in `--release` until replaced.

**Add `secrets_db_path` to the Dart side and regenerate bindings:**

After adding `secrets_db_path: String` to the Rust `AgentConfig` struct, the flutter_rust_bridge codegen must be re-run so the Dart `AgentConfig` class gains the matching field. Steps:

```bash
# 1. Re-run codegen from the Flutter project root (adjust path as needed)
flutter_rust_bridge_codegen generate

# 2. In the Dart call site that constructs AgentConfig, add the new field:
#    AgentConfig(
#      ...existing fields...,
#      secretsDbPath: path.join(appDir, 'secrets.db'),
#    )
```

Without this step the Flutter build will fail with a missing named parameter error.

**Test files** — `tests/integration_tools.rs` and anywhere `ToolContext` is constructed in tests:

Add a `NullSecretStore` (always returns `None`) in `src/secrets/store.rs` behind `#[cfg(feature = "test-utils")]`:

```rust
#[cfg(feature = "test-utils")]
pub mod test_helpers {
    use super::*;

    pub struct NullSecretStore;

    #[async_trait]
    impl SecretStore for NullSecretStore {
        async fn put(&self, _: &str, _: &str) -> ClawResult<()> { Ok(()) }
        async fn get(&self, _: &str) -> ClawResult<Option<SecretString>> { Ok(None) }
        async fn delete(&self, _: &str) -> ClawResult<()> { Ok(()) }
    }
}
```

Update all test `ToolContext` constructions to add:
```rust
secrets: Arc::new(crate::secrets::store::test_helpers::NullSecretStore),
```

Files to update:
- `src/tools/builtin/file.rs` `make_ctx()`
- `src/tools/builtin/http.rs` (no `make_ctx` — uses `is_url_allowed` directly, no change needed)
- `src/tools/builtin/memory_tools.rs` `make_ctx()`
- `src/tools/builtin/system.rs` `make_ctx()`
- `tests/integration_tools.rs` `make_ctx()`
- `tests/integration_agent.rs` `make_ctx()`

- [ ] **Step 5: Run tests**

```bash
cargo test -p mobileclaw-core --features test-utils 2>&1 | grep -E "^test result"
```
Expected: all tests pass (same count as before, plus `email_permissions_are_distinct`).

- [ ] **Step 6: Commit**

```bash
git add mobileclaw-core/src/tools/ mobileclaw-core/src/secrets/ mobileclaw-core/src/ffi.rs mobileclaw-core/tests/
git commit -m "feat(tools): add EmailSend/EmailReceive permissions and secrets to ToolContext"
```

---

## Task 5: EmailSendTool

**Files:**
- Create: `mobileclaw-core/src/tools/builtin/email.rs` (send half)
- Modify: `Cargo.toml` (workspace) — add `lettre`
- Modify: `mobileclaw-core/Cargo.toml` — wire `lettre`

- [ ] **Step 1: Add lettre dependency**

In workspace `Cargo.toml` `[workspace.dependencies]`:
```toml
lettre = { version = "0.11", default-features = false, features = ["tokio1-rustls-tls", "smtp-transport", "builder"] }
```

In `mobileclaw-core/Cargo.toml` `[dependencies]`:
```toml
lettre = { workspace = true }
```

```bash
cargo check -p mobileclaw-core 2>&1 | grep "^error"
```
Expected: no errors.

- [ ] **Step 2: Write failing test**

```rust
// mobileclaw-core/src/tools/builtin/email.rs (bottom)
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        secrets::store::test_helpers::NullSecretStore,
        tools::{PermissionChecker, ToolContext},
        memory::sqlite::SqliteMemory,
    };
    use std::sync::Arc;
    use tempfile::TempDir;

    fn make_ctx(dir: &TempDir) -> ToolContext {
        let mem = tokio::runtime::Handle::current()
            .block_on(SqliteMemory::open(dir.path().join("m.db")))
            .unwrap();
        ToolContext {
            memory: Arc::new(mem),
            sandbox_dir: dir.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: Arc::new(NullSecretStore),
        }
    }

    #[tokio::test]
    async fn send_missing_account_returns_error() {
        // NullSecretStore returns None for all keys → account not found
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir);
        let tool = EmailSendTool;
        let result = tool.execute(
            serde_json::json!({
                "account_id": "work",
                "to": ["bob@example.com"],
                "subject": "Hello",
                "body": "Hi there"
            }),
            &ctx,
        ).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("account") || msg.contains("work"), "got: {}", msg);
    }

    #[test]
    fn send_tool_metadata() {
        let t = EmailSendTool;
        assert_eq!(t.name(), "email_send");
        assert!(!t.description().is_empty());
        assert!(t.required_permissions().contains(&Permission::EmailSend));
    }

    #[tokio::test]
    async fn send_missing_required_args_errors() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir);
        // Missing "to" field
        let result = EmailSendTool.execute(
            serde_json::json!({"account_id": "work", "subject": "Hi", "body": "body"}),
            &ctx,
        ).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 3: Run to verify failure**

```bash
cargo test -p mobileclaw-core --features test-utils -- email 2>&1 | tail -5
```
Expected: compile error — `email` module not found.

- [ ] **Step 4: Implement EmailSendTool**

Create `mobileclaw-core/src/tools/builtin/email.rs`:

```rust
use async_trait::async_trait;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::header::ContentType,
    transport::smtp::authentication::Credentials,
};
use serde_json::{json, Value};

use crate::{
    ClawError, ClawResult,
    tools::{Permission, Tool, ToolContext, ToolResult},
};

pub struct EmailSendTool;

#[async_trait]
impl Tool for EmailSendTool {
    fn name(&self) -> &str { "email_send" }

    fn description(&self) -> &str {
        "Send an email via SMTP. Requires a configured email account (set up via the app settings)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "account_id": {
                    "type": "string",
                    "description": "Email account ID configured in app settings (e.g. 'work')"
                },
                "to": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Recipient email addresses"
                },
                "subject": {"type": "string"},
                "body":    {"type": "string"},
                "cc": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "CC recipients (optional)"
                }
            },
            "required": ["account_id", "to", "subject", "body"]
        })
    }

    fn required_permissions(&self) -> Vec<Permission> { vec![Permission::EmailSend] }
    fn timeout_ms(&self) -> u64 { 30_000 }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult> {
        let account_id = args["account_id"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'account_id'".into() })?;

        let to_arr = args["to"].as_array()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'to'".into() })?;
        if to_arr.is_empty() {
            return Err(ClawError::Tool { tool: self.name().into(), message: "'to' must not be empty".into() });
        }

        let subject = args["subject"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'subject'".into() })?;
        let body = args["body"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'body'".into() })?;

        // Load account config and password from SecretStore
        // SecretStore is accessed via a downcast — we need SqliteSecretStore specifically.
        // Instead, define an EmailSecretStore helper trait to keep ToolContext generic.
        // For Phase 1, access via the concrete type stored in AgentSession.
        // Here we use a helper: ctx.secrets exposes `get_email_account` if the underlying
        // store implements the extension. We call it via dynamic dispatch on a helper trait.
        //
        // Simpler: SecretStore provides get/put/delete; we reconstruct the email account
        // by fetching the JSON config key and the password key directly.
        let config_key = format!("email:{}:config", account_id);
        let pw_key = format!("email:{}:password", account_id);

        let config_secret = ctx.secrets.get(&config_key).await?
            .ok_or_else(|| ClawError::Tool {
                tool: self.name().into(),
                message: format!("email account '{}' not found; configure it in app settings", account_id),
            })?;

        let acc: crate::secrets::types::EmailAccount =
            serde_json::from_str(config_secret.expose())
                .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;

        let password = ctx.secrets.get(&pw_key).await?
            .ok_or_else(|| ClawError::Tool {
                tool: self.name().into(),
                message: format!("password missing for account '{}'", account_id),
            })?;

        // Build message
        let from = acc.username.parse::<lettre::message::Mailbox>()
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;

        let mut builder = Message::builder()
            .from(from.clone())
            .subject(subject)
            .header(ContentType::TEXT_PLAIN);

        for addr in to_arr {
            let addr_str = addr.as_str().ok_or_else(|| ClawError::Tool {
                tool: self.name().into(), message: "to[] must contain strings".into()
            })?;
            let mailbox = addr_str.parse::<lettre::message::Mailbox>()
                .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;
            builder = builder.to(mailbox);
        }

        if let Some(cc_arr) = args["cc"].as_array() {
            for addr in cc_arr {
                let addr_str = addr.as_str().unwrap_or_default();
                if let Ok(mb) = addr_str.parse::<lettre::message::Mailbox>() {
                    builder = builder.cc(mb);
                }
            }
        }

        let email = builder.body(body.to_string())
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;

        // Connect and send
        let creds = Credentials::new(acc.username.clone(), password.expose().to_string());
        let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(&acc.smtp_host)
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?
            .port(acc.smtp_port)
            .credentials(creds)
            .build();

        mailer.send(email).await
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;

        Ok(ToolResult::ok(json!({"sent": true, "to": to_arr})))
    }
}
```

- [ ] **Step 5: Wire EmailSendTool into mod.rs**

In `src/tools/builtin/mod.rs`, add the module declaration and register only `EmailSendTool` for now (`EmailFetchTool` is registered in Task 6):

```rust
pub mod email;   // new line

pub fn register_all_builtins(registry: &mut ToolRegistry) {
    // ... existing registrations ...
    registry.register_builtin(Arc::new(email::EmailSendTool));  // new — Task 5
    // email::EmailFetchTool registered in Task 6
}
```

- [ ] **Step 6: Run tests**

```bash
cargo test -p mobileclaw-core --features test-utils -- email 2>&1 | grep -E "ok|FAILED"
```
Expected: 3 tests pass.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml mobileclaw-core/Cargo.toml mobileclaw-core/src/tools/builtin/email.rs mobileclaw-core/src/tools/builtin/mod.rs
git commit -m "feat(tools): add EmailSendTool with SMTP via lettre"
```

---

## Task 6: EmailFetchTool

**Files:**
- Modify: `mobileclaw-core/src/tools/builtin/email.rs` — add `EmailFetchTool`
- Modify: `Cargo.toml` (workspace) — add `async-imap`, `tokio-rustls`
- Modify: `mobileclaw-core/Cargo.toml` — wire new deps
- Modify: `mobileclaw-core/src/tools/builtin/mod.rs` — register `EmailFetchTool`

- [ ] **Step 1: Add IMAP dependencies**

In workspace `Cargo.toml`:
```toml
async-imap   = { version = "0.9", default-features = false, features = ["runtime-tokio"] }
tokio-rustls = "0.26"
rustls        = { version = "0.23", default-features = false, features = ["ring"] }
webpki-roots  = "0.26"
```

In `mobileclaw-core/Cargo.toml`:
```toml
async-imap   = { workspace = true }
tokio-rustls = { workspace = true }
rustls        = { workspace = true }
webpki-roots  = { workspace = true }
```

```bash
cargo check -p mobileclaw-core 2>&1 | grep "^error"
```
Expected: no errors.

- [ ] **Step 2: Write failing tests**

```rust
// Add to #[cfg(test)] mod tests in email.rs:

#[tokio::test]
async fn fetch_missing_account_returns_error() {
    let dir = TempDir::new().unwrap();
    let ctx = make_ctx(&dir);
    let result = EmailFetchTool.execute(
        serde_json::json!({"account_id": "work", "folder": "INBOX", "limit": 5}),
        &ctx,
    ).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("account") || msg.contains("work"), "got: {}", msg);
}

#[test]
fn fetch_tool_metadata() {
    let t = EmailFetchTool;
    assert_eq!(t.name(), "email_fetch");
    assert!(!t.description().is_empty());
    assert!(t.required_permissions().contains(&Permission::EmailReceive));
    assert_eq!(t.timeout_ms(), 30_000);
}

#[tokio::test]
async fn fetch_missing_account_id_errors() {
    let dir = TempDir::new().unwrap();
    let ctx = make_ctx(&dir);
    let result = EmailFetchTool.execute(serde_json::json!({}), &ctx).await;
    assert!(result.is_err());
}
```

- [ ] **Step 3: Run to verify failure**

```bash
cargo test -p mobileclaw-core --features test-utils -- fetch 2>&1 | tail -5
```
Expected: compile error — `EmailFetchTool` not found.

- [ ] **Step 4: Implement EmailFetchTool**

Append to `mobileclaw-core/src/tools/builtin/email.rs`:

```rust
use async_imap::Client as ImapClient;
use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, rustls::ClientConfig, client::TlsStream};
use std::sync::Arc as StdArc;

pub struct EmailFetchTool;

#[async_trait]
impl Tool for EmailFetchTool {
    fn name(&self) -> &str { "email_fetch" }

    fn description(&self) -> &str {
        "Fetch recent emails from an IMAP mailbox. Returns subject, sender, date, and snippet for each message."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "account_id": {
                    "type": "string",
                    "description": "Email account ID configured in app settings"
                },
                "folder": {
                    "type": "string",
                    "default": "INBOX",
                    "description": "IMAP folder name (default: INBOX)"
                },
                "limit": {
                    "type": "integer",
                    "default": 10,
                    "description": "Maximum number of recent messages to return (max 50)"
                }
            },
            "required": ["account_id"]
        })
    }

    fn required_permissions(&self) -> Vec<Permission> { vec![Permission::EmailReceive] }
    fn timeout_ms(&self) -> u64 { 30_000 }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ClawResult<ToolResult> {
        let account_id = args["account_id"].as_str()
            .ok_or_else(|| ClawError::Tool { tool: self.name().into(), message: "missing 'account_id'".into() })?;
        let folder = args["folder"].as_str().unwrap_or("INBOX");
        let limit = args["limit"].as_u64().unwrap_or(10).min(50) as u32;

        // Load credentials
        let config_key = format!("email:{}:config", account_id);
        let pw_key = format!("email:{}:password", account_id);

        let config_secret = ctx.secrets.get(&config_key).await?
            .ok_or_else(|| ClawError::Tool {
                tool: self.name().into(),
                message: format!("email account '{}' not found; configure it in app settings", account_id),
            })?;
        let acc: crate::secrets::types::EmailAccount =
            serde_json::from_str(config_secret.expose())
                .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;
        let password = ctx.secrets.get(&pw_key).await?
            .ok_or_else(|| ClawError::Tool {
                tool: self.name().into(),
                message: format!("password missing for account '{}'", account_id),
            })?;

        // TLS connection
        let mut root_store = rustls::RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls_config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        let connector = TlsConnector::from(StdArc::new(tls_config));
        let addr = format!("{}:{}", acc.imap_host, acc.imap_port);
        let tcp = TcpStream::connect(&addr).await
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: format!("connect: {}", e) })?;
        let server_name = acc.imap_host.as_str().try_into()
            .map_err(|e: tokio_rustls::rustls::pki_types::InvalidDnsNameError|
                ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;
        let tls: TlsStream<TcpStream> = connector.connect(server_name, tcp).await
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: format!("tls: {}", e) })?;

        let client = ImapClient::new(tls);
        let mut imap_session = client
            .login(&acc.username, password.expose())
            .await
            .map_err(|(e, _)| ClawError::Tool { tool: self.name().into(), message: format!("login: {}", e) })?;

        // Select folder
        let mailbox = imap_session.select(folder).await
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: format!("select: {}", e) })?;

        let total = mailbox.exists;
        let emails = if total == 0 {
            vec![]
        } else {
            let start = total.saturating_sub(limit - 1);
            let seq = format!("{}:{}", start, total);
            let messages = imap_session
                .fetch(&seq, "(ENVELOPE BODY[TEXT]<0.500>)")
                .await
                .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;

            messages.iter().rev().map(|msg| {
                let env = msg.envelope();
                let subject = env
                    .and_then(|e| e.subject.as_ref())
                    .and_then(|s| std::str::from_utf8(s).ok())
                    .unwrap_or("(no subject)")
                    .to_string();
                let from = env
                    .and_then(|e| e.from.as_ref())
                    .and_then(|f| f.first())
                    .map(|a| {
                        let name = a.name.as_ref()
                            .and_then(|n| std::str::from_utf8(n).ok())
                            .unwrap_or("")
                            .to_string();
                        let mbox = a.mailbox.as_ref()
                            .and_then(|m| std::str::from_utf8(m).ok())
                            .unwrap_or("")
                            .to_string();
                        let host = a.host.as_ref()
                            .and_then(|h| std::str::from_utf8(h).ok())
                            .unwrap_or("")
                            .to_string();
                        if name.is_empty() { format!("{}@{}", mbox, host) }
                        else { format!("{} <{}@{}>", name, mbox, host) }
                    })
                    .unwrap_or_default();
                let date = env
                    .and_then(|e| e.date.as_ref())
                    .and_then(|d| std::str::from_utf8(d).ok())
                    .unwrap_or("")
                    .to_string();
                let snippet = msg.text()
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .map(|s| s.chars().take(200).collect::<String>())
                    .unwrap_or_default();
                json!({ "subject": subject, "from": from, "date": date, "snippet": snippet })
            }).collect()
        };

        imap_session.logout().await.ok(); // best-effort

        Ok(ToolResult::ok(json!({
            "folder": folder,
            "total": total,
            "fetched": emails.len(),
            "messages": emails
        })))
    }
}
```

- [ ] **Step 5: Register in mod.rs**

```rust
registry.register_builtin(Arc::new(email::EmailFetchTool));
```

- [ ] **Step 6: Run tests**

```bash
cargo test -p mobileclaw-core --features test-utils -- email 2>&1 | grep -E "ok|FAILED"
```
Expected: all 6 email tests pass.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml mobileclaw-core/Cargo.toml mobileclaw-core/src/tools/builtin/email.rs mobileclaw-core/src/tools/builtin/mod.rs
git commit -m "feat(tools): add EmailFetchTool with IMAP/TLS via async-imap"
```

---

## Task 7: Flutter FFI — email account management

**Files:**
- Modify: `mobileclaw-core/src/ffi.rs` — add `EmailAccountDto`, three new methods

The Flutter app needs to:
1. Save an email account (user fills a form with host/port/username/password)
2. Load account metadata (to show in settings UI — password is NOT returned)
3. Delete an account

The password never travels from Rust back to Dart. `email_account_load` returns config only; password stays encrypted in the Rust store.

- [ ] **Step 1: Write failing test (Rust side)**

```rust
// Add to a new #[cfg(test)] mod tests at the bottom of ffi.rs:
// (These are wiring tests — they verify the DTO and config key formats)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_account_dto_fields() {
        let dto = EmailAccountDto {
            id: "work".into(),
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            username: "alice@example.com".into(),
        };
        assert_eq!(dto.id, "work");
        assert_eq!(dto.smtp_port, 587);
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p mobileclaw-core --features test-utils -- ffi 2>&1 | tail -5
```
Expected: compile error — `EmailAccountDto` not found.

- [ ] **Step 3: Add EmailAccountDto and FFI methods**

In `src/ffi.rs`, add the DTO struct:

```rust
/// Email account configuration DTO (no password — password is stored encrypted in SecretStore).
pub struct EmailAccountDto {
    pub id: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub imap_host: String,
    pub imap_port: u16,
    pub username: String,
}
```

Add three methods to `impl AgentSession`:

```rust
/// Save an email account configuration and its password.
/// The password is encrypted with AES-256-GCM before storage.
/// After this call, the password cannot be retrieved in plaintext via any FFI method.
pub async fn email_account_save(&self, dto: EmailAccountDto, password: String) -> ClawResult<()> {
    use crate::secrets::types::EmailAccount;
    let acc = EmailAccount {
        id: dto.id,
        smtp_host: dto.smtp_host,
        smtp_port: dto.smtp_port,
        imap_host: dto.imap_host,
        imap_port: dto.imap_port,
        username: dto.username,
    };
    self.secrets.put_email_account(&acc, &password).await
}

/// Load an email account's configuration. Returns None if the account does not exist.
/// The password is NOT returned — only the non-secret config fields.
pub async fn email_account_load(&self, id: String) -> ClawResult<Option<EmailAccountDto>> {
    // Use the Task 3 helper to avoid duplicating the key-format logic.
    let Some((acc, _pw)) = self.secrets.get_email_account(&id).await? else {
        return Ok(None);
    };
    // _pw is dropped here, zeroing the password bytes immediately.
    Ok(Some(EmailAccountDto {
        id: acc.id,
        smtp_host: acc.smtp_host,
        smtp_port: acc.smtp_port,
        imap_host: acc.imap_host,
        imap_port: acc.imap_port,
        username: acc.username,
    }))
}

/// Delete an email account and its stored password.
pub async fn email_account_delete(&self, id: String) -> ClawResult<()> {
    self.secrets.delete_email_account(&id).await
}
```

Note: `secrets` field on `AgentSession` is currently `Arc<SqliteSecretStore>` — it needs `put_email_account` and `delete_email_account`. Since those are concrete methods (not on the `SecretStore` trait), keep the field as `Arc<SqliteSecretStore>` in `AgentSession`.

- [ ] **Step 4: Run tests**

```bash
cargo test -p mobileclaw-core --features test-utils -- ffi 2>&1 | grep -E "ok|FAILED"
```
Expected: `email_account_dto_fields ... ok`.

- [ ] **Step 5: Commit**

```bash
git add mobileclaw-core/src/ffi.rs
git commit -m "feat(ffi): add email_account_save/load/delete FFI methods for Flutter settings UI"
```

---

## Task 8: Integration test

**Files:**
- Create: `mobileclaw-core/tests/integration_email.rs`

These tests exercise the full round-trip (SecretStore → ToolContext → Tool) without a real mail server. They verify the plumbing; SMTP/IMAP network tests are out of scope for CI.

- [ ] **Step 1: Write tests**

```rust
// mobileclaw-core/tests/integration_email.rs
use mobileclaw_core::{
    secrets::{store::SqliteSecretStore, types::EmailAccount},
    tools::{PermissionChecker, ToolContext, ToolRegistry, builtin::register_all_builtins},
    memory::sqlite::SqliteMemory,
};
use std::sync::Arc;
use tempfile::TempDir;

async fn make_store(dir: &TempDir) -> Arc<SqliteSecretStore> {
    Arc::new(
        SqliteSecretStore::open(
            dir.path().join("secrets.db"),
            b"test-key-32-bytes-padding0000000",  // exactly 32 bytes
        )
        .await
        .unwrap(),
    )
}

async fn make_ctx(dir: &TempDir, store: Arc<SqliteSecretStore>) -> ToolContext {
    let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
    ToolContext {
        memory: mem,
        sandbox_dir: dir.path().to_path_buf(),
        http_allowlist: vec![],
        permissions: Arc::new(PermissionChecker::allow_all()),
        secrets: store,
    }
}

#[tokio::test]
async fn email_send_unknown_account_returns_error() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let ctx = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();
    let result = tool.execute(
        serde_json::json!({
            "account_id": "nonexistent",
            "to": ["x@example.com"],
            "subject": "t",
            "body": "b"
        }),
        &ctx,
    ).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn email_fetch_unknown_account_returns_error() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let ctx = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_fetch").unwrap();
    let result = tool.execute(
        serde_json::json!({"account_id": "nonexistent", "folder": "INBOX", "limit": 5}),
        &ctx,
    ).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn email_account_stored_password_is_encrypted() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let acc = EmailAccount {
        id: "test".into(),
        smtp_host: "smtp.example.com".into(), smtp_port: 587,
        imap_host: "imap.example.com".into(), imap_port: 993,
        username: "user@example.com".into(),
    };
    store.put_email_account(&acc, "hunter2").await.unwrap();

    // Verify raw SQLite value does not contain plaintext password
    let conn = rusqlite::Connection::open(dir.path().join("secrets.db")).unwrap();
    let raw: String = conn.query_row(
        "SELECT value FROM secrets WHERE key = 'email:test:password'",
        [],
        |r| r.get(0),
    ).unwrap();
    assert!(!raw.contains("hunter2"), "password must not appear in plaintext in DB");

    // Verify we can retrieve it correctly
    let (_, pw) = store.get_email_account("test").await.unwrap().unwrap();
    assert_eq!(pw.expose(), "hunter2");
}

#[tokio::test]
async fn email_tools_registered_as_builtins() {
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    assert!(reg.get("email_send").is_some());
    assert!(reg.get("email_fetch").is_some());
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p mobileclaw-core --features test-utils --test integration_email 2>&1 | tail -5
```
Expected: compile error (module not yet added).

- [ ] **Step 3: Ensure `SqliteSecretStore`, `EmailAccount` are pub-reachable**

In `src/secrets/store.rs`, ensure `SqliteSecretStore` is `pub`.
In `src/secrets/mod.rs`, add `pub use store::SqliteSecretStore;`.

- [ ] **Step 4: Run tests**

```bash
cargo test -p mobileclaw-core --features test-utils --test integration_email 2>&1 | grep -E "ok|FAILED"
```
Expected: all 4 tests pass.

- [ ] **Step 5: Run full suite**

```bash
cargo test -p mobileclaw-core --features test-utils 2>&1 | grep "^test result"
```
Expected: all test suites pass.

- [ ] **Step 6: Commit**

```bash
git add mobileclaw-core/tests/integration_email.rs mobileclaw-core/src/secrets/mod.rs
git commit -m "test(email): integration tests for email tools and secret store"
```

---

## Task 9: Update documentation

**Files:**
- Modify: `mobileclaw-core/docs/05-flutter-interface.md` — document `EmailAccountDto` and three FFI methods; Dart usage example; security contract (password never returned)
- Modify: `mobileclaw-core/docs/06-dev-standards.md` — add `EmailSend`/`EmailReceive` to security section, document secret store usage rules

- [ ] **Step 1: Update 05-flutter-interface.md**

Add a new section "Email Account Management" after the existing Memory API section:

````markdown
## Email Account Management

Email credentials are configured once by the user and stored encrypted in Rust. Dart never retrieves the password after saving.

### FFI Methods

```dart
// Save account (call once from settings screen)
await agent.emailAccountSave(
  dto: EmailAccountDto(
    id: 'work',
    smtpHost: 'smtp.gmail.com',
    smtpPort: 587,
    imapHost: 'imap.gmail.com',
    imapPort: 993,
    username: 'alice@gmail.com',
  ),
  password: _passwordController.text,  // plaintext, used once
);

// Load config for display (password NOT returned)
final EmailAccountDto? config = await agent.emailAccountLoad(id: 'work');

// Remove account
await agent.emailAccountDelete(id: 'work');
```

### Security Contract

- `emailAccountSave`: password is encrypted with AES-256-GCM before storage. The plaintext is never written to disk, logs, or memory beyond the immediate encryption call.
- `emailAccountLoad`: returns config fields only. There is no `emailAccountGetPassword` method. This is intentional and permanent.
- `emailAccountDelete`: removes both config and encrypted password atomically.

### Dart DTO

```dart
class EmailAccountDto {
  final String id;
  final String smtpHost;
  final int smtpPort;
  final String imapHost;
  final int imapPort;
  final String username;
  // No password field — by design
}
```

### Flutter Settings UI Pattern

```dart
// EmailSettingsScreen calls emailAccountSave with password from a
// SecureTextField (obscured, not cached in widget state after submission).
// After save, clear the password controller immediately:
await agent.emailAccountSave(dto: dto, password: _pwCtrl.text);
_pwCtrl.clear();
```
````

- [ ] **Step 2: Update 06-dev-standards.md**

In `mobileclaw-core/docs/06-dev-standards.md`, locate the heading `### 3.4 附加安全规则 (Additional Security Rules)` and insert the following new subsection immediately after the closing paragraph of that section (before the `---` separator that ends Section 3):

```markdown
### 3.5 密钥存储安全 (Secret Store Security)

- All credentials (email passwords, API keys) must be stored exclusively via `SecretStore::put()` — never in `ToolContext`, `AgentConfig`, logs, or memory without `SecretString`
- `SecretString` must never be passed to `tracing::*!` macros, formatted into error messages, or serialized to JSON
- The AES-256-GCM key passed to `SqliteSecretStore::open()` must originate from the platform keystore (Android Keystore / iOS Keychain) in production builds. The placeholder key in `ffi.rs` must be replaced before Phase 2 release
- `FFI`: no `get_password` or equivalent method may ever be added to the Flutter API surface. If the user needs to change a password, they call `email_account_save` again with the new password
```

- [ ] **Step 3: Run final check**

```bash
cargo test -p mobileclaw-core --features test-utils 2>&1 | grep "^test result"
cargo clippy -p mobileclaw-core --features test-utils -- -D warnings 2>&1 | grep "^error"
```
Expected: all tests pass, zero clippy errors.

- [ ] **Step 4: Commit**

```bash
git add mobileclaw-core/docs/
git commit -m "docs: document email FFI API and secret store security rules"
```

---

## Summary

| Task | Deliverable |
|------|-------------|
| 1 | `SecretString` (zeroize) + `EmailAccount` types |
| 2 | `SecretStore` trait + `SqliteSecretStore` (AES-256-GCM) |
| 3 | `put/get/delete_email_account` helpers |
| 4 | `EmailSend`/`EmailReceive` permissions; `secrets` in `ToolContext` |
| 5 | `EmailSendTool` (SMTP via lettre) |
| 6 | `EmailFetchTool` (IMAP/TLS via async-imap) |
| 7 | Flutter FFI: `email_account_save/load/delete` (password never returned) |
| 8 | Integration tests (4 tests, no live server needed) |
| 9 | Docs: Flutter interface + dev standards |

**Security boundaries enforced:**
- Password stored as AES-256-GCM ciphertext; never in plaintext on disk
- `SecretString` zeroes memory on drop
- No `get_password` FFI method exists — structural guarantee, not a policy
- Platform keystore integration is a TODO for Phase 2 production hardening
