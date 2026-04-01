# Memory Design: mobileclaw-core

## Overview

`SqliteMemory` provides persistent, full-text-searchable key-value document storage backed
by SQLite. It is the sole implementation of the `Memory` trait in MVP Phase 1. The design
prioritises:

- Low read latency on mobile (MMAP + WAL)
- CJK-capable substring search without an external search engine (FTS5 trigram tokenizer)
- Upsert semantics so callers need not distinguish insert from update

---

## SQLite Schema

### `documents` table

```sql
CREATE TABLE IF NOT EXISTS documents (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL UNIQUE,
    category    TEXT NOT NULL,
    content     TEXT NOT NULL,
    created_at  INTEGER NOT NULL,   -- Unix seconds (u64 stored as i64)
    updated_at  INTEGER NOT NULL
);
```

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT PK | Hex hash derived from path + content + nanosecond timestamp (see ID section) |
| `path` | TEXT UNIQUE | Logical document address; used as the upsert key |
| `category` | TEXT | Serialized `MemoryCategory` string (see category encoding section) |
| `content` | TEXT | Full document body |
| `created_at` | INTEGER | Unix epoch seconds, stored as `i64` in SQLite |
| `updated_at` | INTEGER | Updated on every upsert |

### `docs_fts` virtual table

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS docs_fts USING fts5(
    path, content, category,
    content='documents',
    content_rowid='rowid',
    tokenize='trigram'
);
```

- `content='documents'` — "content table" mode: FTS5 stores only the index, not the text.
  The actual content is read from `documents` by joining on `rowid`. This avoids data
  duplication.
- `content_rowid='rowid'` — links the FTS index row to `documents.rowid`.
- `tokenize='trigram'` — splits text into overlapping 3-character n-grams. This enables
  substring matching without word boundaries, supporting CJK scripts and partial-word
  queries that would fail with default tokenizers.

### Sync Triggers

Three triggers keep the FTS index consistent with the base table:

| Trigger | Event | Action |
|---------|-------|--------|
| `docs_fts_insert` | `AFTER INSERT ON documents` | Inserts new row into `docs_fts` |
| `docs_fts_delete` | `AFTER DELETE ON documents` | Deletes row from `docs_fts` using sentinel insert with `'delete'` command |
| `docs_fts_update` | `AFTER UPDATE ON documents` | Deletes old row, then inserts new row into `docs_fts` |

The "delete" sentinel syntax (`INSERT INTO docs_fts(docs_fts, rowid, ...) VALUES ('delete', ...)`)
is the FTS5 content table protocol for removing a row from the index.

---

## WAL + MMAP Rationale

Set at database open time via `execute_batch`:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA mmap_size = 67108864;   -- 64 MiB
PRAGMA cache_size = -4000;     -- 4 MiB page cache
```

| PRAGMA | Value | Rationale |
|--------|-------|-----------|
| `journal_mode = WAL` | WAL | Write-Ahead Log allows concurrent readers while a write transaction is in progress. On mobile, the agent loop may read memory while a write tool is executing. |
| `synchronous = NORMAL` | NORMAL | Syncs at WAL checkpoints only, not after every write. Reduces fsync system calls; acceptable durability for agent memory (not financial data). |
| `mmap_size = 67108864` | 64 MiB | Maps the database file into virtual memory. Read queries are served from OS page cache with zero-copy access. Beneficial for read-heavy memory search workloads. |
| `cache_size = -4000` | 4 MiB | SQLite page cache in addition to MMAP. Negative value = kibibytes. |

---

## FTS5 BM25 Scoring

The `recall` query uses `bm25(docs_fts)` as a relevance score:

```sql
SELECT ..., bm25(docs_fts) AS score
FROM docs_fts
JOIN documents d ON d.rowid = docs_fts.rowid
WHERE docs_fts MATCH ?1
ORDER BY score
LIMIT ?5
```

**BM25 polarity:** SQLite's `bm25()` returns a negative float where a more relevant
document has a more negative value (lower is better). The Rust layer negates this to
produce a positive score where higher is more relevant:

```rust
score: -(row.get::<_, f64>(6)? as f32),
```

Results are ordered `ORDER BY score` (ascending, i.e., most-negative first) which
corresponds to descending relevance.

**Trigram tokenizer and CJK:** The trigram tokenizer generates all 3-character substrings
of the indexed text. A query for `"async"` matches documents containing `"asy"`, `"syn"`,
`"ync"` — and by extension any document containing the substring `"async"`. Chinese text
`"代码审查"` is similarly tokenized into `"代码审"`, `"码审查"`, enabling substring search
across ideographic scripts without explicit word segmentation.

---

## Category String Encoding

`MemoryCategory` is stored as a plain string in the `category` column. The encoding
rules are:

| Rust variant | Stored string |
|--------------|---------------|
| `MemoryCategory::Core` | `"core"` |
| `MemoryCategory::Daily` | `"daily"` |
| `MemoryCategory::Conversation` | `"conversation"` |
| `MemoryCategory::Custom(s)` | `"custom:{s}"` (e.g., `"custom:project-notes"`) |

The prefix `"custom:"` ensures that custom category names cannot collide with the three
reserved names. Deserialization strips the prefix via `trim_start_matches("custom:")`.
An unrecognized string that does not match any reserved name and lacks the `"custom:"`
prefix is treated as `Custom(string)` with a tracing warning, providing forward
compatibility.

---

## MemoryDoc ID

`MemoryDoc::new` computes the `id` field as a hex-encoded `u64` hash:

```rust
let id = format!("{:x}", {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    content_str.hash(&mut h);
    secs.hash(&mut h);
    nanos.hash(&mut h);   // nanosecond timestamp for collision resistance
    h.finish()
});
```

- **Path + content** ensure two documents with different content at the same path get
  different IDs across upserts.
- **Nanosecond timestamp (`subsec_nanos`)** provides collision resistance when the same
  path and content are stored within the same second.
- The ID is informational; `path` is the authoritative upsert key (`ON CONFLICT(path)`).

---

## Upsert Semantics

`store()` uses `INSERT … ON CONFLICT(path) DO UPDATE SET …` to atomically insert or
update a document. The `id` is also updated on conflict so it reflects the latest version.
The `created_at` field is preserved from the original insert (not overwritten) by the
fact that `created_at` is not listed in the `DO UPDATE SET` clause — wait, actually the
current implementation does update `id` but not `created_at` because `created_at` is
not included in the update list. `updated_at` is always set to the new timestamp.

---

## Future Extension: Vector Embeddings

When semantic (embedding-based) search is needed, `fastembed-rs` can be integrated with
minimal schema change:

1. Add `embedding BLOB` column to `documents`.
2. Store quantized float32 vectors as raw bytes.
3. Implement a cosine similarity scan or integrate `sqlite-vec` extension for ANN search.

The `Memory` trait interface (`store`, `recall`, `get`, `forget`, `count`) does not need
to change; the `recall` implementation can be extended to fuse BM25 and vector scores.
