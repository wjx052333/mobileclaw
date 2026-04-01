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

use async_imap::Client as ImapClient;
use futures::StreamExt;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
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
        let limit = args["limit"].as_u64().unwrap_or(10).clamp(1, 50) as u32;

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
        let tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        let connector = TlsConnector::from(StdArc::new(tls_config));
        let addr = format!("{}:{}", acc.imap_host, acc.imap_port);
        let tcp = TcpStream::connect(&addr).await
            .map_err(|e| ClawError::Tool { tool: self.name().into(), message: format!("connect: {}", e) })?;
        let server_name: rustls::pki_types::ServerName<'static> =
            rustls::pki_types::ServerName::try_from(acc.imap_host.as_str())
                .map_err(|e: rustls::pki_types::InvalidDnsNameError|
                    ClawError::Tool { tool: self.name().into(), message: e.to_string() })?
                .to_owned();
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
            let start = total.saturating_sub(limit.saturating_sub(1)).max(1);
            let seq = format!("{}:{}", start, total);
            let fetch_stream = imap_session
                .fetch(&seq, "(ENVELOPE BODY[TEXT]<0.500>)")
                .await
                .map_err(|e| ClawError::Tool { tool: self.name().into(), message: e.to_string() })?;

            let messages: Vec<_> = fetch_stream
                .filter_map(|r| async move {
                    r.map_err(|e| tracing::warn!(error = %e, "IMAP fetch stream error, skipping message"))
                     .ok()
                })
                .collect()
                .await;

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

    #[tokio::test]
    async fn fetch_missing_account_returns_error() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
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
        assert!(t.required_permissions().contains(&crate::tools::Permission::EmailReceive));
        assert_eq!(t.timeout_ms(), 30_000);
    }

    #[tokio::test]
    async fn fetch_missing_account_id_errors() {
        let dir = TempDir::new().unwrap();
        let ctx = make_ctx(&dir).await;
        let result = EmailFetchTool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }
}
