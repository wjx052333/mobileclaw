use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClawError {
    #[error("memory error: {0}")]
    Memory(String),

    #[error("tool error: {tool} — {message}")]
    Tool { tool: String, message: String },

    #[error("tool name conflict: '{0}' is a protected built-in name")]
    ToolNameConflict(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("path traversal attempt: '{0}'")]
    PathTraversal(String),

    #[error("url not in allowlist: '{0}'")]
    UrlNotAllowed(String),

    #[error("skill load error: {0}")]
    SkillLoad(String),

    #[error("llm error: {0}")]
    Llm(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("secret store error: {0}")]
    SecretStore(String),

    #[error(transparent)]
    Sql(#[from] rusqlite::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type ClawResult<T> = Result<T, ClawError>;
