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
            .from(from)
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
                let addr_str = addr.as_str().ok_or_else(|| ClawError::Tool {
                    tool: self.name().into(),
                    message: "cc[] must contain strings".into(),
                })?;
                let mb = addr_str.parse::<lettre::message::Mailbox>()
                    .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;
                builder = builder.cc(mb);
            }
        }

        let email = builder.body(body.to_string())
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;

        // Connect and send
        // to_string() allocates a heap copy; lettre's Credentials does not zeroize on drop.
        // This is unavoidable with the lettre API — the copy is short-lived (dropped with `mailer`).
        let creds = Credentials::new(acc.username, password.expose().to_string());
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

    async fn make_ctx(dir: &TempDir) -> ToolContext {
        let mem = Arc::new(SqliteMemory::open(dir.path().join("m.db")).await.unwrap());
        ToolContext {
            memory: mem,
            sandbox_dir: dir.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: Arc::new(NullSecretStore),
        }
    }

    #[tokio::test]
    async fn send_missing_account_returns_error() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
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
        assert!(t.required_permissions().contains(&crate::tools::Permission::EmailSend));
    }

    #[tokio::test]
    async fn send_missing_required_args_errors() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        // Missing "to" field
        let result = EmailSendTool.execute(
            serde_json::json!({"account_id": "work", "subject": "Hi", "body": "body"}),
            &ctx,
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn send_empty_to_errors() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = EmailSendTool.execute(
            serde_json::json!({"account_id": "work", "to": [], "subject": "Hi", "body": "body"}),
            &ctx,
        ).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("empty") || msg.contains("to"), "got: {}", msg);
    }
}
