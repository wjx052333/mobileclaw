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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

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
