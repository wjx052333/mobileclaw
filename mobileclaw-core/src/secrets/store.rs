use std::path::PathBuf;

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit, OsRng, rand_core::RngCore},
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::{ClawError, ClawResult, secrets::types::{EmailAccount, SecretString}};

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
/// # Schema
/// One table: `secrets(key TEXT PRIMARY KEY, value TEXT NOT NULL)`.
/// Stored value format: `base64(nonce_12_bytes || ciphertext)`.
///
/// # Key management
/// The 32-byte encryption key must be derived from a device-specific secret
/// (Android Keystore / iOS Keychain) by the caller. This struct does not manage
/// key derivation.
///
/// # Phase 1 warning
/// The placeholder key in `ffi.rs` must be replaced with platform keystore
/// derivation before any release build. A `compile_error!` guard enforces this.
pub struct SqliteSecretStore {
    conn: Mutex<Connection>,
    cipher: Aes256Gcm,
}

impl SqliteSecretStore {
    pub async fn open(path: PathBuf, key_bytes: &[u8; 32]) -> ClawResult<Self> {
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS secrets (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS providers (
                id         TEXT PRIMARY KEY,
                name       TEXT NOT NULL,
                protocol   TEXT NOT NULL,
                base_url   TEXT NOT NULL,
                model      TEXT NOT NULL,
                created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS provider_secrets (
                provider_id TEXT PRIMARY KEY REFERENCES providers(id) ON DELETE CASCADE,
                encrypted   TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS kv (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );",
        )?;
        let key = Key::<Aes256Gcm>::from_slice(key_bytes);
        let cipher = Aes256Gcm::new(key);
        Ok(Self {
            conn: Mutex::new(conn),
            cipher,
        })
    }

    fn encrypt(&self, plaintext: &str) -> ClawResult<String> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
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
        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| ClawError::SecretStore("decryption failed (wrong key?)".into()))?;
        let s = String::from_utf8(plaintext)
            .map_err(|e| ClawError::SecretStore(e.to_string()))?;
        Ok(SecretString::new(s))
    }

    /// Store an email account's config (as JSON) and password, both encrypted.
    /// Config is stored under `email:<id>:config`, password under `email:<id>:password`.
    pub async fn put_email_account(&self, acc: &EmailAccount, password: &str) -> ClawResult<()> {
        let config_json = serde_json::to_string(acc)
            .map_err(|e| ClawError::SecretStore(e.to_string()))?;
        self.put(&format!("email:{}:config", acc.id), &config_json).await?;
        self.put(&format!("email:{}:password", acc.id), password).await?;
        Ok(())
    }

    /// Load an email account config and its password.
    /// Returns `None` if the account does not exist.
    /// The config JSON is decrypted and deserialized; the password is returned as a `SecretString`.
    pub async fn get_email_account(
        &self,
        id: &str,
    ) -> ClawResult<Option<(EmailAccount, SecretString)>> {
        let config_key = format!("email:{}:config", id);
        let Some(config_secret) = self.get(&config_key).await? else {
            return Ok(None);
        };
        let acc: EmailAccount = serde_json::from_str(config_secret.expose())
            .map_err(|e| ClawError::SecretStore(e.to_string()))?;
        let pw_key = format!("email:{}:password", id);
        let Some(pw) = self.get(&pw_key).await? else {
            return Err(ClawError::SecretStore(format!("password missing for account '{}'", id)));
        };
        Ok(Some((acc, pw)))
    }

    /// Delete all entries for an email account (both config and password).
    pub async fn delete_email_account(&self, id: &str) -> ClawResult<()> {
        self.delete(&format!("email:{}:config", id)).await?;
        self.delete(&format!("email:{}:password", id)).await?;
        Ok(())
    }

    /// Returns true if at least one email account has been configured.
    /// Used to conditionally register email tools at session creation.
    pub async fn has_email_accounts(&self) -> ClawResult<bool> {
        let conn = self.conn.lock().await;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM secrets WHERE key LIKE 'email:%:config'",
            [],
            |r| r.get(0),
        ).map_err(ClawError::Sql)?;
        Ok(count > 0)
    }

    /// Save (upsert) a provider config. Optionally encrypts and stores an API key.
    pub async fn provider_save(
        &self,
        config: &crate::llm::provider::ProviderConfig,
        api_key: Option<&str>,
    ) -> ClawResult<()> {
        let protocol = serde_json::to_string(&config.protocol)
            .map_err(|e| ClawError::SecretStore(e.to_string()))?;
        // serde serialises enum variants as JSON strings ("anthropic"), strip the quotes.
        let protocol = protocol.trim_matches('"').to_string();
        {
            let conn = self.conn.lock().await;
            conn.execute(
                "INSERT INTO providers (id, name, protocol, base_url, model, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                   name=excluded.name, protocol=excluded.protocol,
                   base_url=excluded.base_url, model=excluded.model",
                rusqlite::params![
                    config.id, config.name, protocol,
                    config.base_url, config.model, config.created_at
                ],
            )?;
        }
        if let Some(key) = api_key {
            let encrypted = self.encrypt(key)?;
            let conn = self.conn.lock().await;
            conn.execute(
                "INSERT INTO provider_secrets (provider_id, encrypted) VALUES (?1, ?2)
                 ON CONFLICT(provider_id) DO UPDATE SET encrypted=excluded.encrypted",
                rusqlite::params![config.id, encrypted],
            )?;
        }
        Ok(())
    }

    /// Load a provider config by ID. Returns `ProviderNotFound` if the ID does not exist.
    pub async fn provider_load(
        &self,
        id: &str,
    ) -> ClawResult<crate::llm::provider::ProviderConfig> {
        let conn = self.conn.lock().await;
        let result: Option<(String, String, String, String, i64)> = conn
            .query_row(
                "SELECT name, protocol, base_url, model, created_at FROM providers WHERE id = ?1",
                rusqlite::params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        match result {
            None => Err(ClawError::ProviderNotFound(id.into())),
            Some((name, protocol_str, base_url, model, created_at)) => {
                let protocol: crate::llm::provider::ProviderProtocol =
                    serde_json::from_str(&format!("\"{}\"", protocol_str))
                        .map_err(|e| ClawError::SecretStore(e.to_string()))?;
                Ok(crate::llm::provider::ProviderConfig {
                    id: id.to_string(),
                    name,
                    protocol,
                    base_url,
                    model,
                    created_at,
                })
            }
        }
    }

    /// Return all stored provider configs ordered by creation time.
    pub async fn provider_list(
        &self,
    ) -> ClawResult<Vec<crate::llm::provider::ProviderConfig>> {
        // Collect all rows while holding the lock, then drop the lock before any
        // further processing so we never hold an async mutex across an await point.
        // We assign the collected vec to a binding before the block closes so that
        // `stmt` and `conn` are dropped before `rows` is returned.
        let rows: Vec<(String, String, String, String, String, i64)> = {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare(
                "SELECT id, name, protocol, base_url, model, created_at \
                 FROM providers ORDER BY created_at ASC",
            )?;
            let collected = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
            collected
        };

        let mut configs = Vec::with_capacity(rows.len());
        for (id, name, protocol_str, base_url, model, created_at) in rows {
            let protocol: crate::llm::provider::ProviderProtocol =
                serde_json::from_str(&format!("\"{}\"", protocol_str))
                    .map_err(|e| ClawError::SecretStore(e.to_string()))?;
            configs.push(crate::llm::provider::ProviderConfig {
                id,
                name,
                protocol,
                base_url,
                model,
                created_at,
            });
        }
        Ok(configs)
    }

    /// Delete a provider config (and its API key via ON DELETE CASCADE).
    pub async fn provider_delete(&self, id: &str) -> ClawResult<()> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM providers WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    /// Return the encrypted API key for a provider, or `None` if none was stored.
    pub async fn provider_api_key(&self, id: &str) -> ClawResult<Option<String>> {
        let encrypted: Option<String> = {
            let conn = self.conn.lock().await;
            conn.query_row(
                "SELECT encrypted FROM provider_secrets WHERE provider_id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .optional()?
        };
        match encrypted {
            None => Ok(None),
            Some(enc) => self.decrypt(&enc).map(|s| Some(s.expose().to_string())),
        }
    }

    /// Return the active provider ID, or `None` if not set.
    pub async fn active_provider_id(&self) -> ClawResult<Option<String>> {
        let conn = self.conn.lock().await;
        let val: Option<String> = conn
            .query_row(
                "SELECT value FROM kv WHERE key = 'active_provider_id'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(val)
    }

    /// Persist the active provider ID.
    pub async fn set_active_provider_id(&self, id: &str) -> ClawResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO kv (key, value) VALUES ('active_provider_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            rusqlite::params![id],
        )?;
        Ok(())
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
        conn.execute(
            "DELETE FROM secrets WHERE key = ?1",
            rusqlite::params![key],
        )?;
        Ok(())
    }
}

/// Test helper: a secret store that accepts writes but always returns None on get.
/// Use in unit tests to satisfy ToolContext.secrets without a real database.
#[cfg(feature = "test-utils")]
pub mod test_helpers {
    use super::*;

    pub struct NullSecretStore;

    #[async_trait]
    impl SecretStore for NullSecretStore {
        async fn put(&self, _: &str, _: &str) -> ClawResult<()> {
            Ok(())
        }
        async fn get(&self, _: &str) -> ClawResult<Option<SecretString>> {
            Ok(None)
        }
        async fn delete(&self, _: &str) -> ClawResult<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let dir = TempDir::new().unwrap();
        let store = open_store(&dir).await;
        store.put("pw", "my_password").await.unwrap();

        let conn = Connection::open(dir.path().join("secrets.db")).unwrap();
        let raw: String = conn
            .query_row(
                "SELECT value FROM secrets WHERE key = 'pw'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!raw.contains("my_password"), "password must not appear in plaintext");
    }

    #[tokio::test]
    async fn wrong_key_fails_to_decrypt() {
        let dir = TempDir::new().unwrap();
        let store = open_store(&dir).await;
        store.put("pw", "secret").await.unwrap();

        let store2 = SqliteSecretStore::open(
            dir.path().join("secrets.db"),
            b"ffffffffffffffffffffffffffffffff",
        )
        .await
        .unwrap();
        let result = store2.get("pw").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn email_account_save_and_load() {
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
        assert_eq!(loaded_acc.smtp_port, 587);
        assert_eq!(loaded_pw.expose(), "s3cr3t");
    }

    #[tokio::test]
    async fn email_account_delete_removes_both_entries() {
        let dir = TempDir::new().unwrap();
        let store = open_store(&dir).await;
        let acc = EmailAccount {
            id: "tmp".into(),
            smtp_host: "s".into(),
            smtp_port: 25,
            imap_host: "i".into(),
            imap_port: 143,
            username: "u".into(),
        };
        store.put_email_account(&acc, "pw").await.unwrap();
        store.delete_email_account("tmp").await.unwrap();
        assert!(store.get_email_account("tmp").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn email_account_not_found_returns_none() {
        let dir = TempDir::new().unwrap();
        let store = open_store(&dir).await;
        assert!(store.get_email_account("missing").await.unwrap().is_none());
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use tempfile::TempDir;
    use crate::llm::provider::{ProviderConfig, ProviderProtocol};

    async fn open_test_store() -> (SqliteSecretStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = SqliteSecretStore::open(
            dir.path().join("secrets.db"),
            b"test-key-32bytes0000000000000000",
        )
        .await
        .unwrap();
        (store, dir)
    }

    #[tokio::test]
    async fn test_provider_save_and_load() {
        let (store, _dir) = open_test_store().await;
        let cfg = ProviderConfig::new("Groq".into(), ProviderProtocol::OpenAiCompat,
            "https://api.groq.com/openai".into(), "mixtral-8x7b".into());
        store.provider_save(&cfg, Some("sk-test")).await.unwrap();

        let loaded = store.provider_load(&cfg.id).await.unwrap();
        assert_eq!(loaded.name, "Groq");
        assert_eq!(loaded.model, "mixtral-8x7b");

        let key = store.provider_api_key(&cfg.id).await.unwrap();
        assert_eq!(key, Some("sk-test".into()));
    }

    #[tokio::test]
    async fn test_provider_list_and_delete() {
        let (store, _dir) = open_test_store().await;
        let a = ProviderConfig::new("A".into(), ProviderProtocol::Anthropic,
            "https://api.anthropic.com".into(), "claude-opus-4-6".into());
        let b = ProviderConfig::new("B".into(), ProviderProtocol::Ollama,
            "http://localhost:11434".into(), "llama3".into());
        store.provider_save(&a, Some("key-a")).await.unwrap();
        store.provider_save(&b, None).await.unwrap();

        let list = store.provider_list().await.unwrap();
        assert_eq!(list.len(), 2);

        store.provider_delete(&a.id).await.unwrap();
        let list = store.provider_list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "B");
    }

    #[tokio::test]
    async fn test_active_provider_id_persistence() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secrets.db");
        let key = b"test-key-32bytes0000000000000000";

        let store = SqliteSecretStore::open(path.clone(), key).await.unwrap();
        let cfg = ProviderConfig::new("X".into(), ProviderProtocol::Ollama,
            "http://localhost:11434".into(), "llama3".into());
        store.provider_save(&cfg, None).await.unwrap();
        store.set_active_provider_id(&cfg.id).await.unwrap();
        drop(store);

        // Re-open from same file — active ID must survive
        let store2 = SqliteSecretStore::open(path, key).await.unwrap();
        assert_eq!(store2.active_provider_id().await.unwrap(), Some(cfg.id));
    }

    #[tokio::test]
    async fn test_provider_not_found_returns_error() {
        let (store, _dir) = open_test_store().await;
        let err = store.provider_load("nonexistent-id").await.unwrap_err();
        assert!(matches!(err, crate::ClawError::ProviderNotFound(_)));
    }
}
