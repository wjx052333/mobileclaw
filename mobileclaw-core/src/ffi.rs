//! FFI API layer for flutter_rust_bridge.
//!
//! Exposes `AgentSession` as an opaque handle and DTOs (plain data structs)
//! that can safely cross the FFI boundary: primitives, String, Vec<T>, Option<T>.
//! No references, no lifetimes, no generic type parameters in public signatures.

use std::{path::Path, sync::Arc};

use flutter_rust_bridge::frb;

use crate::{
    agent::loop_impl::AgentLoop,
    memory::{Memory, MemoryCategory, MemoryDoc, SearchQuery, sqlite::SqliteMemory},
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
}

/// An `AgentEvent` that can cross the FFI boundary.
pub enum AgentEventDto {
    TextDelta { text: String },
    ToolCall { name: String },
    ToolResult { name: String, success: bool },
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

// ─── Private helpers ─────────────────────────────────────────────────────────

fn category_to_string(c: &MemoryCategory) -> String {
    match c {
        MemoryCategory::Core => "core".into(),
        MemoryCategory::Daily => "daily".into(),
        MemoryCategory::Conversation => "conversation".into(),
        MemoryCategory::Custom(s) => format!("custom:{}", s),
    }
}

fn string_to_category(s: &str) -> MemoryCategory {
    match s {
        "core" => MemoryCategory::Core,
        "daily" => MemoryCategory::Daily,
        "conversation" => MemoryCategory::Conversation,
        other if other.starts_with("custom:") => {
            MemoryCategory::Custom(other.trim_start_matches("custom:").into())
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
}

impl AgentSession {
    /// Create a new agent session.
    ///
    /// If `skills_dir` is set, the directory must exist and be readable or `create()` will return an error.
    pub async fn create(config: AgentConfig) -> anyhow::Result<AgentSession> {
        let memory = Arc::new(SqliteMemory::open(Path::new(&config.db_path)).await?);

        // Open secrets store with the AES-256 key derived from the platform keystore by Dart.
        let key: &[u8; 32] = config.encryption_key.as_slice().try_into()
            .map_err(|_| anyhow::anyhow!("encryption_key must be exactly 32 bytes"))?;
        let secrets = Arc::new(
            SqliteSecretStore::open(
                std::path::Path::new(&config.secrets_db_path).to_path_buf(),
                key,
            )
            .await?,
        );

        // Resolve LLM client: active provider from SecretStore, or legacy explicit config
        let llm: std::sync::Arc<dyn crate::llm::client::LlmClient> = {
            use crate::llm::provider::{ProviderConfig, ProviderProtocol, create_llm_client};
            match secrets.active_provider_id().await? {
                Some(id) => {
                    let provider_cfg = secrets.provider_load(&id).await?;
                    let api_key = secrets.provider_api_key(&id).await?;
                    create_llm_client(&provider_cfg, api_key.as_deref())?
                }
                None => {
                    // Backwards-compat: explicit api_key + model in AgentConfig
                    let key = config.api_key.as_deref()
                        .ok_or_else(|| anyhow::anyhow!("no active provider and no api_key in config"))?;
                    let model = config.model.as_deref()
                        .ok_or_else(|| anyhow::anyhow!("no active provider and no model in config"))?;
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

        let ctx = ToolContext {
            memory: memory.clone() as Arc<dyn Memory>, // Arc clone: both AgentSession.memory and AgentLoop's ToolContext must co-own the same memory instance
            sandbox_dir: config.sandbox_dir.into(),
            http_allowlist: config.http_allowlist,
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: secrets.clone() as Arc<dyn crate::secrets::SecretStore>,
        };

        let skills = if let Some(dir) = &config.skills_dir {
            load_skills_from_dir(Path::new(dir)).await?
        } else {
            vec![]
        };
        let skill_mgr = SkillManager::new(skills);

        let inner = AgentLoop::new(llm, registry, ctx, skill_mgr);
        Ok(AgentSession { inner, memory, secrets })
    }

    /// Send a user message and return all events produced by one agent turn.
    pub async fn chat(&mut self, input: String, system: String) -> anyhow::Result<Vec<AgentEventDto>> {
        use crate::agent::loop_impl::AgentEvent;

        let events = self.inner.chat(&input, &system).await?;
        let dtos = events
            .into_iter()
            .map(|e| match e {
                AgentEvent::TextDelta { text } => AgentEventDto::TextDelta { text },
                AgentEvent::ToolCall { name } => AgentEventDto::ToolCall { name },
                AgentEvent::ToolResult { name, success } => {
                    AgentEventDto::ToolResult { name, success }
                }
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
