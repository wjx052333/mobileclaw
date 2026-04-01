use anyhow::Result;
use std::path::Path;
use mobileclaw_core::ffi::EmailAccountDto;
use crate::{env_parser::load_env_file, session::open_session};

/// Store email account from test_env.sh (or the given path).
pub async fn cmd_email_add_from_env(
    data_dir: &Path,
    env_file: &Path,
    account_id: &str,
) -> Result<()> {
    let env = load_env_file(env_file);

    let smtp_host = env.get("SMTP_SERVER").cloned()
        .ok_or_else(|| anyhow::anyhow!("SMTP_SERVER not in env file"))?;
    let smtp_port: i32 = env.get("SMTP_PORT")
        .and_then(|v| v.parse().ok())
        .unwrap_or(465);
    let username = env.get("EMAIL_SENDER").cloned()
        .ok_or_else(|| anyhow::anyhow!("EMAIL_SENDER not in env file"))?;
    let password = env.get("EMAIL_PASSWORD").cloned()
        .ok_or_else(|| anyhow::anyhow!("EMAIL_PASSWORD not in env file"))?;

    // Derive IMAP host from SMTP host: smtp.163.com → imap.163.com
    let imap_host = smtp_host.replace("smtp.", "imap.");
    let imap_port: i32 = 993;

    let session = open_session(data_dir).await?;
    session.email_account_save(
        EmailAccountDto {
            id: account_id.to_string(),
            smtp_host: smtp_host.clone(),
            smtp_port,
            imap_host: imap_host.clone(),
            imap_port,
            username: username.clone(),
        },
        password,
    ).await?;

    println!("Saved email account '{account_id}':");
    println!("  SMTP: {smtp_host}:{smtp_port}");
    println!("  IMAP: {imap_host}:{imap_port}");
    println!("  User: {username}");
    Ok(())
}

/// Add email account interactively (via CLI flags).
pub async fn cmd_email_add(
    data_dir: &Path,
    id: String,
    smtp_host: String,
    smtp_port: u16,
    imap_host: String,
    imap_port: u16,
    username: String,
    password: String,
) -> Result<()> {
    let session = open_session(data_dir).await?;
    session.email_account_save(
        EmailAccountDto {
            id: id.clone(),
            smtp_host,
            smtp_port: smtp_port as i32,
            imap_host,
            imap_port: imap_port as i32,
            username,
        },
        password,
    ).await?;
    println!("Saved email account '{id}'.");
    Ok(())
}

pub async fn cmd_email_list(_data_dir: &Path) -> Result<()> {
    println!("Use `mclaw email fetch <id>` or `mclaw email send <id> ...` to interact with a known account.");
    println!("Use `mclaw email add-from-env --id <id>` to add an account from test_env.sh.");
    Ok(())
}

pub async fn cmd_email_delete(data_dir: &Path, id: String) -> Result<()> {
    let session = open_session(data_dir).await?;
    session.email_account_delete(id.clone()).await?;
    println!("Deleted email account '{id}'.");
    Ok(())
}
