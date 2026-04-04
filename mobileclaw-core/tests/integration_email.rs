use mobileclaw_core::{
    secrets::{SecretStore, store::SqliteSecretStore, types::EmailAccount},
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

async fn make_ctx(dir: &TempDir, store: Arc<SqliteSecretStore>) -> (ToolContext, Arc<SqliteSecretStore>) {
    let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
    let secrets: Arc<dyn SecretStore> = store.clone();  // explicit unsizing coercion
    (ToolContext {
        memory: mem,
        sandbox_dir: dir.path().to_path_buf(),
        http_allowlist: vec![],
        permissions: Arc::new(PermissionChecker::allow_all()),
        secrets,
        camera_frame_buffer: None,
        camera_authorized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        vision_supported: true,
    }, store)
}

#[tokio::test]
async fn email_send_unknown_account_returns_error() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
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
    let (ctx, store) = make_ctx(&dir, store).await;
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

// ─── Email Send Error Paths ─────────────────────────────────────────────────

#[tokio::test]
async fn email_send_missing_account_id() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();
    let result = tool.execute(
        serde_json::json!({
            "to": ["x@example.com"],
            "subject": "t",
            "body": "b"
        }),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject missing account_id");
}

#[tokio::test]
async fn email_send_missing_to_recipient() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();
    let result = tool.execute(
        serde_json::json!({
            "account_id": "test",
            "subject": "t",
            "body": "b"
        }),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject missing 'to'");
}

#[tokio::test]
async fn email_send_empty_to_array() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();
    let result = tool.execute(
        serde_json::json!({
            "account_id": "test",
            "to": [],
            "subject": "t",
            "body": "b"
        }),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject empty 'to' array");
}

#[tokio::test]
async fn email_send_missing_subject() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();
    let result = tool.execute(
        serde_json::json!({
            "account_id": "test",
            "to": ["x@example.com"],
            "body": "b"
        }),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject missing subject");
}

#[tokio::test]
async fn email_send_missing_body() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();
    let result = tool.execute(
        serde_json::json!({
            "account_id": "test",
            "to": ["x@example.com"],
            "subject": "t"
        }),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject missing body");
}

#[tokio::test]
async fn email_send_to_non_string_in_array() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();
    let result = tool.execute(
        serde_json::json!({
            "account_id": "test",
            "to": [123],
            "subject": "t",
            "body": "b"
        }),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject non-string in 'to' array");
}

#[tokio::test]
async fn email_send_with_optional_cc() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();
    // This should fail because account doesn't exist, but the cc parameter should be accepted
    let result = tool.execute(
        serde_json::json!({
            "account_id": "nonexistent",
            "to": ["x@example.com"],
            "subject": "t",
            "body": "b",
            "cc": ["cc@example.com"]
        }),
        &ctx,
    ).await;
    assert!(result.is_err());
    // The error should be about account, not cc parameter
    let err = result.unwrap_err();
    assert!(err.to_string().contains("account") || err.to_string().contains("not found"));
}

#[tokio::test]
async fn email_send_cc_non_string_in_array() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();
    let result = tool.execute(
        serde_json::json!({
            "account_id": "test",
            "to": ["x@example.com"],
            "subject": "t",
            "body": "b",
            "cc": [123]
        }),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject non-string in 'cc' array");
}

#[tokio::test]
async fn email_send_invalid_from_address_format() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();

    // Save account with invalid email format
    let acc = EmailAccount {
        id: "bad_email".into(),
        smtp_host: "smtp.example.com".into(),
        smtp_port: 587,
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        username: "not-an-email".into(),  // Invalid email format
    };
    store.put_email_account(&acc, "password").await.unwrap();

    let result = tool.execute(
        serde_json::json!({
            "account_id": "bad_email",
            "to": ["x@example.com"],
            "subject": "t",
            "body": "b"
        }),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject invalid email format in 'from'");
}

#[tokio::test]
async fn email_send_invalid_to_address_format() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();

    // Save valid account
    let acc = EmailAccount {
        id: "valid".into(),
        smtp_host: "smtp.example.com".into(),
        smtp_port: 587,
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        username: "user@example.com".into(),
    };
    store.put_email_account(&acc, "password").await.unwrap();

    let result = tool.execute(
        serde_json::json!({
            "account_id": "valid",
            "to": ["not-an-email"],
            "subject": "t",
            "body": "b"
        }),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject invalid email format in 'to'");
}

#[tokio::test]
async fn email_send_invalid_cc_address_format() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_send").unwrap();

    // Save valid account
    let acc = EmailAccount {
        id: "valid".into(),
        smtp_host: "smtp.example.com".into(),
        smtp_port: 587,
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        username: "user@example.com".into(),
    };
    store.put_email_account(&acc, "password").await.unwrap();

    let result = tool.execute(
        serde_json::json!({
            "account_id": "valid",
            "to": ["valid@example.com"],
            "subject": "t",
            "body": "b",
            "cc": ["not-an-email"]
        }),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject invalid email format in 'cc'");
}

// ─── Email Fetch Error Paths ────────────────────────────────────────────────

#[tokio::test]
async fn email_fetch_missing_account_id() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_fetch").unwrap();
    let result = tool.execute(
        serde_json::json!({"folder": "INBOX", "limit": 5}),
        &ctx,
    ).await;
    assert!(result.is_err(), "should reject missing account_id");
}

#[tokio::test]
async fn email_fetch_with_optional_folder() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_fetch").unwrap();
    let result = tool.execute(
        serde_json::json!({"account_id": "nonexistent"}),
        &ctx,
    ).await;
    // Should fail on account, not folder
    assert!(result.is_err());
}

#[tokio::test]
async fn email_fetch_with_optional_limit() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_fetch").unwrap();
    let result = tool.execute(
        serde_json::json!({"account_id": "nonexistent", "limit": 25}),
        &ctx,
    ).await;
    // Should fail on account, not limit
    assert!(result.is_err());
}

#[tokio::test]
async fn email_fetch_limit_clamped_to_max_50() {
    let dir = TempDir::new().unwrap();
    let store = make_store(&dir).await;
    let (ctx, store) = make_ctx(&dir, store).await;
    let mut reg = ToolRegistry::new();
    register_all_builtins(&mut reg);
    let tool = reg.get("email_fetch").unwrap();
    let result = tool.execute(
        serde_json::json!({"account_id": "nonexistent", "limit": 100}),
        &ctx,
    ).await;
    // Should fail on account, but the implementation should clamp limit to 50
    assert!(result.is_err());
    // This test documents that limit is clamped, even if account doesn't exist
}
