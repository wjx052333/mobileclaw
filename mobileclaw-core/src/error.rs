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

    #[error("provider not found: '{0}'")]
    ProviderNotFound(String),

    #[error(transparent)]
    Sql(#[from] rusqlite::Error),

    #[error("session error: {0}")]
    Session(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type ClawResult<T> = Result<T, ClawError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_not_found_display() {
        let e = ClawError::ProviderNotFound("abc-123".into());
        assert_eq!(e.to_string(), "provider not found: 'abc-123'");
    }
}
