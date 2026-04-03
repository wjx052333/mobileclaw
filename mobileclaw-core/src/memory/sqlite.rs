use async_trait::async_trait;
use rusqlite::{Connection, params};
use std::{path::Path, sync::Mutex};
use crate::{ClawError, ClawResult};
use super::{traits::Memory, types::{MemoryCategory, MemoryDoc, SearchQuery, SearchResult, category_to_string}};

pub struct SqliteMemory {
    conn: Mutex<Connection>,
}

fn str_to_category(s: &str) -> MemoryCategory {
    match s {
        "core" | "project" => MemoryCategory::Core,
        "daily" => MemoryCategory::Daily,
        // New taxonomy takes priority over legacy "conversation" → "user" conflict
        "user" => MemoryCategory::User,
        "feedback" => MemoryCategory::Feedback,
        "reference" => MemoryCategory::Reference,
        "conversation" => MemoryCategory::Conversation,
        other if other.starts_with("custom:") => {
            MemoryCategory::Custom(other.strip_prefix("custom:").unwrap_or(other).into())
        }
        other => {
            tracing::warn!("Unknown memory category string '{}', treating as Custom", other);
            MemoryCategory::Custom(other.into())
        }
    }
}

impl SqliteMemory {
    pub async fn open(path: impl AsRef<Path>) -> ClawResult<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA mmap_size = 67108864;
            PRAGMA cache_size = -4000;
            CREATE TABLE IF NOT EXISTS documents (
                id          TEXT PRIMARY KEY,
                path        TEXT NOT NULL UNIQUE,
                category    TEXT NOT NULL,
                content     TEXT NOT NULL,
                created_at  INTEGER NOT NULL,
                updated_at  INTEGER NOT NULL
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS docs_fts USING fts5(
                path, content, category,
                content='documents',
                content_rowid='rowid',
                tokenize='trigram'
            );
            CREATE TRIGGER IF NOT EXISTS docs_fts_insert
            AFTER INSERT ON documents BEGIN
                INSERT INTO docs_fts(rowid, path, content, category)
                VALUES (new.rowid, new.path, new.content, new.category);
            END;
            CREATE TRIGGER IF NOT EXISTS docs_fts_delete
            AFTER DELETE ON documents BEGIN
                INSERT INTO docs_fts(docs_fts, rowid, path, content, category)
                VALUES ('delete', old.rowid, old.path, old.content, old.category);
            END;
            CREATE TRIGGER IF NOT EXISTS docs_fts_update
            AFTER UPDATE ON documents BEGIN
                INSERT INTO docs_fts(docs_fts, rowid, path, content, category)
                VALUES ('delete', old.rowid, old.path, old.content, old.category);
                INSERT INTO docs_fts(rowid, path, content, category)
                VALUES (new.rowid, new.path, new.content, new.category);
            END;
        ")?;
        Ok(Self { conn: Mutex::new(conn) })
    }
}

#[async_trait]
impl Memory for SqliteMemory {
    async fn store(&self, path: &str, content: &str, category: MemoryCategory) -> ClawResult<MemoryDoc> {
        let cat_str = category_to_string(&category);
        let doc = MemoryDoc::new(path, content, category);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO documents (id, path, category, content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
               content    = excluded.content,
               category   = excluded.category,
               updated_at = excluded.updated_at,
               id         = excluded.id",
            params![
                &doc.id, &doc.path, &cat_str,
                &doc.content, doc.created_at as i64, doc.updated_at as i64
            ],
        )?;
        Ok(doc)
    }

    async fn recall(&self, query: &SearchQuery) -> ClawResult<Vec<SearchResult>> {
        // Escape user text as FTS5 phrase to prevent query injection
        // FTS5 phrase: wrap in double quotes, escaping internal quotes
        let fts_query = format!("\"{}\"", query.text.replace('"', "\"\""));
        let limit = if query.limit == 0 { 10 } else { query.limit };
        let conn = self.conn.lock().unwrap();
        let cat_filter = query.category.as_ref().map(category_to_string);

        let mut stmt = conn.prepare_cached(
            "SELECT d.id, d.path, d.category, d.content, d.created_at, d.updated_at,
                    bm25(docs_fts) AS score
             FROM docs_fts
             JOIN documents d ON d.rowid = docs_fts.rowid
             WHERE docs_fts MATCH ?1
               AND (?2 IS NULL OR d.category = ?2)
               AND (?3 IS NULL OR d.created_at >= ?3)
               AND (?4 IS NULL OR d.created_at <= ?4)
             ORDER BY score
             LIMIT ?5"
        )?;

        let rows = stmt.query_map(
            params![
                fts_query,
                cat_filter,
                query.since.map(|s| s as i64),
                query.until.map(|u| u as i64),
                limit as i64,
            ],
            |row| {
                let cat_str: String = row.get(2)?;
                Ok(SearchResult {
                    doc: MemoryDoc {
                        id: row.get(0)?,
                        path: row.get(1)?,
                        category: str_to_category(&cat_str),
                        content: row.get(3)?,
                        created_at: row.get::<_, i64>(4)? as u64,
                        updated_at: row.get::<_, i64>(5)? as u64,
                    },
                    score: -(row.get::<_, f64>(6)? as f32),
                })
            },
        )?;

        rows.collect::<Result<Vec<_>, _>>().map_err(ClawError::from)
    }

    async fn get(&self, path: &str) -> ClawResult<Option<MemoryDoc>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT id, path, category, content, created_at, updated_at FROM documents WHERE path = ?1"
        )?;
        let result = stmt.query_row(params![path], |row| {
            let cat_str: String = row.get(2)?;
            Ok(MemoryDoc {
                id: row.get(0)?,
                path: row.get(1)?,
                category: str_to_category(&cat_str),
                content: row.get(3)?,
                created_at: row.get::<_, i64>(4)? as u64,
                updated_at: row.get::<_, i64>(5)? as u64,
            })
        });
        match result {
            Ok(doc) => Ok(Some(doc)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn forget(&self, path: &str) -> ClawResult<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM documents WHERE path = ?1", params![path])?;
        Ok(n > 0)
    }

    async fn count(&self) -> ClawResult<usize> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))?;
        Ok(n as usize)
    }
}

impl SqliteMemory {
    /// Return all documents whose path starts with `prefix`, ordered by
    /// `created_at` ascending.
    ///
    /// Used to reconstruct a history prefix when count-based context pruning
    /// fires: paths are `history/{session_id}/{timestamp_hex}`, so querying
    /// `history/{session_id}/` returns all stored turn summaries for the
    /// session in chronological order.
    ///
    /// LIKE metacharacters (`%`, `_`, `\`) in `prefix` are automatically
    /// escaped before the trailing `%` wildcard is appended.
    pub async fn list_by_path_prefix(&self, prefix: &str) -> ClawResult<Vec<MemoryDoc>> {
        let conn = self.conn.lock().unwrap();
        // Escape LIKE metacharacters so the prefix is treated as a literal string
        let escaped = prefix
            .replace('\\', r"\\")
            .replace('%', r"\%")
            .replace('_', r"\_");
        let pattern = format!("{}%", escaped);
        let mut stmt = conn.prepare_cached(
            "SELECT id, path, category, content, created_at, updated_at
             FROM documents WHERE path LIKE ?1 ESCAPE '\\' ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            let cat_str: String = row.get(2)?;
            Ok(MemoryDoc {
                id: row.get(0)?,
                path: row.get(1)?,
                category: str_to_category(&cat_str),
                content: row.get(3)?,
                created_at: row.get::<_, i64>(4)? as u64,
                updated_at: row.get::<_, i64>(5)? as u64,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(ClawError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

        #[test]
        fn trigram_short_token_behavior() {
            // FTS5 trigram tokenizer requires >=3 chars per token.
            // Tokens shorter than 3 chars produce no trigrams and cause
            // phrase queries containing them to return 0 results.
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(
                "CREATE VIRTUAL TABLE t USING fts5(content, tokenize='trigram');
                 INSERT INTO t VALUES ('Early Optimization Turns 9 15 Lazy-load saves 320ms');
                 INSERT INTO t VALUES ('Startup Time Baseline Turn 9 15 two second acceptable');",
            ).unwrap();

            // Long words match fine
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM t WHERE t MATCH ?1", [r#""optimization""#], |r| r.get(0)
            ).unwrap();
            assert_eq!(n, 1, "single long word should match");

            // Short token "9" (1 char) alone → no trigrams → 0 results
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM t WHERE t MATCH ?1", [r#""9""#], |r| r.get(0)
            ).unwrap_or(0);
            assert_eq!(n, 0, "1-char token '9' should not match with trigram tokenizer");

            // Short token "15" (2 chars) alone → 0 results
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM t WHERE t MATCH ?1", [r#""15""#], |r| r.get(0)
            ).unwrap_or(0);
            assert_eq!(n, 0, "2-char token '15' should not match with trigram tokenizer");

            // Phrase containing short token → whole phrase fails to match
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM t WHERE t MATCH ?1",
                [r#""optimization startup performance early attempt turn 9 15""#],
                |r| r.get(0)
            ).unwrap_or(0);
            assert_eq!(n, 0, "phrase with short tokens produces no results");

            // Query without short tokens → works
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM t WHERE t MATCH ?1", [r#""optimization""#], |r| r.get(0)
            ).unwrap();
            assert!(n > 0, "phrase without short tokens should match");
        }

    // ── list_by_path_prefix ────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_by_path_prefix_returns_in_created_at_order() {
        let mem = SqliteMemory::open(":memory:").await.unwrap();
        // Store three docs under the same prefix with small sleeps to ensure
        // distinct created_at values (epoch seconds resolution).
        mem.store("history/sess/0000000a", "c1", MemoryCategory::Conversation).await.unwrap();
        mem.store("history/sess/0000000b", "c2", MemoryCategory::Conversation).await.unwrap();
        mem.store("history/sess/0000000c", "c3", MemoryCategory::Conversation).await.unwrap();
        let docs = mem.list_by_path_prefix("history/sess/").await.unwrap();
        assert_eq!(docs.len(), 3);
        assert_eq!(docs[0].path, "history/sess/0000000a");
        assert_eq!(docs[2].path, "history/sess/0000000c");
    }

    #[tokio::test]
    async fn list_by_path_prefix_excludes_other_prefixes() {
        let mem = SqliteMemory::open(":memory:").await.unwrap();
        mem.store("history/sess-a/001", "a", MemoryCategory::Conversation).await.unwrap();
        mem.store("history/sess-b/001", "b", MemoryCategory::Conversation).await.unwrap();
        let docs = mem.list_by_path_prefix("history/sess-a/").await.unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].path, "history/sess-a/001");
    }

    #[tokio::test]
    async fn list_by_path_prefix_empty_when_no_match() {
        let mem = SqliteMemory::open(":memory:").await.unwrap();
        mem.store("other/path", "x", MemoryCategory::Core).await.unwrap();
        let docs = mem.list_by_path_prefix("history/missing/").await.unwrap();
        assert!(docs.is_empty());
    }

    #[tokio::test]
    async fn list_by_path_prefix_escapes_like_metacharacters() {
        let mem = SqliteMemory::open(":memory:").await.unwrap();
        // Store a doc whose path contains a literal '%' — must not be treated
        // as a LIKE wildcard when used as the search prefix.
        mem.store("prefix_%_literal/doc", "v", MemoryCategory::Core).await.unwrap();
        mem.store("prefix_other/doc", "v2", MemoryCategory::Core).await.unwrap();
        // Searching for "prefix_%_literal/" must only return the first doc
        let docs = mem.list_by_path_prefix("prefix_%_literal/").await.unwrap();
        assert_eq!(docs.len(), 1, "metacharacters in prefix must be escaped");
    }

    proptest! {
        #[test]
        fn str_to_category_never_panics(s in ".*") {
            // Must not panic for any string input
            let _ = str_to_category(&s);
        }

        #[test]
        fn custom_prefix_strips_exactly_once(suffix in "[a-zA-Z0-9_]+") {
            let input = format!("custom:{}", suffix);
            let cat = str_to_category(&input);
            match cat {
                MemoryCategory::Custom(s) => prop_assert_eq!(s, suffix, "suffix must equal the part after 'custom:'"),
                _ => prop_assert!(false, "custom: prefix must produce Custom variant"),
            }
        }
    }
}
