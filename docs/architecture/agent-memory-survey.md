# Agent Memory Survey: ironclaw / nanoclaw / zeroclaw

Survey date: 2026-04-02

Sources: `~/agent_eyes/thirdparty/bot/{ironclaw,nanoclaw,zeroclaw}/src/`

---

## 1. ironclaw (Rust + PostgreSQL)

### Session / Conversation History

**Architecture**: hierarchical — Session → Thread → Turn

```
Session (groups threads for one user)
  └── Thread (one conversation channel)
        └── Turn (single user input + response + tool calls)
```

Key files:
- `src/agent/session.rs` (~2041 lines) — Session/Thread/Turn structs
- `src/agent/session_manager.rs` — multi-user session coordination
- `src/context/memory.rs` — in-memory circular buffer
- `src/history/store.rs` — persistence

**In-memory layer** (`ConversationMemory`):
- Circular buffer, default max 100 messages
- Auto-trims oldest when capacity exceeded; system messages are never trimmed
- Sliding-window semantics

**Persistence layer**:
- PostgreSQL primary; libSQL (Turso-compatible embedded) optional
- Schema: `conversations` (id, channel, user_id, thread_id) + `conversation_messages` (id, conversation_id, role, content)
- Migrations managed by refinery; connection pool via deadpool

**Pending message queue**: each Thread holds a `VecDeque` for rapid user inputs arriving
while processing; hard cap `MAX_PENDING_MESSAGES = 10`.

**Turn structure** (notable fields):
```rust
pub struct Turn {
    pub turn_number: usize,
    pub user_input: String,
    pub response: Option<String>,
    pub tool_calls: Vec<TurnToolCall>,
    pub narrative: Option<String>,   // agent reasoning
    pub image_content_parts: Vec<ContentPart>,  // cleared after LLM call
}
```

### Long-Term Memory (Workspace System)

- `src/workspace/mod.rs` (~83 KB) — primary
- **Vector store**: pgvector column in PostgreSQL; configurable embedding provider (OpenAI etc.)
- **FTS**: PostgreSQL tsvector columns, BM25 keyword scoring
- **Hybrid search**: fuses vector similarity + BM25, configurable weights
- Documents split into `MemoryChunk` objects with embeddings; metadata includes timestamps, doc type, privacy classification
- `src/workspace/privacy.rs` — PII redaction before storage
- `src/workspace/hygiene.rs` — deduplication and cleanup
- Schema tables: `memory_documents`, `memory_chunks` (with embedding column)

### Context Window Management

| Mechanism | Detail |
|-----------|--------|
| In-memory cap | 100 messages (circular buffer, configurable) |
| Turn truncation | `Thread::truncate_turns(keep: usize)` — keeps N most recent turns, re-indexes turn numbers |
| Tool result cap | Individual results truncated to 1000 chars |
| Compaction | `/compact` command referenced; explicit strategy not visible in source |

**Verdict**: sliding-window hard delete by turn; no summarization in code examined.
Oldest turns are discarded without summary — same category as mobileclaw's current prune.

---

## 2. nanoclaw (TypeScript + SQLite)

### Session / Conversation History

**Architecture**: flat — Chat (identified by JID) → Messages

Key files:
- `src/db.ts` (~500 lines) — schema + all queries
- `src/types.ts` — NewMessage, ScheduledTask types

No in-memory conversation buffer; all history lives in SQLite.

**Schema**:
```sql
CREATE TABLE chats (
  jid TEXT PRIMARY KEY, name TEXT, last_message_time TEXT,
  channel TEXT, is_group INTEGER
);
CREATE TABLE messages (
  id TEXT, chat_jid TEXT, sender TEXT, sender_name TEXT,
  content TEXT, timestamp TEXT, is_from_me INTEGER,
  is_bot_message INTEGER DEFAULT 0,
  PRIMARY KEY (id, chat_jid)
);
CREATE INDEX idx_timestamp ON messages(timestamp);
```

No WAL mode configured; synchronous writes; single connection (no pool).

### Long-Term Memory

None. No vector embeddings, no semantic search, no fact extraction.
The only persistence is the raw message log.

Scheduled tasks stored in a separate `scheduled_tasks` table with execution logs
(`task_run_logs`) — unrelated to memory.

### Context Window Management

Hard message limit via SQL:
```sql
SELECT * FROM (
  SELECT ... FROM messages
  WHERE chat_jid = ? AND timestamp > ?
    AND is_bot_message = 0 AND content NOT LIKE ?
  ORDER BY timestamp DESC LIMIT 200
) ORDER BY timestamp
```

- `getMessagesSince(chatJid, sinceTimestamp, limit=200)` — 200 messages hard cap
- Cursor-based recovery: tracks `lastTimestamp` to skip already-processed messages
- Bot messages filtered by `is_bot_message = 1` flag and content prefix `{ASSISTANT_NAME}:`
- No summarization, no compaction — pure FIFO eviction at query time

**Verdict**: simplest design; oldest messages silently drop out of context window.
No knowledge of what was pruned.

---

## 3. zeroclaw (Rust + SQLite, pluggable backends)

### Session / Conversation History

**Architecture**: per-session message store; session identified by composite key (user + channel)

Key files:
- `src/channels/session_sqlite.rs` — primary SQLite session store (with JSONL migration)
- `src/channels/session_postgres.rs` — optional PostgreSQL backend

**Schema** (SQLite):
```sql
CREATE TABLE sessions (
  id INTEGER PRIMARY KEY, session_key TEXT, role TEXT,
  content TEXT, created_at TEXT
);
CREATE VIRTUAL TABLE sessions_fts USING fts5(
  session_key, content, content=sessions, content_rowid=id
);
CREATE TABLE session_metadata (
  session_key TEXT PRIMARY KEY, created_at TEXT,
  last_activity TEXT, message_count INTEGER
);
```

PRAGMA config: `journal_mode=WAL`, `synchronous=NORMAL`, `mmap_size=8MB`, `cache_size=-2000`,
`temp_store=MEMORY` — production-tuned, same philosophy as mobileclaw.

**JSONL migration**: auto-detects legacy `.jsonl` files, imports to SQLite, renames to
`.jsonl.migrated`. Clean upgrade path.

Workspace layout: `{workspace}/sessions/sessions.db` (session) + `{workspace}/memory/brain.db`
(long-term memory) — two separate databases, clear separation of concerns.

### Long-Term Memory

**Architecture**: hybrid vector + keyword search, LLM-driven consolidation

Key files:
- `src/memory/sqlite.rs` — primary SQLite backend
- `src/memory/consolidation.rs` — LLM-driven consolidation (~150 lines)
- `src/memory/traits.rs` — `Memory` trait (same pattern as mobileclaw)
- `src/agent/memory_loader.rs` — context injection for LLM prompts
- `src/tools/memory_store.rs` — user-facing `memory_store` tool
- `src/tools/memory_recall.rs` — user-facing `memory_recall` tool

Optional backends (feature flags):
- `memory-postgres` — PostgreSQL + pgvector
- `memory-qdrant` — Qdrant vector DB
- `src/memory/markdown.rs` — plain Markdown files (debug/fallback)
- `src/memory/mem0.rs` — Mem0 REST API

**Vector storage**: embeddings stored as BLOB in SQLite; cosine similarity computed in-process.
LRU embedding cache (configurable size) to avoid redundant API calls.

**Memory categories**:
- `Core` — permanent facts and preferences
- `Daily` — session notes; auto-created with `daily_{date}` key prefix
- `Conversation` — chat context
- `Custom(String)` — user-defined

**Hybrid search**: configurable `vector_weight` and `keyword_weight`; BM25 via FTS5; results
merged and re-ranked by weighted fusion score.

**Knowledge graph** (`src/memory/knowledge_graph.rs`): optional structured memory extraction.

### Context Window Management — The Consolidation Engine

This is zeroclaw's most distinctive feature. Two-phase LLM consolidation per turn:

```
Phase 1: history entry
  Input: current turn (user + assistant, capped at 4000 chars)
  Output: one-sentence timestamped history entry
  → written to memory store under a session-scoped key

Phase 2: fact extraction (optional)
  Input: same turn
  Output: null  OR  new factual claim worth remembering long-term
  → written to Core memory if non-null
```

System prompt instructs the LLM to emit JSON:
```json
{"history_entry": "User asked about X; assistant explained Y", "memory_update": null}
```
or
```json
{"history_entry": "...", "memory_update": "User prefers dark mode"}
```

Fallback on malformed JSON: use truncated turn text as history entry.

**Memory loader** (`agent/memory_loader.rs`) at query time:
```rust
pub fn load_context(&self, memory: &dyn Memory, user_message: &str,
                    session_id: Option<&str>) -> Result<String> {
    let entries = memory.recall(user_message, self.limit, session_id, None, None)?;
    // filter by min_relevance_score (default 0.4), skip autosave keys
    // format as "[Memory context]\n- key: content\n..."
}
```

Context is injected into the system prompt before each LLM call — the model sees retrieved
memory as part of system context, not conversation history.

**Response cache** (`src/response_cache.rs`): optional LRU cache of LLM responses to avoid
re-running identical prompts (useful in group chat scenarios with repeated queries).

**Verdict**: most sophisticated design. Session history is persisted per-turn; long-term
memory is populated automatically via consolidation. The model never "forgets" — old turns
become distilled facts rather than being silently dropped.

---

## 4. Comparative Summary

| Dimension | ironclaw | nanoclaw | zeroclaw |
|-----------|----------|----------|----------|
| **Session storage** | PostgreSQL (primary), libSQL | SQLite | SQLite (WAL) |
| **Session model** | Session→Thread→Turn hierarchy | Flat chat+messages | Per-session key, flat messages |
| **In-memory buffer** | Circular buffer 100 msgs | None | None |
| **Long-term memory** | pgvector + PostgreSQL FTS | None | SQLite BLOB embeddings + FTS5 |
| **Semantic search** | Hybrid vector + BM25 | None | Hybrid vector + BM25 |
| **Context window strategy** | Sliding window (turn truncation) | Hard limit 200 msgs, FIFO | Consolidation → relevance recall |
| **When old context is pruned** | Hard delete, oldest turns first | Silent drop from SQL LIMIT | Distilled into memory before drop |
| **LLM call on prune** | None | None | Yes (Haiku-level, per turn) |
| **Tool result handling** | Truncate to 1000 chars | Filtered at query level | Via memory loader threshold |
| **Embedding storage** | pgvector column | None | BLOB in SQLite |
| **Embedding cache** | Not visible | N/A | LRU cache (configurable) |
| **Optional backends** | libSQL | None | Postgres, Qdrant, Markdown, Mem0 |
| **Concurrency model** | deadpool connection pool | Single connection | `Arc<Mutex<Connection>>` |
| **Privacy/hygiene** | PII redaction + dedup engine | None | None visible |
| **Knowledge graph** | Not visible | None | Optional (`knowledge_graph.rs`) |

---

## 5. Design Patterns Worth Adopting

### From zeroclaw: per-turn consolidation

Most directly applicable to mobileclaw. The key insight: **don't wait until context overflow
to summarize**. Summarize every turn into a compressed history entry as it happens. When the
context window eventually fills:

- Session memory already contains a distilled record of all past turns
- Compact operation just slices to recent N messages + prepends session memory
- No extra LLM call needed at compact time (cost was paid incrementally)

This is essentially claude-code's Session Memory Compact path, independently arrived at.
Approximate cost: one cheap LLM call per conversation turn; total is comparable to one
expensive compact call, but spread evenly and never blocking.

### From ironclaw: separate session DB from memory DB

zeroclaw does this too. `sessions.db` (raw message log) and `brain.db` (long-term memory)
are separate files. Advantages:
- Can purge/rotate sessions without touching long-term memory
- Different VACUUM and backup strategies per database
- Session DB grows and shrinks; memory DB grows monotonically

mobileclaw currently uses a single `memory.db` for everything. Worth splitting when
session persistence is activated.

### From nanoclaw: explicit `is_bot_message` flag

Simple but effective. Filtering assistant messages from the "context shown to LLM" query
is cleaner than relying on role-based filtering. Nanoclaw stores all messages (including
bot responses) in a single table but marks them — makes bulk analytics and filtering easy.

### From ironclaw: tool result truncation at ingestion

Truncating tool results to 1000 chars at the point they are stored (not at the point they
are retrieved) keeps the database lean and prevents a single large tool result from
dominating the context window in a later session.

---

## 6. Gap vs mobileclaw (actionable)

| Gap | Severity | Effort | Design source |
|-----|----------|--------|---------------|
| Per-turn consolidation to memory | High | Medium | zeroclaw consolidation.rs |
| Separate sessions.db from memory.db | Medium | Low | zeroclaw workspace layout |
| Embedding cache (LRU) to avoid re-embedding | Medium | Low | zeroclaw memory/sqlite.rs |
| Tool result truncation at ingestion | Low | Low | ironclaw context/memory.rs |
| Session FTS search (find past turns by keyword) | Low | Low | zeroclaw sessions_fts table |
