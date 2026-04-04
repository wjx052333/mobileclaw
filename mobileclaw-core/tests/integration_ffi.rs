#![cfg(feature = "test-utils")]
// Run with: cargo test -p mobileclaw-core --features test-utils --test integration_ffi_final

use mobileclaw_core::ffi::{AgentConfig, AgentSession, EmailAccountDto};
use tempfile::TempDir;

fn test_config(dir: &TempDir) -> AgentConfig {
    AgentConfig {
        api_key: None,
        db_path: dir.path().join("memory.db").to_string_lossy().to_string(),
        secrets_db_path: dir.path().join("secrets.db").to_string_lossy().to_string(),
        encryption_key: b"test-key-32-bytes-padding0000000".to_vec(),
        sandbox_dir: dir.path().to_string_lossy().to_string(),
        http_allowlist: vec![],
        model: None,
        skills_dir: None,
        log_dir: None,
        session_dir: None,
        context_window: None,
        max_session_messages: None,
        camera_frames_per_capture: None,
        camera_max_frames_per_capture: None,
        camera_ring_buffer_capacity: None,
    }
}

// ─── Session Creation & Configuration ────────────────────────────────────────

#[tokio::test]
async fn session_create_with_minimal_config() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await;
    assert!(session.is_ok());
}

#[tokio::test]
async fn session_create_with_invalid_encryption_key_fails() {
    let dir = TempDir::new().unwrap();
    let mut cfg = test_config(&dir);
    cfg.encryption_key = b"too-short".to_vec();
    let session = AgentSession::create(cfg).await;
    assert!(session.is_err());
}

// ─── Memory API ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn memory_store_and_recall() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    session.memory_store("user/profile".into(), "John Doe".into(), "user".into()).await.unwrap();

    let recalled = session.memory_get("user/profile".into()).await.unwrap();
    assert!(recalled.is_some());
    assert_eq!(recalled.unwrap().content, "John Doe");
}

#[tokio::test]
async fn memory_search_returns_results() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    session.memory_store("doc1".into(), "Python programming".into(), "conversation".into()).await.unwrap();
    session.memory_store("doc2".into(), "Rust systems programming".into(), "conversation".into()).await.unwrap();

    let results = session.memory_recall("programming".into(), 10, None, None, None).await.unwrap();
    assert!(!results.is_empty());
}

#[tokio::test]
async fn memory_count() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    assert_eq!(session.memory_count().await.unwrap(), 0);
    session.memory_store("doc1".into(), "content".into(), "user".into()).await.unwrap();
    assert_eq!(session.memory_count().await.unwrap(), 1);
}

#[tokio::test]
async fn memory_forget() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    session.memory_store("doc1".into(), "content".into(), "user".into()).await.unwrap();
    assert_eq!(session.memory_count().await.unwrap(), 1);

    session.memory_forget("doc1".into()).await.unwrap();
    assert_eq!(session.memory_count().await.unwrap(), 0);
}

// ─── Email Account API ────────────────────────────────────────────────────────

#[tokio::test]
async fn email_account_save_and_load() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    let dto = EmailAccountDto {
        id: "gmail".into(),
        smtp_host: "smtp.gmail.com".into(),
        smtp_port: 587,
        imap_host: "imap.gmail.com".into(),
        imap_port: 993,
        username: "user@gmail.com".into(),
    };
    session.email_account_save(dto, "password123".into()).await.unwrap();

    let loaded = session.email_account_load("gmail".into()).await.unwrap();
    assert!(loaded.is_some());
    assert_eq!(loaded.unwrap().username, "user@gmail.com");
}

#[tokio::test]
async fn email_account_delete() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    let dto = EmailAccountDto {
        id: "test".into(),
        smtp_host: "smtp.test.com".into(),
        smtp_port: 587,
        imap_host: "imap.test.com".into(),
        imap_port: 993,
        username: "user@test.com".into(),
    };
    session.email_account_save(dto, "password".into()).await.unwrap();

    session.email_account_delete("test".into()).await.unwrap();
    let loaded = session.email_account_load("test".into()).await.unwrap();
    assert!(loaded.is_none());
}

// ─── Query API (no LLM required) ───────────────────────────────────────────────

#[tokio::test]
async fn history_starts_empty() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    let history = session.history();
    assert_eq!(history.len(), 0);
}

#[tokio::test]
async fn skills_returns_list() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    let skills = session.skills();
    let _ = skills;  // Just verify no panic
}

#[tokio::test]
async fn provider_list_starts_empty() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    let providers = session.provider_list().await.unwrap();
    assert_eq!(providers.len(), 0);
}

#[tokio::test]
async fn provider_get_active_returns_none() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    let active = session.provider_get_active().await.unwrap();
    assert!(active.is_none());
}

#[tokio::test]
async fn session_list_without_session_dir_returns_empty() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    let list = session.session_list().await.unwrap();
    assert!(list.is_empty());
}

// ─── Error Cases ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn session_save_without_llm_fails() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    let result = session.session_save().await;
    assert!(result.is_err(), "session_save should fail without LLM client or session_dir");
}

#[tokio::test]
async fn provider_set_active_nonexistent_fails() {
    let dir = TempDir::new().unwrap();
    let cfg = test_config(&dir);
    let session = AgentSession::create(cfg).await.unwrap();

    let result = session.provider_set_active("nonexistent".into()).await;
    assert!(result.is_err());
}
