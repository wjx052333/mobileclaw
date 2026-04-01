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
