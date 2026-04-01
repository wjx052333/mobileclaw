use async_trait::async_trait;
use crate::ClawResult;
use super::types::{MemoryDoc, MemoryCategory, SearchQuery, SearchResult};

#[async_trait]
pub trait Memory: Send + Sync {
    async fn store(&self, path: &str, content: &str, category: MemoryCategory) -> ClawResult<MemoryDoc>;
    async fn recall(&self, query: &SearchQuery) -> ClawResult<Vec<SearchResult>>;
    async fn get(&self, path: &str) -> ClawResult<Option<MemoryDoc>>;
    async fn forget(&self, path: &str) -> ClawResult<bool>;
    async fn count(&self) -> ClawResult<usize>;
}
