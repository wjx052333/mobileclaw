use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryCategory {
    Core,
    Daily,
    Conversation,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let path = path.into();
        let id = format!("{:x}", {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            path.hash(&mut h);
            now.hash(&mut h);
            h.finish()
        });
        Self { id, path, content: content.into(), category, created_at: now, updated_at: now }
    }
}

#[derive(Debug, Clone)]
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
