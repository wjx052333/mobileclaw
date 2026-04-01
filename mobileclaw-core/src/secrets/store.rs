use std::path::PathBuf;

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit, OsRng, rand_core::RngCore},
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rusqlite::{Connection, OptionalExtension};
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
            "CREATE TABLE IF NOT EXISTS secrets (
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
}
