//! FFI API layer for flutter_rust_bridge.
//!
//! Exposes `AgentSession` as an opaque handle and DTOs (plain data structs)
//! that can safely cross the FFI boundary: primitives, String, Vec<T>, Option<T>.
//! No references, no lifetimes, no generic type parameters in public signatures.

use std::{path::Path, sync::Arc};

use flutter_rust_bridge::frb;

use crate::{
    agent::loop_impl::AgentLoop,
    llm::client::ClaudeClient,
    memory::{Memory, MemoryCategory, MemoryDoc, SearchQuery, sqlite::SqliteMemory},
    skill::{SkillManager, SkillTrust, load_skills_from_dir},
    tools::{PermissionChecker, ToolContext, ToolRegistry, builtin::register_all_builtins},
};

// ─── DTOs ────────────────────────────────────────────────────────────────────

/// Configuration passed from Dart when creating a new agent session.
pub struct AgentConfig {
    pub api_key: String,
    pub db_path: String,
    pub sandbox_dir: String,
    pub http_allowlist: Vec<String>,
    pub model: String,
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
    inner: AgentLoop<ClaudeClient>,
    memory: Arc<SqliteMemory>,
}

impl AgentSession {
    /// Create a new agent session.
    ///
    /// If `skills_dir` is set, the directory must exist and be readable or `create()` will return an error.
    pub async fn create(config: AgentConfig) -> anyhow::Result<AgentSession> {
        let llm = ClaudeClient::new(&config.api_key, &config.model);

        let memory = Arc::new(SqliteMemory::open(Path::new(&config.db_path)).await?);

        let mut registry = ToolRegistry::new();
        register_all_builtins(&mut registry);

        let ctx = ToolContext {
            memory: memory.clone() as Arc<dyn Memory>,
            sandbox_dir: config.sandbox_dir.into(),
            http_allowlist: config.http_allowlist,
            permissions: Arc::new(PermissionChecker::allow_all()),
        };

        let skills = if let Some(dir) = &config.skills_dir {
            load_skills_from_dir(Path::new(dir)).await?
        } else {
            vec![]
        };
        let skill_mgr = SkillManager::new(skills);

        let inner = AgentLoop::new(llm, registry, ctx, skill_mgr);
        Ok(AgentSession { inner, memory })
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
                name: s.manifest.name.clone(),
                description: s.manifest.description.clone(),
                trust: match s.manifest.trust {
                    SkillTrust::Bundled => "bundled".into(),
                    SkillTrust::Installed => "installed".into(),
                },
                keywords: s.manifest.activation.keywords.clone(),
                allowed_tools: s.manifest.allowed_tools.clone().unwrap_or_default(),
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
}
