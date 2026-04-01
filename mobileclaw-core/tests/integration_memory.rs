use mobileclaw_core::memory::{Memory, MemoryCategory, SearchQuery, sqlite::SqliteMemory};
use tempfile::TempDir;

async fn make_memory() -> (SqliteMemory, TempDir) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("memory.db");
    let mem = SqliteMemory::open(db_path).await.unwrap();
    (mem, dir)
}

#[tokio::test]
async fn store_and_get_roundtrip() {
    let (mem, _dir) = make_memory().await;
    mem.store("notes/hello.md", "hello world", MemoryCategory::Core).await.unwrap();
    let doc = mem.get("notes/hello.md").await.unwrap().expect("doc not found");
    assert_eq!(doc.content, "hello world");
    assert_eq!(doc.category, MemoryCategory::Core);
}

#[tokio::test]
async fn full_text_search_finds_document() {
    let (mem, _dir) = make_memory().await;
    mem.store("notes/rust.md", "Rust 是一门系统编程语言", MemoryCategory::Core).await.unwrap();
    mem.store("notes/python.md", "Python 是脚本语言", MemoryCategory::Core).await.unwrap();
    let results = mem.recall(&SearchQuery::new("系统编程")).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].doc.path, "notes/rust.md");
}

#[tokio::test]
async fn store_overwrites_existing_path() {
    let (mem, _dir) = make_memory().await;
    mem.store("notes/x.md", "version 1", MemoryCategory::Core).await.unwrap();
    mem.store("notes/x.md", "version 2", MemoryCategory::Core).await.unwrap();
    assert_eq!(mem.count().await.unwrap(), 1);
    let doc = mem.get("notes/x.md").await.unwrap().unwrap();
    assert_eq!(doc.content, "version 2");
}

#[tokio::test]
async fn forget_removes_document() {
    let (mem, _dir) = make_memory().await;
    mem.store("notes/x.md", "content", MemoryCategory::Core).await.unwrap();
    let removed = mem.forget("notes/x.md").await.unwrap();
    assert!(removed);
    assert!(mem.get("notes/x.md").await.unwrap().is_none());
}

#[tokio::test]
async fn category_filter_works() {
    let (mem, _dir) = make_memory().await;
    mem.store("core.md", "core data", MemoryCategory::Core).await.unwrap();
    mem.store("daily.md", "daily log", MemoryCategory::Daily).await.unwrap();
    // Search for "data" which appears in "core data"
    let q = SearchQuery {
        text: "data".into(),
        category: Some(MemoryCategory::Core),
        limit: 10,
        ..Default::default()
    };
    let results = mem.recall(&q).await.unwrap();
    assert!(!results.is_empty(), "should find at least one Core document");
    assert!(results.iter().all(|r| r.doc.category == MemoryCategory::Core));
}
