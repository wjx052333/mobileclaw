//! FFI API layer for flutter_rust_bridge.
//!
//! Exposes `AgentSession` as an opaque handle and DTOs (plain data structs)
//! that can safely cross the FFI boundary: primitives, String, Vec<T>, Option<T>.
//! No references, no lifetimes, no generic type parameters in public signatures.

use std::{path::Path, sync::Arc};

use flutter_rust_bridge::frb;

use crate::{
    agent::loop_impl::AgentLoop,
    memory::{Memory, MemoryCategory, MemoryDoc, SearchQuery, category_to_string, sqlite::SqliteMemory},
    secrets::store::SqliteSecretStore,
    skill::{SkillManager, SkillTrust, load_skills_from_dir},
    tools::{PermissionChecker, ToolContext, ToolRegistry, builtin::register_all_builtins},
};

// ─── DTOs ────────────────────────────────────────────────────────────────────

/// Configuration passed from Dart when creating a new agent session.
pub struct AgentConfig {
    pub api_key: Option<String>,   // None = load active provider from SecretStore
    pub db_path: String,
    pub secrets_db_path: String,   // path to encrypted secrets database
    pub encryption_key: Vec<u8>,   // 32-byte AES-256 key from platform keystore
    pub sandbox_dir: String,
    pub http_allowlist: Vec<String>,
    pub model: Option<String>,     // None = use model from active provider
    pub skills_dir: Option<String>,
    /// Directory for log files. When `Some`, `mobileclaw.log` is written there.
    /// Platform guidance:
    ///   Android — pass `context.getFilesDir().absolutePath` (or Flutter's
    ///             `getApplicationSupportDirectory()`)
    ///   iOS     — pass `FileManager.default.urls(.applicationSupportDirectory)[0].path`
    ///             (or Flutter's `getApplicationSupportDirectory()`)
    ///   CLI     — leave as `None`; the CLI calls its own `init_logging()` instead.
    /// When `None`, tracing output goes wherever the caller already registered a
    /// subscriber (no-op if none was registered).
    pub log_dir: Option<String>,
    /// Directory for JSONL session transcripts.
    /// When set, each `chat()` call persists the full conversation history.
    /// Platform guidance: pass a writable directory inside the app's sandbox.
    pub session_dir: Option<String>,
    /// Maximum context window tokens (default: 200_000 for Claude Sonnet 4.6).
    /// Controls when context pruning triggers.
    pub context_window: Option<u32>,
}

/// Initialize file-based tracing to `{dir}/mobileclaw.log`.
///
/// Safe to call multiple times — subsequent calls are silently ignored by
/// `try_init()`.  Creates `dir` if it does not exist.
pub fn init_file_logging(dir: &str) {
    use std::fs::{self, OpenOptions};
    use tracing_subscriber::{fmt, EnvFilter};

    if let Err(e) = fs::create_dir_all(dir) {
        // Can't log yet — just print to stderr so the developer sees it.
        eprintln!("[mobileclaw] failed to create log dir '{}': {}", dir, e);
        return;
    }

    let log_path = std::path::Path::new(dir).join("mobileclaw.log");
    let log_file = match OpenOptions::new().create(true).append(true).open(&log_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[mobileclaw] failed to open log file '{}': {}", log_path.display(), e);
            return;
        }
    };

    let _ = fmt()
        .with_env_filter(
            EnvFilter::try_from_env("MCLAW_LOG")
                .unwrap_or_else(|_| EnvFilter::new("debug")),
        )
        .with_writer(std::sync::Mutex::new(log_file))
        .with_ansi(false)
        .try_init();
}

/// An `AgentEvent` that can cross the FFI boundary.
pub enum AgentEventDto {
    TextDelta { text: String },
    ToolCall { name: String },
    ToolResult { name: String, success: bool },
    /// Context-window observability snapshot emitted once per chat() turn.
    ContextStats {
        tokens_before_turn: usize,
        tokens_after_prune: usize,
        messages_pruned: usize,
        history_len: usize,
        pruning_threshold: usize,
    },
    Done,
}

/// A chat history entry.
pub struct MessageDto {
    pub role: String,
    pub content: String,
}

/// Skill manifest as a plain DTO.
pub struct SkillManifestDto {
    pub name: String,
    pub description: String,
    pub trust: String,
    pub keywords: Vec<String>,
    pub allowed_tools: Vec<String>,
}

/// A stored memory document.
pub struct MemoryDocDto {
    pub id: String,
    pub path: String,
    pub content: String,
    pub category: String,
    pub created_at: u64,
    pub updated_at: u64,
}

/// A memory search result.
pub struct SearchResultDto {
    pub doc: MemoryDocDto,
    pub score: f32,
}

/// Email account configuration DTO (no password — password is stored encrypted in SecretStore).
pub struct EmailAccountDto {
    pub id: String,
    pub smtp_host: String,
    pub smtp_port: i32,  // i32 matches Dart's int encoding via FRB SSE
    pub imap_host: String,
    pub imap_port: i32,  // i32 matches Dart's int encoding via FRB SSE
    pub username: String,
}

/// LLM provider configuration DTO.
#[derive(Debug, Clone)]
pub struct ProviderConfigDto {
    pub id: String,
    pub name: String,
    pub protocol: String,    // "anthropic" | "openai_compat" | "ollama"
    pub base_url: String,
    pub model: String,
    pub created_at: i64,
}

/// Result of a connectivity probe against an LLM provider.
#[derive(Debug, Clone)]
pub struct ProbeResultDto {
    pub ok: bool,
    pub latency_ms: u64,
    pub degraded: bool,
    pub error: Option<String>,
}

impl ProviderConfigDto {
    fn to_provider_config(&self) -> crate::ClawResult<crate::llm::provider::ProviderConfig> {
        use crate::llm::provider::ProviderProtocol;
        let protocol = match self.protocol.as_str() {
            "anthropic"     => ProviderProtocol::Anthropic,
            "openai_compat" => ProviderProtocol::OpenAiCompat,
            "ollama"        => ProviderProtocol::Ollama,
            other => return Err(crate::ClawError::Llm(format!("unknown protocol: {other}"))),
        };
        Ok(crate::llm::provider::ProviderConfig {
            id: self.id.clone(),
            name: self.name.clone(),
            protocol,
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            created_at: self.created_at,
        })
    }
}

impl From<crate::llm::provider::ProviderConfig> for ProviderConfigDto {
    fn from(c: crate::llm::provider::ProviderConfig) -> Self {
        let protocol = match c.protocol {
            crate::llm::provider::ProviderProtocol::Anthropic    => "anthropic",
            crate::llm::provider::ProviderProtocol::OpenAiCompat => "openai_compat",
            crate::llm::provider::ProviderProtocol::Ollama       => "ollama",
        };
        Self {
            id: c.id,
            name: c.name,
            protocol: protocol.into(),
            base_url: c.base_url,
            model: c.model,
            created_at: c.created_at,
        }
    }
}

impl From<crate::llm::probe::ProbeResult> for ProbeResultDto {
    fn from(r: crate::llm::probe::ProbeResult) -> Self {
        Self { ok: r.ok, latency_ms: r.latency_ms, degraded: r.degraded, error: r.error }
    }
}

/// Summary of a saved session file — returned by `AgentSession::session_list()`.
#[derive(Debug, Clone)]
pub struct SessionEntryDto {
    pub id: String,
    pub modified: u64,
    pub message_count: usize,
    pub file_path: String,
}

// ─── Private helpers ─────────────────────────────────────────────────────────

fn string_to_category(s: &str) -> MemoryCategory {
    match s {
        "core" | "project" => MemoryCategory::Core,
        "daily" => MemoryCategory::Daily,
        "conversation" => MemoryCategory::Conversation,
        "user" => MemoryCategory::User,
        "feedback" => MemoryCategory::Feedback,
        "reference" => MemoryCategory::Reference,
        other if other.starts_with("custom:") => {
            MemoryCategory::Custom(other.strip_prefix("custom:").unwrap_or(other).into())
        }
        other => MemoryCategory::Custom(other.into()),
    }
}

fn doc_to_dto(doc: MemoryDoc) -> MemoryDocDto {
    MemoryDocDto {
        id: doc.id,
        path: doc.path,
        content: doc.content,
        category: category_to_string(&doc.category),
        created_at: doc.created_at,
        updated_at: doc.updated_at,
    }
}

// ─── AgentSession ─────────────────────────────────────────────────────────────

/// Opaque session handle held by Dart. Dart cannot inspect the internals.
#[frb(opaque)]
pub struct AgentSession {
    inner: AgentLoop<std::sync::Arc<dyn crate::llm::client::LlmClient>>,
    memory: Arc<SqliteMemory>,
    secrets: Arc<SqliteSecretStore>,
    session_dir: Option<std::path::PathBuf>,
}

impl AgentSession {
    /// Create a new agent session.
    ///
    /// If `skills_dir` is set, the directory must exist and be readable or `create()` will return an error.
    pub async fn create(config: AgentConfig) -> anyhow::Result<AgentSession> {
        // Initialize file logging before the first tracing call.
        if let Some(ref dir) = config.log_dir {
            init_file_logging(dir);
        }

        tracing::info!(
            db_path = %config.db_path,
            secrets_db = %config.secrets_db_path,
            sandbox_dir = %config.sandbox_dir,
            skills_dir = ?config.skills_dir,
            has_log_dir = config.log_dir.is_some(),
            "AgentSession::create starting"
        );

        let memory = Arc::new(SqliteMemory::open(Path::new(&config.db_path)).await
            .inspect_err(|e| tracing::error!(error = %e, path = %config.db_path, "failed to open memory db"))?);
        tracing::debug!(path = %config.db_path, "memory db opened");

        // Open secrets store with the AES-256 key derived from the platform keystore by Dart.
        let key: &[u8; 32] = config.encryption_key.as_slice().try_into()
            .map_err(|_| anyhow::anyhow!("encryption_key must be exactly 32 bytes"))?;
        let secrets = Arc::new(
            SqliteSecretStore::open(
                std::path::Path::new(&config.secrets_db_path).to_path_buf(),
                key,
            )
            .await
            .inspect_err(|e| tracing::error!(error = %e, path = %config.secrets_db_path, "failed to open secrets db"))?,
        );
        tracing::debug!(path = %config.secrets_db_path, "secrets db opened");

        // Resolve LLM client: active provider from SecretStore, or legacy explicit config
        let llm: std::sync::Arc<dyn crate::llm::client::LlmClient> = {
            use crate::llm::provider::{ProviderConfig, ProviderProtocol, create_llm_client};
            match secrets.active_provider_id().await? {
                Some(id) => {
                    let provider_cfg = secrets.provider_load(&id).await?;
                    let api_key = secrets.provider_api_key(&id).await?;
                    tracing::info!(
                        provider_id = %id,
                        protocol = %format!("{:?}", provider_cfg.protocol),
                        model = %provider_cfg.model,
                        base_url = %provider_cfg.base_url,
                        "using active provider from secrets db"
                    );
                    create_llm_client(&provider_cfg, api_key.as_deref())
                        .inspect_err(|e| tracing::error!(provider_id = %id, error = %e, "failed to build LLM client"))?
                }
                None => {
                    // Backwards-compat: explicit api_key + model in AgentConfig
                    let key = config.api_key.as_deref()
                        .ok_or_else(|| anyhow::anyhow!("no active provider and no api_key in config"))?;
                    let model = config.model.as_deref()
                        .ok_or_else(|| anyhow::anyhow!("no active provider and no model in config"))?;
                    tracing::info!(model = %model, "no active provider — using legacy api_key from AgentConfig");
                    let cfg = ProviderConfig::new(
                        "legacy".into(),
                        ProviderProtocol::Anthropic,
                        "https://api.anthropic.com".into(),
                        model.to_string(),
                    );
                    create_llm_client(&cfg, Some(key))?
                }
            }
        };

        let mut registry = ToolRegistry::new();
        register_all_builtins(&mut registry);
        tracing::debug!(tool_count = registry.list().len(), "builtins registered");

        let ctx = ToolContext {
            memory: memory.clone() as Arc<dyn Memory>, // Arc clone: both AgentSession.memory and AgentLoop's ToolContext must co-own the same memory instance
            sandbox_dir: config.sandbox_dir.into(),
            http_allowlist: config.http_allowlist,
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: secrets.clone() as Arc<dyn crate::secrets::SecretStore>,
        };

        let skills = if let Some(dir) = &config.skills_dir {
            let loaded = load_skills_from_dir(Path::new(dir)).await
                .inspect_err(|e| tracing::error!(dir = %dir, error = %e, "failed to load skills"))?;
            tracing::info!(dir = %dir, count = loaded.len(), "skills loaded from dir");
            loaded
        } else {
            tracing::debug!("no skills_dir configured");
            vec![]
        };
        let skill_mgr = SkillManager::new(skills);

        let inner = AgentLoop::new(llm, registry, ctx, skill_mgr);

        // Apply context configuration
        let ctx_config = crate::agent::context_manager::ContextConfig {
            max_tokens: config.context_window.unwrap_or(200_000) as usize,
            buffer_tokens: 13_000,
            min_user_turns: 3,
        };
        let inner = inner.with_context_config(ctx_config);

        // Apply session directory
        let inner = if let Some(ref dir_str) = config.session_dir {
            inner.with_session_dir(std::path::PathBuf::from(dir_str))
        } else {
            inner
        };

        let session_dir = config.session_dir.map(std::path::PathBuf::from);
        tracing::info!("AgentSession created successfully");
        Ok(AgentSession { inner, memory, secrets, session_dir })
    }

    /// Send a user message and return all events produced by one agent turn.
    pub async fn chat(&mut self, input: String, system: String) -> anyhow::Result<Vec<AgentEventDto>> {
        use crate::agent::loop_impl::AgentEvent;

        tracing::info!(
            input_len = input.len(),
            input_preview = %input.chars().take(120).collect::<String>(),
            "AgentSession::chat called"
        );

        let events = self.inner.chat(&input, &system).await
            .inspect_err(|e| tracing::error!(error = %e, "AgentSession::chat failed"))?;

        let text_chars: usize = events.iter().map(|e| {
            if let AgentEvent::TextDelta { text } = e { text.len() } else { 0 }
        }).sum();
        let tool_calls: usize = events.iter().filter(|e| matches!(e, AgentEvent::ToolCall { .. })).count();
        tracing::info!(
            event_count = events.len(),
            text_chars,
            tool_calls,
            "AgentSession::chat completed"
        );

        let dtos = events
            .into_iter()
            .map(|e| match e {
                AgentEvent::TextDelta { text } => AgentEventDto::TextDelta { text },
                AgentEvent::ToolCall { name } => AgentEventDto::ToolCall { name },
                AgentEvent::ToolResult { name, success } => {
                    AgentEventDto::ToolResult { name, success }
                }
                AgentEvent::ContextStats(s) => AgentEventDto::ContextStats {
                    tokens_before_turn: s.tokens_before_turn,
                    tokens_after_prune: s.tokens_after_prune,
                    messages_pruned: s.messages_pruned,
                    history_len: s.history_len,
                    pruning_threshold: s.pruning_threshold,
                },
                AgentEvent::Done => AgentEventDto::Done,
            })
            .collect();
        Ok(dtos)
    }

    /// Return a snapshot of the conversation history.
    pub fn history(&self) -> Vec<MessageDto> {
        use crate::llm::types::Role;

        self.inner
            .history()
            .iter()
            .map(|m| MessageDto {
                role: match m.role {
                    Role::User => "user".into(),
                    Role::Assistant => "assistant".into(),
                    Role::System => "system".into(),
                },
                content: m.text_content(),
            })
            .collect()
    }

    /// Return the loaded skills as DTOs.
    pub fn skills(&self) -> Vec<SkillManifestDto> {
        self.inner
            .skills()
            .iter()
            .map(|s| SkillManifestDto {
                name: s.manifest.name.clone(), // String/Vec clone: DTOs must own their data to cross the FFI boundary
                description: s.manifest.description.clone(), // String/Vec clone: DTOs must own their data to cross the FFI boundary
                trust: match s.manifest.trust {
                    SkillTrust::Bundled => "bundled".into(),
                    SkillTrust::Installed => "installed".into(),
                },
                keywords: s.manifest.activation.keywords.clone(), // String/Vec clone: DTOs must own their data to cross the FFI boundary
                allowed_tools: s.manifest.allowed_tools.clone().unwrap_or_default(), // String/Vec clone: DTOs must own their data to cross the FFI boundary
            })
            .collect()
    }

    /// Load skills from a directory and replace the current skill manager.
    pub async fn load_skills_from_dir(&mut self, dir: String) -> anyhow::Result<()> {
        let skills = load_skills_from_dir(Path::new(&dir)).await?;
        self.inner.replace_skills(SkillManager::new(skills));
        Ok(())
    }

    /// Store a document in the memory database.
    pub async fn memory_store(
        &self,
        path: String,
        content: String,
        category: String,
    ) -> anyhow::Result<MemoryDocDto> {
        let cat = string_to_category(&category);
        let doc = self.memory.store(&path, &content, cat).await?;
        Ok(doc_to_dto(doc))
    }

    /// Search the memory database and return ranked results.
    pub async fn memory_recall(
        &self,
        query: String,
        limit: usize,
        category: Option<String>,
        since: Option<u64>,
        until: Option<u64>,
    ) -> anyhow::Result<Vec<SearchResultDto>> {
        let q = SearchQuery {
            text: query,
            limit,
            category: category.as_deref().map(string_to_category),
            since,
            until,
        };
        let results = self.memory.recall(&q).await?;
        Ok(results
            .into_iter()
            .map(|r| SearchResultDto {
                doc: doc_to_dto(r.doc),
                score: r.score,
            })
            .collect())
    }

    /// Retrieve a single memory document by path.
    pub async fn memory_get(&self, path: String) -> anyhow::Result<Option<MemoryDocDto>> {
        let doc = self.memory.get(&path).await?;
        Ok(doc.map(doc_to_dto))
    }

    /// Delete a memory document. Returns true if it existed.
    pub async fn memory_forget(&self, path: String) -> anyhow::Result<bool> {
        self.memory.forget(&path).await.map_err(anyhow::Error::from)
    }

    /// Return the total number of memory documents.
    pub async fn memory_count(&self) -> anyhow::Result<usize> {
        self.memory.count().await.map_err(anyhow::Error::from)
    }

    /// Save an email account configuration and its password.
    /// The password is encrypted with AES-256-GCM before storage.
    /// After this call, the password cannot be retrieved in plaintext via any FFI method.
    pub async fn email_account_save(
        &self,
        dto: EmailAccountDto,
        password: String,
    ) -> anyhow::Result<()> {
        use crate::secrets::types::EmailAccount;
        let acc = EmailAccount {
            id: dto.id,
            smtp_host: dto.smtp_host,
            smtp_port: dto.smtp_port as u16,
            imap_host: dto.imap_host,
            imap_port: dto.imap_port as u16,
            username: dto.username,
        };
        self.secrets.put_email_account(&acc, &password).await.map_err(anyhow::Error::from)
    }

    /// Load an email account's configuration. Returns None if the account does not exist.
    /// The password is NOT returned — only the non-secret config fields.
    pub async fn email_account_load(
        &self,
        id: String,
    ) -> anyhow::Result<Option<EmailAccountDto>> {
        let Some((acc, _pw)) = self.secrets.get_email_account(&id).await? else {
            return Ok(None);
        };
        // _pw is dropped here, zeroing the password bytes
        Ok(Some(EmailAccountDto {
            id: acc.id,
            smtp_host: acc.smtp_host,
            smtp_port: acc.smtp_port as i32,
            imap_host: acc.imap_host,
            imap_port: acc.imap_port as i32,
            username: acc.username,
        }))
    }

    /// Delete an email account and its stored password.
    pub async fn email_account_delete(&self, id: String) -> anyhow::Result<()> {
        self.secrets.delete_email_account(&id).await.map_err(anyhow::Error::from)
    }

    /// Save the current conversation history to a session file.
    /// Returns the absolute path of the saved file, or an error if session_dir is not configured.
    pub async fn session_save(&self) -> anyhow::Result<String> {
        let dir = self.session_dir.as_ref()
            .ok_or_else(|| anyhow::anyhow!("session_dir not configured in AgentConfig"))?;
        let path = crate::agent::session::save_session(dir, self.inner.history()).await?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Load a session from a JSONL file. Replaces current history.
    /// Returns the number of messages loaded.
    pub async fn session_load(&mut self, file_path: String) -> anyhow::Result<usize> {
        let path = std::path::Path::new(&file_path);
        let messages = crate::agent::session::load_session(path).await?;
        let count = messages.len();
        self.inner.set_history(messages);
        Ok(count)
    }

    /// List all saved sessions in the configured session_dir.
    /// Returns an empty vec if session_dir is not configured.
    pub async fn session_list(&self) -> anyhow::Result<Vec<SessionEntryDto>> {
        let dir = match &self.session_dir {
            Some(d) => d,
            None => return Ok(vec![]),
        };
        let entries = crate::agent::session::list_sessions(dir).await?;
        Ok(entries.into_iter().map(|e| SessionEntryDto {
            id: e.id,
            modified: e.modified,
            message_count: e.message_count,
            file_path: e.file_path,
        }).collect())
    }

    /// Delete a saved session file. Path is validated against session_dir.
    /// Returns true if the file existed and was deleted.
    pub async fn session_delete(&self, file_path: String) -> anyhow::Result<bool> {
        let dir = self.session_dir.as_ref()
            .ok_or_else(|| anyhow::anyhow!("session_dir not configured in AgentConfig"))?;
        let path = std::path::Path::new(&file_path);
        Ok(crate::agent::session::delete_session(dir, path).await?)
    }

    /// Save (upsert) a provider config and optionally store its API key encrypted.
    pub async fn provider_save(
        &self,
        config: ProviderConfigDto,
        api_key: Option<String>,
    ) -> anyhow::Result<()> {
        let cfg = config.to_provider_config().map_err(anyhow::Error::from)?;
        self.secrets.provider_save(&cfg, api_key.as_deref()).await.map_err(anyhow::Error::from)
    }

    /// Return all stored provider configs as DTOs.
    pub async fn provider_list(&self) -> anyhow::Result<Vec<ProviderConfigDto>> {
        self.secrets
            .provider_list()
            .await
            .map(|v| v.into_iter().map(ProviderConfigDto::from).collect())
            .map_err(anyhow::Error::from)
    }

    /// Delete a provider config (and its API key via ON DELETE CASCADE).
    pub async fn provider_delete(&self, id: String) -> anyhow::Result<()> {
        self.secrets.provider_delete(&id).await.map_err(anyhow::Error::from)
    }

    /// Set the active provider by ID. Returns an error if the provider does not exist.
    pub async fn provider_set_active(&self, id: String) -> anyhow::Result<()> {
        // Verify provider exists before setting active
        self.secrets.provider_load(&id).await.map_err(anyhow::Error::from)?;
        self.secrets.set_active_provider_id(&id).await.map_err(anyhow::Error::from)
    }

    /// Return the active provider config, or `None` if no active provider is set.
    pub async fn provider_get_active(&self) -> anyhow::Result<Option<ProviderConfigDto>> {
        match self.secrets.active_provider_id().await.map_err(anyhow::Error::from)? {
            None => Ok(None),
            Some(id) => {
                let cfg = self.secrets.provider_load(&id).await.map_err(anyhow::Error::from)?;
                Ok(Some(ProviderConfigDto::from(cfg)))
            }
        }
    }
}

/// Probe an LLM provider for reachability without creating a full session.
/// Returns a `ProbeResultDto` indicating whether the provider is reachable.
pub async fn provider_probe(
    config: ProviderConfigDto,
    api_key: Option<String>,
) -> ProbeResultDto {
    let cfg = match config.to_provider_config() {
        Ok(c) => c,
        Err(e) => return ProbeResultDto {
            ok: false,
            latency_ms: 0,
            degraded: false,
            error: Some(e.to_string()),
        },
    };
    crate::llm::probe::probe_provider(&cfg, api_key.as_deref()).await.into()
}

#[cfg(feature = "test-utils")]
#[cfg(test)]
mod session_ffi_tests {
    use super::*;

    #[test]
    fn session_entry_dto_fields_are_complete() {
        // Verify SessionEntryDto has all expected fields and can be constructed
        let dto = SessionEntryDto {
            id: "session_abc".into(),
            modified: 1234567890,
            message_count: 5,
            file_path: "/tmp/session_abc.jsonl".into(),
        };
        assert_eq!(dto.id, "session_abc");
        assert_eq!(dto.modified, 1234567890);
        assert_eq!(dto.message_count, 5);
        assert_eq!(dto.file_path, "/tmp/session_abc.jsonl");
    }

    #[test]
    fn agent_config_session_fields_are_optional() {
        // Verify AgentConfig can be created without session fields
        let config_without_session = AgentConfig {
            api_key: None,
            db_path: "/tmp/test.db".into(),
            secrets_db_path: "/tmp/secrets.db".into(),
            encryption_key: vec![0u8; 32],
            sandbox_dir: "/tmp/sandbox".into(),
            http_allowlist: vec![],
            model: None,
            skills_dir: None,
            log_dir: None,
            session_dir: None,
            context_window: None,
        };
        assert!(config_without_session.session_dir.is_none());
        assert!(config_without_session.context_window.is_none());

        let config_with_session = AgentConfig {
            api_key: None,
            db_path: "/tmp/test.db".into(),
            secrets_db_path: "/tmp/secrets.db".into(),
            encryption_key: vec![0u8; 32],
            sandbox_dir: "/tmp/sandbox".into(),
            http_allowlist: vec![],
            model: None,
            skills_dir: None,
            log_dir: None,
            session_dir: Some("/tmp/sessions".into()),
            context_window: Some(100_000),
        };
        assert_eq!(config_with_session.session_dir.as_deref(), Some("/tmp/sessions"));
        assert_eq!(config_with_session.context_window, Some(100_000));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_account_dto_fields() {
        let dto = EmailAccountDto {
            id: "work".into(),
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            username: "alice@example.com".into(),
        };
        assert_eq!(dto.id, "work");
        assert_eq!(dto.smtp_port, 587);
    }

    #[test]
    fn provider_config_dto_to_provider_config_anthropic() {
        let dto = ProviderConfigDto {
            id: "p1".into(),
            name: "Claude".into(),
            protocol: "anthropic".into(),
            base_url: "https://api.anthropic.com".into(),
            model: "claude-opus-4-6".into(),
            created_at: 1000,
        };
        let cfg = dto.to_provider_config().unwrap();
        assert_eq!(cfg.id, "p1");
        assert_eq!(cfg.model, "claude-opus-4-6");
        assert!(matches!(cfg.protocol, crate::llm::provider::ProviderProtocol::Anthropic));
    }

    #[test]
    fn provider_config_dto_unknown_protocol_returns_err() {
        let dto = ProviderConfigDto {
            id: "p2".into(),
            name: "Bad".into(),
            protocol: "grpc".into(),
            base_url: "https://example.com".into(),
            model: "m".into(),
            created_at: 0,
        };
        assert!(dto.to_provider_config().is_err());
    }

    #[test]
    fn provider_config_roundtrip_via_dto() {
        use crate::llm::provider::{ProviderConfig, ProviderProtocol};
        let cfg = ProviderConfig {
            id: "p3".into(),
            name: "Ollama Local".into(),
            protocol: ProviderProtocol::Ollama,
            base_url: "http://localhost:11434".into(),
            model: "llama3".into(),
            created_at: 9999,
        };
        let dto = ProviderConfigDto::from(cfg.clone());
        assert_eq!(dto.protocol, "ollama");
        assert_eq!(dto.model, "llama3");
        let cfg2 = dto.to_provider_config().unwrap();
        assert_eq!(cfg2.id, cfg.id);
        assert_eq!(cfg2.base_url, cfg.base_url);
        assert!(matches!(cfg2.protocol, ProviderProtocol::Ollama));
    }

    #[test]
    fn probe_result_dto_from_probe_result() {
        let r = crate::llm::probe::ProbeResult {
            ok: true,
            latency_ms: 42,
            degraded: false,
            error: None,
        };
        let dto = ProbeResultDto::from(r);
        assert!(dto.ok);
        assert_eq!(dto.latency_ms, 42);
        assert!(dto.error.is_none());
    }
}

#[cfg(test)]
mod category_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn string_to_category_never_panics(s in ".*") {
            let _ = string_to_category(&s);
        }

        #[test]
        fn custom_prefix_strips_exactly_once(suffix in "[a-zA-Z0-9_]+") {
            let input = format!("custom:{}", suffix);
            let cat = string_to_category(&input);
            match cat {
                MemoryCategory::Custom(s) => prop_assert_eq!(s, suffix),
                _ => prop_assert!(false, "custom: prefix must produce Custom variant"),
            }
        }
    }
}
