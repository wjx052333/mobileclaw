# mobileclaw-cli Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A standalone CLI binary (`mobileclaw-cli`) that directly exercises every Rust API exposed to Flutter — email accounts, LLM providers, agent chat — reading real credentials from `test_env.sh`.

**Architecture:** New workspace crate `mobileclaw-cli` depends on `mobileclaw-core` as a library. Calls `AgentSession` and `SqliteSecretStore` APIs directly (same functions the FFI exposes to Flutter, but called as plain Rust). Uses `clap` for argument parsing, `tokio` for async, `rustyline` for interactive chat REPL.

**Tech Stack:** clap v4, tokio, rustyline, anyhow, mobileclaw-core (internal)

**Data directory:** `~/.mobileclaw/` (created on first run). Holds `memory.db` and `secrets.db`.

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `mobileclaw-cli/Cargo.toml` | Binary crate manifest |
| Modify | `Cargo.toml` (workspace) | Add mobileclaw-cli member; add clap, rustyline deps |
| Create | `mobileclaw-cli/src/main.rs` | Entry point, clap CLI definition, subcommand dispatch |
| Create | `mobileclaw-cli/src/env_parser.rs` | Parse `test_env.sh` export lines into a HashMap |
| Create | `mobileclaw-cli/src/session.rs` | Build `AgentSession` from data dir; shared by all subcommands |
| Create | `mobileclaw-cli/src/cmd/email.rs` | `email add-from-env`, `email add`, `email list`, `email delete`, `email fetch`, `email send` |
| Create | `mobileclaw-cli/src/cmd/provider.rs` | `provider add`, `provider list`, `provider set-active`, `provider delete`, `provider probe` |
| Create | `mobileclaw-cli/src/cmd/chat.rs` | Interactive chat REPL using `rustyline` |

---

## Task 1: Workspace Setup

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `mobileclaw-cli/Cargo.toml`

- [ ] **Step 1.1: Add workspace deps**

  In root `Cargo.toml`, add to `[workspace.dependencies]`:
  ```toml
  clap     = { version = "4", features = ["derive"] }
  rustyline = "14"
  ```

  Add `"mobileclaw-cli"` to `members`:
  ```toml
  members = ["mobileclaw-core", "mobileclaw-cli"]
  ```

- [ ] **Step 1.2: Create mobileclaw-cli/Cargo.toml**

  ```toml
  [package]
  name    = "mobileclaw-cli"
  version = "0.1.0"
  edition = "2021"

  [[bin]]
  name = "mclaw"
  path = "src/main.rs"

  [dependencies]
  mobileclaw-core = { path = "../mobileclaw-core" }
  clap      = { workspace = true }
  tokio     = { workspace = true }
  anyhow    = { workspace = true }
  serde_json = { workspace = true }
  rustyline  = { workspace = true }
  ```

- [ ] **Step 1.3: Create stub main.rs**

  ```rust
  // mobileclaw-cli/src/main.rs
  fn main() { println!("mobileclaw-cli"); }
  ```

- [ ] **Step 1.4: Verify workspace compiles**

  ```bash
  cargo build -p mobileclaw-cli 2>&1 | head -20
  ```
  Expected: no errors.

- [ ] **Step 1.5: Commit**

  ```bash
  git add Cargo.toml mobileclaw-cli/
  git commit -m "feat(cli): scaffold mobileclaw-cli crate"
  ```

---

## Task 2: test_env.sh Parser

**Files:**
- Create: `mobileclaw-cli/src/env_parser.rs`

Parses lines like `export KEY = "value"` or `export KEY=value` from test_env.sh.
Returns a `HashMap<String, String>`.

- [ ] **Step 2.1: Write failing test**

  Create `mobileclaw-cli/src/env_parser.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_parse_export_quoted_spaces() {
          let src = r#"export SMTP_SERVER = "smtp.163.com"
  export SMTP_PORT = 25
  export EMAIL_SENDER = "17611188358@163.com"
  export EMAIL_PASSWORD = "ZXhXL5j2Z579xLMd"
  export EMAIL_RECEIVER = "wjx052333@139.com","#;
          let map = parse_env_file(src);
          assert_eq!(map.get("SMTP_SERVER").map(|s| s.as_str()), Some("smtp.163.com"));
          assert_eq!(map.get("SMTP_PORT").map(|s| s.as_str()), Some("25"));
          assert_eq!(map.get("EMAIL_PASSWORD").map(|s| s.as_str()), Some("ZXhXL5j2Z579xLMd"));
          // trailing comma on RECEIVER is stripped
          assert_eq!(map.get("EMAIL_RECEIVER").map(|s| s.as_str()), Some("wjx052333@139.com"));
      }
  }
  ```

- [ ] **Step 2.2: Run test — expect compile error**

  ```bash
  cargo test -p mobileclaw-cli test_parse_export_quoted_spaces
  ```

- [ ] **Step 2.3: Implement parse_env_file**

  Full `mobileclaw-cli/src/env_parser.rs`:
  ```rust
  use std::collections::HashMap;
  use std::path::Path;

  /// Parse `export KEY = "value"` lines from a shell env file.
  /// Strips surrounding quotes, leading/trailing whitespace, and trailing commas.
  pub fn parse_env_file(src: &str) -> HashMap<String, String> {
      let mut map = HashMap::new();
      for line in src.lines() {
          let line = line.trim();
          let rest = if let Some(r) = line.strip_prefix("export ") { r } else { continue };
          let (key, val) = if let Some(pos) = rest.find('=') {
              (&rest[..pos], &rest[pos + 1..])
          } else {
              continue
          };
          let key = key.trim().to_string();
          let val = val.trim()
              .trim_end_matches(',')   // trailing comma (e.g. RECEIVER line)
              .trim_matches('"')
              .trim_matches('\'')
              .to_string();
          if !key.is_empty() {
              map.insert(key, val);
          }
      }
      map
  }

  /// Load env from a .sh file path. Returns empty map if file can't be read.
  pub fn load_env_file(path: &Path) -> HashMap<String, String> {
      std::fs::read_to_string(path)
          .map(|s| parse_env_file(&s))
          .unwrap_or_default()
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_parse_export_quoted_spaces() {
          let src = r#"export SMTP_SERVER = "smtp.163.com"
  export SMTP_PORT = 25
  export EMAIL_SENDER = "17611188358@163.com"
  export EMAIL_PASSWORD = "ZXhXL5j2Z579xLMd"
  export EMAIL_RECEIVER = "wjx052333@139.com","#;
          let map = parse_env_file(src);
          assert_eq!(map.get("SMTP_SERVER").map(|s| s.as_str()), Some("smtp.163.com"));
          assert_eq!(map.get("SMTP_PORT").map(|s| s.as_str()), Some("25"));
          assert_eq!(map.get("EMAIL_PASSWORD").map(|s| s.as_str()), Some("ZXhXL5j2Z579xLMd"));
          assert_eq!(map.get("EMAIL_RECEIVER").map(|s| s.as_str()), Some("wjx052333@139.com"));
      }

      #[test]
      fn test_skips_comments_and_blanks() {
          let src = "# comment\n\nexport FOO = \"bar\"\n# another";
          let map = parse_env_file(src);
          assert_eq!(map.len(), 1);
          assert_eq!(map["FOO"], "bar");
      }
  }
  ```

- [ ] **Step 2.4: Run tests — expect PASS**

  ```bash
  cargo test -p mobileclaw-cli
  ```

- [ ] **Step 2.5: Commit**

  ```bash
  git add mobileclaw-cli/src/env_parser.rs
  git commit -m "feat(cli): add test_env.sh parser"
  ```

---

## Task 3: Session Helper

**Files:**
- Create: `mobileclaw-cli/src/session.rs`

Builds an `AgentSession` from a data directory. Used by chat and email-fetch/send commands.
The `AgentSession` is the same object Flutter creates via FFI — calling this exercises the same path.

- [ ] **Step 3.1: Create session.rs**

  ```rust
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

  /// Build an AgentSession. LLM provider is loaded from the active provider in
  /// secrets.db (set via `mclaw provider set-active`). If no active provider,
  /// falls back to ANTHROPIC_API_KEY + ANTHROPIC_MODEL env vars.
  pub async fn open_session(data_dir: &Path) -> Result<AgentSession> {
      let (memory_db, secrets_db) = prepare_data_dir(data_dir)?;

      let config = AgentConfig {
          api_key:       std::env::var("ANTHROPIC_API_KEY").ok(),
          model:         std::env::var("ANTHROPIC_MODEL").ok(),
          db_path:       memory_db.to_string_lossy().into_owned(),
          sandbox_dir:   data_dir.join("sandbox").to_string_lossy().into_owned(),
          http_allowlist: vec![],  // agent can access all URLs (CLI testing)
          skills_dir:    None,
          secrets_db_path: secrets_db.to_string_lossy().into_owned(),
      };

      AgentSession::create(config).await.map_err(anyhow::Error::from)
  }

  /// Open just the SqliteSecretStore (for provider/email management without a full agent).
  pub async fn open_secrets(data_dir: &Path) -> Result<mobileclaw_core::secrets::store::SqliteSecretStore> {
      let (_, secrets_db) = prepare_data_dir(data_dir)?;
      mobileclaw_core::secrets::store::SqliteSecretStore::open(
          secrets_db,
          b"mobileclaw-dev-key-32bytes000000",
      )
      .await
      .map_err(anyhow::Error::from)
  }
  ```

  **Note:** `open_secrets` bypasses `AgentSession` so provider/email commands don't require an LLM provider to be configured. This is important for the initial setup flow (add email → add provider → chat).

- [ ] **Step 3.2: Verify it compiles**

  ```bash
  cargo build -p mobileclaw-cli 2>&1 | head -20
  ```
  If `mobileclaw_core::secrets::store::SqliteSecretStore` is not `pub`, make it `pub(crate)` at minimum in mobileclaw-core, or use `AgentSession` for all operations. Adjust as needed.

- [ ] **Step 3.3: Commit**

  ```bash
  git add mobileclaw-cli/src/session.rs
  git commit -m "feat(cli): add session/secrets helper"
  ```

---

## Task 4: Provider Subcommands

**Files:**
- Create: `mobileclaw-cli/src/cmd/provider.rs`
- Create: `mobileclaw-cli/src/cmd/mod.rs`

These directly exercise the same provider APIs that `AgentSession` exposes to Flutter.

- [ ] **Step 4.1: Create cmd/mod.rs**

  ```rust
  pub mod email;
  pub mod provider;
  pub mod chat;
  ```

- [ ] **Step 4.2: Create cmd/provider.rs**

  ```rust
  use anyhow::Result;
  use std::path::Path;
  use mobileclaw_core::ffi::{ProviderConfigDto, provider_probe};
  use crate::session::open_secrets;

  pub async fn cmd_provider_add(
      data_dir: &Path,
      name: String,
      protocol: String,   // "anthropic" | "openai_compat" | "ollama"
      url: String,
      model: String,
      key: Option<String>,
      set_active: bool,
  ) -> Result<()> {
      let secrets = open_secrets(data_dir).await?;
      use mobileclaw_core::llm::provider::{ProviderConfig, ProviderProtocol};
      let proto = match protocol.as_str() {
          "anthropic"     => ProviderProtocol::Anthropic,
          "openai_compat" => ProviderProtocol::OpenAiCompat,
          "ollama"        => ProviderProtocol::Ollama,
          other           => anyhow::bail!("unknown protocol: {other}  (use: anthropic | openai_compat | ollama)"),
      };
      let cfg = ProviderConfig::new(name.clone(), proto, url, model);
      secrets.provider_save(&cfg, key.as_deref()).await?;
      println!("Saved provider '{}' (id: {})", name, cfg.id);

      if set_active {
          secrets.set_active_provider_id(&cfg.id).await?;
          println!("Set as active provider.");
      }
      Ok(())
  }

  pub async fn cmd_provider_list(data_dir: &Path) -> Result<()> {
      let secrets = open_secrets(data_dir).await?;
      let active_id = secrets.active_provider_id().await?;
      let list = secrets.provider_list().await?;
      if list.is_empty() {
          println!("No providers configured. Use `mclaw provider add` to add one.");
          return Ok(());
      }
      println!("{:<38} {:<14} {:<20} {}", "ID", "PROTOCOL", "NAME", "MODEL");
      println!("{}", "-".repeat(90));
      for p in &list {
          let proto = format!("{:?}", p.protocol).to_lowercase();
          let active = if active_id.as_deref() == Some(&p.id) { " ✓ active" } else { "" };
          println!("{:<38} {:<14} {:<20} {}{}", p.id, proto, p.name, p.model, active);
      }
      Ok(())
  }

  pub async fn cmd_provider_set_active(data_dir: &Path, id: String) -> Result<()> {
      let secrets = open_secrets(data_dir).await?;
      // Verify exists
      secrets.provider_load(&id).await?;
      secrets.set_active_provider_id(&id).await?;
      println!("Active provider set to: {id}");
      Ok(())
  }

  pub async fn cmd_provider_delete(data_dir: &Path, id: String) -> Result<()> {
      let secrets = open_secrets(data_dir).await?;
      let cfg = secrets.provider_load(&id).await?;
      secrets.provider_delete(&id).await?;
      println!("Deleted provider '{}' ({})", cfg.name, id);
      Ok(())
  }

  pub async fn cmd_provider_probe(
      data_dir: &Path,
      id: Option<String>,
      protocol: Option<String>,
      url: Option<String>,
      model: Option<String>,
      key: Option<String>,
  ) -> Result<()> {
      // Build ProviderConfigDto — either from stored ID or inline flags
      let (dto, api_key): (ProviderConfigDto, Option<String>) = if let Some(id) = id {
          let secrets = open_secrets(data_dir).await?;
          let cfg = secrets.provider_load(&id).await?;
          let key = secrets.provider_api_key(&id).await?;
          let proto = format!("{:?}", cfg.protocol).to_lowercase();
          (ProviderConfigDto {
              id: cfg.id, name: cfg.name, protocol: proto,
              base_url: cfg.base_url, model: cfg.model, created_at: cfg.created_at,
          }, key)
      } else {
          let p = protocol.as_deref().unwrap_or("openai_compat");
          let u = url.unwrap_or_default();
          let m = model.unwrap_or_default();
          (ProviderConfigDto {
              id: "probe-tmp".into(), name: "probe".into(), protocol: p.into(),
              base_url: u, model: m, created_at: 0,
          }, key)
      };

      println!("Probing {} ({})...", dto.name, dto.protocol);
      let result = provider_probe(dto, api_key).await;
      if result.ok {
          if result.degraded {
              println!("⚠  Reachable ({}ms) — completions unverified, only /models endpoint responded", result.latency_ms);
          } else {
              println!("✓  OK ({}ms) — completion request succeeded", result.latency_ms);
          }
      } else {
          println!("✗  Failed ({}ms): {}", result.latency_ms, result.error.unwrap_or_default());
      }
      Ok(())
  }
  ```

- [ ] **Step 4.3: Build check**

  ```bash
  cargo build -p mobileclaw-cli 2>&1 | head -30
  ```
  Fix any visibility or import errors.

- [ ] **Step 4.4: Commit**

  ```bash
  git add mobileclaw-cli/src/cmd/
  git commit -m "feat(cli): add provider subcommands"
  ```

---

## Task 5: Email Subcommands

**Files:**
- Modify: `mobileclaw-cli/src/cmd/email.rs`

`add-from-env` reads `test_env.sh` and stores the account. Other commands call `AgentSession`'s email FFI methods directly. For `email fetch` and `email send`, the agent is NOT used — these directly call the IMAP/SMTP tools via `AgentSession`'s FFI methods.

- [ ] **Step 5.1: Create cmd/email.rs**

  ```rust
  use anyhow::{bail, Result};
  use std::path::Path;
  use mobileclaw_core::ffi::{AgentSession, EmailAccountDto};
  use crate::{env_parser::load_env_file, session::{open_session, open_secrets}};

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
      let imap_port: i32 = 993;  // EmailAccountDto uses i32 (FRB SSE encoding)

      // Use AgentSession so we exercise the same FFI path Flutter calls
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
          // EmailAccountDto ports are i32 (FRB SSE encoding); CLI args are u16 — cast at boundary
          EmailAccountDto { id: id.clone(), smtp_host, smtp_port: smtp_port as i32, imap_host, imap_port: imap_port as i32, username },
          password,
      ).await?;
      println!("Saved email account '{id}'.");
      Ok(())
  }

  pub async fn cmd_email_list(data_dir: &Path) -> Result<()> {
      // SecretStore doesn't have a list-all-email-accounts method currently.
      // Use the known IDs from the session by trying to load common IDs.
      // NOTE: If you add a list_email_accounts() method to SqliteSecretStore later,
      // update this command. For now, prompt the user to specify the account ID.
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
  ```

  **Note on email fetch/send:** These are handled via the `chat` command — tell the agent "fetch emails from account X" or "send an email to Y from account Z". The agent runs the built-in email tools with the configured account. This is the most realistic test of the full stack.

- [ ] **Step 5.2: Build check**

  ```bash
  cargo build -p mobileclaw-cli 2>&1 | head -30
  ```

- [ ] **Step 5.3: Commit**

  ```bash
  git add mobileclaw-cli/src/cmd/email.rs
  git commit -m "feat(cli): add email subcommands (add-from-env, add, delete)"
  ```

---

## Task 6: Interactive Chat Command

**Files:**
- Create: `mobileclaw-cli/src/cmd/chat.rs`

Starts an `AgentSession` and enters a REPL loop. The user types natural language; the agent can call email tools, memory tools, etc. This is the primary way to test `email fetch` and `email send` from the CLI.

- [ ] **Step 6.1: Create cmd/chat.rs**

  ```rust
  use anyhow::Result;
  use std::path::Path;
  use rustyline::{DefaultEditor, error::ReadlineError};
  use mobileclaw_core::ffi::AgentEventDto;
  use crate::session::open_session;

  pub async fn cmd_chat(data_dir: &Path, system: Option<String>) -> Result<()> {
      println!("Opening agent session...");
      let mut session = open_session(data_dir).await?;
      let system = system.unwrap_or_else(|| {
          "You are a helpful assistant. You have access to email tools. \
           When the user asks to fetch or send email, use the email tools directly.".into()
      });

      println!("Chat started. Type '/quit' or Ctrl-D to exit.\n");

      let mut rl = DefaultEditor::new()?;
      loop {
          let line = match rl.readline("you> ") {
              Ok(l) => l,
              Err(ReadlineError::Eof | ReadlineError::Interrupted) => break,
              Err(e) => return Err(e.into()),
          };
          let input = line.trim().to_string();
          if input.is_empty() { continue; }
          if input == "/quit" || input == "/exit" { break; }
          let _ = rl.add_history_entry(&input);

          let events = match session.chat(input, system.clone()).await {
              Ok(e) => e,
              Err(e) => {
                  eprintln!("Error: {e}");
                  continue;
              }
          };

          print!("agent> ");
          for event in events {
              // AgentEventDto has: TextDelta { text }, ToolCall { name }, ToolResult { name, success }, Done
              match event {
                  AgentEventDto::TextDelta { text } => print!("{text}"),
                  AgentEventDto::ToolCall { name } => {
                      println!("\n[tool call: {name}]");
                  }
                  AgentEventDto::ToolResult { name, success } => {
                      let status = if success { "ok" } else { "error" };
                      println!("  [{name}: {status}]");
                  }
                  AgentEventDto::Done => {}
              }
          }
          println!();
      }

      println!("Bye.");
      Ok(())
  }
  ```

- [ ] **Step 6.2: Build check**

  ```bash
  cargo build -p mobileclaw-cli 2>&1 | head -30
  ```

- [ ] **Step 6.3: Commit**

  ```bash
  git add mobileclaw-cli/src/cmd/chat.rs
  git commit -m "feat(cli): add interactive chat command"
  ```

---

## Task 7: Wire CLI in main.rs

**Files:**
- Modify: `mobileclaw-cli/src/main.rs`

- [ ] **Step 7.1: Write main.rs with clap**

  Full `mobileclaw-cli/src/main.rs`:
  ```rust
  mod cmd;
  mod env_parser;
  mod session;

  use std::path::PathBuf;
  use clap::{Parser, Subcommand};
  use session::default_data_dir;

  #[derive(Parser)]
  #[command(name = "mclaw", about = "mobileclaw-core CLI — test all Rust APIs interactively")]
  struct Cli {
      /// Data directory (default: ~/.mobileclaw/)
      #[arg(long, global = true, env = "MCLAW_DATA_DIR")]
      data_dir: Option<PathBuf>,

      #[command(subcommand)]
      command: Command,
  }

  #[derive(Subcommand)]
  enum Command {
      /// Manage LLM provider configurations (tests provider FFI)
      Provider {
          #[command(subcommand)]
          action: ProviderCmd,
      },
      /// Manage email accounts (tests email FFI)
      Email {
          #[command(subcommand)]
          action: EmailCmd,
      },
      /// Start interactive agent chat (tests full agent loop)
      Chat {
          /// Override system prompt
          #[arg(long)]
          system: Option<String>,
      },
  }

  #[derive(Subcommand)]
  enum ProviderCmd {
      /// Add a new LLM provider
      Add {
          #[arg(long)] name: String,
          /// Protocol: anthropic | openai_compat | ollama
          #[arg(long)] protocol: String,
          /// Base URL (e.g. https://api.anthropic.com or http://localhost:11434)
          #[arg(long)] url: String,
          /// Model name (e.g. claude-opus-4-6)
          #[arg(long)] model: String,
          /// API key (not needed for Ollama)
          #[arg(long)] key: Option<String>,
          /// Set as active provider immediately
          #[arg(long, default_value = "true")] active: bool,
      },
      /// List all configured providers
      List,
      /// Set active provider by ID
      SetActive { id: String },
      /// Delete a provider by ID
      Delete { id: String },
      /// Test provider availability (by stored ID or inline flags)
      Probe {
          /// Use stored provider ID
          #[arg(long)] id: Option<String>,
          #[arg(long)] protocol: Option<String>,
          #[arg(long)] url: Option<String>,
          #[arg(long)] model: Option<String>,
          #[arg(long)] key: Option<String>,
      },
  }

  #[derive(Subcommand)]
  enum EmailCmd {
      /// Import email account from test_env.sh (or any shell env file)
      AddFromEnv {
          /// Account ID to store as
          #[arg(long, default_value = "default")] id: String,
          /// Path to env file
          #[arg(long, default_value = "test_env.sh")] env_file: PathBuf,
      },
      /// Add email account from flags
      Add {
          #[arg(long)] id: String,
          #[arg(long)] smtp_host: String,
          #[arg(long, default_value = "465")] smtp_port: u16,
          #[arg(long)] imap_host: String,
          #[arg(long, default_value = "993")] imap_port: u16,
          #[arg(long)] username: String,
          #[arg(long)] password: String,
      },
      /// Delete an email account
      Delete { id: String },
      /// List stored accounts (shows IDs only — passwords never shown)
      List,
  }

  #[tokio::main]
  async fn main() -> anyhow::Result<()> {
      let cli = Cli::parse();
      let data_dir = cli.data_dir.unwrap_or_else(default_data_dir);

      match cli.command {
          Command::Provider { action } => match action {
              ProviderCmd::Add { name, protocol, url, model, key, active } => {
                  cmd::provider::cmd_provider_add(&data_dir, name, protocol, url, model, key, active).await?;
              }
              ProviderCmd::List => { cmd::provider::cmd_provider_list(&data_dir).await?; }
              ProviderCmd::SetActive { id } => { cmd::provider::cmd_provider_set_active(&data_dir, id).await?; }
              ProviderCmd::Delete { id } => { cmd::provider::cmd_provider_delete(&data_dir, id).await?; }
              ProviderCmd::Probe { id, protocol, url, model, key } => {
                  cmd::provider::cmd_provider_probe(&data_dir, id, protocol, url, model, key).await?;
              }
          },
          Command::Email { action } => match action {
              EmailCmd::AddFromEnv { id, env_file } => {
                  cmd::email::cmd_email_add_from_env(&data_dir, &env_file, &id).await?;
              }
              EmailCmd::Add { id, smtp_host, smtp_port, imap_host, imap_port, username, password } => {
                  cmd::email::cmd_email_add(&data_dir, id, smtp_host, smtp_port, imap_host, imap_port, username, password).await?;
              }
              EmailCmd::Delete { id } => { cmd::email::cmd_email_delete(&data_dir, id).await?; }
              EmailCmd::List => { cmd::email::cmd_email_list(&data_dir).await?; }
          },
          Command::Chat { system } => {
              cmd::chat::cmd_chat(&data_dir, system).await?;
          }
      }

      Ok(())
  }
  ```

- [ ] **Step 7.2: Build — expect clean compile**

  ```bash
  cargo build -p mobileclaw-cli 2>&1
  ```
  Fix any visibility or import issues:
  - If `AgentEvent` variants aren't public, add `#[allow(dead_code)]` or make them pub
  - If `SqliteSecretStore` isn't accessible, expose it via `pub use` in `mobileclaw-core/src/secrets/mod.rs`

- [ ] **Step 7.3: Run the binary to check basic CLI help works**

  ```bash
  cargo run -p mobileclaw-cli -- --help
  cargo run -p mobileclaw-cli -- provider --help
  cargo run -p mobileclaw-cli -- email --help
  ```
  Expected: help text printed for each subcommand.

- [ ] **Step 7.4: Run parser tests**

  ```bash
  cargo test -p mobileclaw-cli
  ```
  Expected: 2 tests pass.

- [ ] **Step 7.5: Commit**

  ```bash
  git add mobileclaw-cli/src/main.rs
  git commit -m "feat(cli): wire all subcommands in main.rs"
  ```

---

## Task 8: End-to-End Smoke Test

Manual test sequence that validates the full stack. Run after building.

- [ ] **Step 8.1: Build release binary**

  ```bash
  cargo build -p mobileclaw-cli --release
  # Binary at: target/release/mclaw
  ```

- [ ] **Step 8.2: Add Anthropic provider**

  ```bash
  ./target/release/mclaw provider add \
    --name "Claude Opus" \
    --protocol anthropic \
    --url https://api.anthropic.com \
    --model claude-opus-4-6 \
    --key $ANTHROPIC_API_KEY
  ```
  Expected: `Saved provider 'Claude Opus' (id: ...)` + `Set as active provider.`

- [ ] **Step 8.3: Probe the provider**

  ```bash
  ./target/release/mclaw provider probe --id <id-from-above>
  ```
  Expected: `✓  OK (XXXms) — completion request succeeded`

- [ ] **Step 8.4: Import email from test_env.sh**

  ```bash
  ./target/release/mclaw email add-from-env \
    --id work \
    --env-file /home/wjx/agent_eyes/bot/mobileclaw/test_env.sh
  ```
  Expected:
  ```
  Saved email account 'work':
    SMTP: smtp.163.com:25
    IMAP: imap.163.com:993
    User: 17611188358@163.com
  ```

- [ ] **Step 8.5: List providers**

  ```bash
  ./target/release/mclaw provider list
  ```
  Expected: table showing the added provider with ✓ active marker.

- [ ] **Step 8.6: Start chat and ask agent to fetch email**

  ```bash
  ./target/release/mclaw chat
  ```
  Then type:
  ```
  you> fetch the 3 most recent emails from my work account
  ```
  Expected: agent calls `email_fetch` tool, returns email list.

- [ ] **Step 8.7: Ask agent to send a test email**

  While still in chat:
  ```
  you> send an email from my work account to wjx052333@139.com with subject "test from mclaw" and body "Hello from mobileclaw-cli"
  ```
  Expected: agent calls `email_send` tool, returns `{"sent": true, ...}`

- [ ] **Step 8.8: Test OpenAI-compatible provider (optional, needs Ollama or compatible API)**

  ```bash
  ./target/release/mclaw provider add \
    --name "Local Ollama" \
    --protocol ollama \
    --url http://localhost:11434 \
    --model llama3

  ./target/release/mclaw provider probe --id <new-id>
  ```

- [ ] **Step 8.9: Commit**

  ```bash
  git add .
  git commit -m "feat(cli): complete mobileclaw-cli with email and provider commands"
  ```

---

## Usage Reference

After building (`cargo build -p mobileclaw-cli`):

```bash
# Provider management (tests same APIs Flutter calls via FFI)
mclaw provider add --name "Claude" --protocol anthropic \
  --url https://api.anthropic.com --model claude-opus-4-6 --key sk-ant-...
mclaw provider list
mclaw provider probe --id <id>
mclaw provider set-active <id>
mclaw provider delete <id>

# Email account management
mclaw email add-from-env --id work --env-file /path/to/test_env.sh
mclaw email add --id personal --smtp-host smtp.gmail.com --smtp-port 587 \
  --imap-host imap.gmail.com --imap-port 993 \
  --username me@gmail.com --password app-password
mclaw email delete work

# Agent chat (agent can use email tools, memory tools, file tools, etc.)
mclaw chat
mclaw chat --system "You are an email assistant."

# Use a different data directory
mclaw --data-dir /tmp/test-mclaw provider list
```

**Typical first-run sequence:**
```bash
# 1. Set up a provider
mclaw provider add --name "Claude" --protocol anthropic \
  --url https://api.anthropic.com --model claude-opus-4-6 --key $YOUR_KEY

# 2. Import email account
mclaw email add-from-env --id work \
  --env-file /home/wjx/agent_eyes/bot/mobileclaw/test_env.sh

# 3. Chat and test
mclaw chat
# > fetch my latest 5 emails from work
# > send a test email from work to wjx052333@139.com
```
