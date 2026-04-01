// mobileclaw-cli/src/session.rs
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use mobileclaw_core::ffi::{AgentConfig, AgentSession};

/// Default data directory: ~/.mobileclaw/
pub fn default_data_dir() -> PathBuf {
    dirs_or_home().join(".mobileclaw")
}

fn dirs_or_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Ensure data directory exists and return paths to db files.
pub fn prepare_data_dir(data_dir: &Path) -> Result<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("creating data dir {}", data_dir.display()))?;
    Ok((
        data_dir.join("memory.db"),
        data_dir.join("secrets.db"),
    ))
}

/// Build an AgentSession using env vars for the provider config if present.
/// ANTHROPIC_API_KEY env var → api_key; ANTHROPIC_MODEL env var → model override.
/// If neither is set (both None), mobileclaw-core will load the active provider
/// configured via `mclaw provider set-active` from secrets.db.
pub async fn open_session(data_dir: &Path) -> Result<AgentSession> {
    let (memory_db, secrets_db) = prepare_data_dir(data_dir)?;

    let config = AgentConfig {
        api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
        model: std::env::var("ANTHROPIC_MODEL").ok(),
        db_path: memory_db.to_string_lossy().into_owned(),
        sandbox_dir: data_dir.join("sandbox").to_string_lossy().into_owned(),
        http_allowlist: vec![],
        skills_dir: None,
        secrets_db_path: secrets_db.to_string_lossy().into_owned(),
        encryption_key: b"mobileclaw-dev-key-32bytes000000".to_vec(),
    };

    AgentSession::create(config).await
}

/// Open just the SqliteSecretStore (for provider/email management without a full agent).
pub async fn open_secrets(data_dir: &Path) -> Result<mobileclaw_core::secrets::SqliteSecretStore> {
    let (_, secrets_db) = prepare_data_dir(data_dir)?;
    mobileclaw_core::secrets::SqliteSecretStore::open(
        secrets_db,
        b"mobileclaw-dev-key-32bytes000000",
    )
    .await
    .map_err(anyhow::Error::from)
}
