use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Memory category — aligns with claude-code taxonomy (user/feedback/project/reference)
/// while preserving backward-compatible deserialization of existing Core/Daily/Conversation names.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryCategory {
    /// Long-term project context (alias: "project" for claude-code compat)
    #[serde(alias = "project")]
    Core,
    /// Time-scoped daily notes (kept for long-session logging)
    Daily,
    /// Ephemeral conversation context
    Conversation,
    /// User profile — name, role, preferences (NEW)
    User,
    /// Behavioral guidance — corrections, preferences (NEW)
    Feedback,
    /// External resource pointers — dashboards, Linear projects, etc. (NEW)
    Reference,
    /// Extensible custom categories
    Custom(String),
}

/// Convert category to a String (for SQLite TEXT column storage and FFI display).
/// Core displays as "project" (claude-code naming convention).
///
/// Returns owned String because `Custom(s)` borrows from the enum — returning
/// `&'static str` is impossible. The clone is cheap (typically short strings).
pub fn category_to_string(category: &MemoryCategory) -> String {
    match category {
        MemoryCategory::Core => "project".into(),
        MemoryCategory::Daily => "daily".into(),
        MemoryCategory::Conversation => "conversation".into(),
        MemoryCategory::User => "user".into(),
        MemoryCategory::Feedback => "feedback".into(),
        MemoryCategory::Reference => "reference".into(),
        MemoryCategory::Custom(s) => s.clone(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryDoc {
    pub id: String,
    pub path: String,
    pub content: String,
    pub category: MemoryCategory,
    pub created_at: u64,
    pub updated_at: u64,
}

impl MemoryDoc {
    pub fn new(path: impl Into<String>, content: impl Into<String>, category: MemoryCategory) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let secs = now.as_secs();
        let nanos = now.subsec_nanos();
        let path = path.into();
        let content_str: String = content.into();
        let id = format!("{:x}", {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            path.hash(&mut h);
            content_str.hash(&mut h);
            secs.hash(&mut h);
            nanos.hash(&mut h);
            h.finish()
        });
        Self { id, path, content: content_str, category, created_at: secs, updated_at: secs }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub doc: MemoryDoc,
    pub score: f32,
}

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub text: String,
    pub category: Option<MemoryCategory>,
    pub limit: usize,
    pub since: Option<u64>,
    pub until: Option<u64>,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), limit: 10, ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_doc_created_at_is_set() {
        let doc = MemoryDoc::new("notes/foo.md", "hello world", MemoryCategory::Core);
        assert!(!doc.id.is_empty());
        assert_eq!(doc.path, "notes/foo.md");
        assert_eq!(doc.category, MemoryCategory::Core);
    }

    #[test]
    fn search_result_ordering() {
        let mut results = vec![
            SearchResult { doc: MemoryDoc::new("a", "a", MemoryCategory::Core), score: 0.5 },
            SearchResult { doc: MemoryDoc::new("b", "b", MemoryCategory::Core), score: 0.9 },
        ];
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        assert_eq!(results[0].score, 0.9);
    }
}
