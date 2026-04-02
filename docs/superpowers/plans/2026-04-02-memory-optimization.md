# Memory Optimization — Implementation Plan (Rust Core)

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent unbounded context growth, persist sessions across restarts, and align memory taxonomy with claude-code standards — while keeping RSS under 20 MB in CLI usage.

**Guiding principles:**

> **极致性能和安全约束是项目成功的基石。**
> - No `unwrap()` without safety comment
> - Zero copies on hot paths (clone requires justification)
> - Token estimation must be O(N) over message content, never call any API
> - Session files written with atomic rename (`.tmp` → final) to prevent corruption on crash
> - All path inputs validated against sandbox; no writes outside user's app sandbox

**Status Quo:** `AgentLoop::chat()` appends unlimited `Vec<Message>` to `history`. No token counting, no disk persistence, `MemoryCategory` uses internal names (Core/Daily/Conversation) diverging from claude-code taxonomy (user/feedback/project/reference).

**Reference:** `docs/cc-evaluation/findings.md`, `docs/architecture/memory-management-analysis.md`, `src/services/compact/autoCompact.ts` in claude-code.

**Tech Stack:** tokio, serde, serde_json, rusqlite, tempfile (tests), wiremock (probe tests).

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `mobileclaw-core/src/agent/token_counter.rs` | Token estimation from message text (4 bytes/token rule) |
| Create | `mobileclaw-core/src/agent/context_manager.rs` | History pruning, compact strategies, threshold enforcement |
| Create | `mobileclaw-core/src/agent/session.rs` | JSONL session persistence, load/resume, stale session cleanup |
| Modify | `mobileclaw-core/src/agent/loop_impl.rs` | Integrate token counter + context manager into chat loop |
| Modify | `mobileclaw-core/src/memory/types.rs` | Category rename: Core→Project, Daily→Daily (keep), Conversation→Conversation (keep), add User, Feedback, Reference aliases; backward-compatible serde |
| Modify | `mobileclaw-core/src/ffi.rs` | Add session management FFI: `session_save()`, `session_load()`, `session_list()`, `session_delete()` |
| Modify | `mobileclaw-core/src/ffi.rs` | Add `session_dir`, `context_window` optional fields to `AgentConfig` |

---

## Task 1: Token Estimator — O(1) dependency, no async

**Files:**
- Create: `mobileclaw-core/src/agent/token_counter.rs`

- [ ] **Step 1.1: Write failing test first**

```rust
// mobileclaw-core/src/agent/token_counter.rs  (bottom #[cfg(test)] mod tests)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_ascii_message() {
        // "hello world" = 11 bytes + JSON overhead ≈ ~8 tokens (11/4 ≈ 3, ceil to integer)
        let msg = Message::user("hello world");
        let tokens = estimate_message_tokens(&msg);
        assert!(tokens >= 3, "minimum tokens for 11-char ASCII, got {tokens}");
    }

    #[test]
    fn estimate_tokens_empty_message() {
        assert_eq!(estimate_message_tokens(&Message::user("")), 0, "empty message must be zero tokens");
    }

    #[test]
    fn estimate_tool_result_xml_overhead() {
        // Assistant messages containing tool XML should count the XML text too
        let msg = Message::assistant("text<tool_result name=\"x\" status=\"ok\">val</tool_result>");
        let tokens = estimate_message_tokens(&msg);
        assert!(tokens >= 20, "tool result XML has overhead, got {tokens}");
    }
}
```

- [ ] **Step 1.2: Implement `estimate_message_tokens(&Message) -> usize`**

  ```rust
  /// Estimate token count for a single message using the 4-bytes-per-token rule
  /// (matching claude-code's tokenEstimation.ts behavior).
  /// Content is measured as bytes, divided by 4, rounded up.
  /// System/structural overhead: +3 tokens for role tag, +1 per content block.
  pub fn estimate_message_tokens(msg: &Message) -> usize {
      let text_bytes: usize = msg.content.iter().map(|b| match b {
          ContentBlock::Text { text } => text.as_bytes().len(),
      }).sum();
      let overhead = 3 /* role */ + msg.content.len() /* per-block */;
      if text_bytes == 0 { return overhead; }
      overhead + (text_bytes + 3) / 4  // ceil division
  }
  ```

- [ ] **Step 1.3: Implement `estimate_tokens(messages: &[Message]) -> usize`**

  ```rust
  /// Sum of `estimate_message_tokens` over all messages. O(N) scan, zero allocations.
  pub fn estimate_tokens(messages: &[Message]) -> usize {
      messages.iter().map(estimate_message_tokens).sum()
  }
  ```

- [ ] **Step 1.4: Add export in `mod.rs` or `lib.rs`**

  Export from `agent/mod.rs` (if exists) via `pub mod token_counter;` or at crate root.

---

## Task 2: Context Manager — history pruning + compact

**Files:**
- Create: `mobileclaw-core/src/agent/context_manager.rs`

- [ ] **Step 2.1: Write failing tests first**

```rust
// mobileclaw-core/src/agent/context_manager.rs  (bottom #[cfg(test)] mod tests)

#[test]
fn prune_oldest_keeps_system_and_latest() {
    let mut msgs = vec![sys("A"), user("1"), assistant("r1"), user("2"), assistant("r2"), user("3"), assistant("r3")];
    let original = msgs.clone();
    prune_oldest_messages(&mut msgs, 80, estimate_tokens(&original)).unwrap();
    // System message always preserved
    assert!(msgs.iter().any(|m| m.role == Role::System && m.text_content() == "A"));
    // Oldest user messages removed
    assert!(!msgs.iter().any(|m| m.role == Role::User && m.text_content() == "1"));
}

#[test]
fn prune_noop_when_under_threshold() {
    let mut msgs = vec![sys("sys"), user("hi")];
    prune_oldest_messages(&mut msgs, 80, estimate_tokens(&msgs)).unwrap();
    assert_eq!(msgs.len(), 2, "should not prune when under threshold");
}

#[test]
fn prune_preserves_at_least_minimum_user_turns() {
    let mut msgs = vec![sys("s"), user("1")];
    for i in 2..30 { msgs.push(user(&format!("{i}"))); }
    prune_oldest_messages(&mut msgs, 80, 1000).unwrap();  // well over 80
    assert!(msgs.iter().filter(|m| m.role == Role::User).count() >= 3,
        "must preserve at least 3 user turns even under extreme pruning");
}
```

- [ ] **Step 2.2: Implement `ContextConfig` struct**

  ```rust
  #[derive(Debug, Clone)]
  pub struct ContextConfig {
      /// Maximum tokens allowed in history (default: 200_000 for Claude Sonnet 4.6)
      pub max_tokens: usize,
      /// Buffer tokens to keep before limit (claude-code default: 13_000)
      pub buffer_tokens: usize,
      /// Minimum user turns to always preserve (at least last 3 turns)
      pub min_user_turns: usize,
  }

  impl Default for ContextConfig {
      fn default() -> Self {
          Self { max_tokens: 200_000, buffer_tokens: 13_000, min_user_turns: 3 }
      }
  }
  ```

- [ ] **Step 2.3: Implement `prune_oldest_messages(&mut Vec<Message>, threshold: usize, current_tokens: usize) -> Result<usize>`**

  Algorithm:
  1. If `current_tokens <= threshold`, return Ok(0) — nothing to do
  2. Identify messages eligible for removal: system messages NEVER, last 3 user turns NEVER, last assistant turn NEVER
  3. Remove eligible messages from oldest first until `current_tokens <= threshold` or no more eligible
  4. Return count of pruned messages
  5. Never return an empty message history (keep at least system + last user turn)

  Design constraint: zero allocation of intermediate Vec — perform in-place with `swap_remove` or `drain_filter`.

- [ ] **Step 2.4: Export in module**

---

## Task 3: Session Persistence (JSONL transcript + resume)

**Files:**
- Create: `mobileclaw-core/src/agent/session.rs`

- [ ] **Step 3.1: Write failing tests first**

```rust
// mobileclaw-core/src/agent/session.rs  (bottom #[cfg(test)] mod tests)

#[tokio::test]
async fn save_and_load_session_round_trip() {
    let dir = TempDir::new().unwrap();
    let msgs = vec![Message::user("hello"), Message::assistant("hi!")];
    let session_path = save_session(&dir.path(), &msgs).await.unwrap();
    let loaded = load_session(&session_path).await.unwrap();
    assert_eq!(loaded.len(), msgs.len());
    assert_eq!(loaded[0].role, msgs[0].role);
    assert_eq!(loaded[1].text_content(), "hi!");
}

#[tokio::test]
async fn save_session_atomic_rename() {
    let dir = TempDir::new().unwrap();
    save_session(&dir.path(), &vec![Message::user("x")]).await.unwrap();
    // Verify no .tmp files leaked
    let mut entries = std::fs::read_dir(&dir).unwrap();
    assert_eq!(entries.count(), 1, "only final file, no .tmp");
    assert!(entries.next().unwrap().unwrap().file_name().to_str().unwrap().ends_with(".jsonl"));
}

#[tokio::test]
async fn list_sessions_returns_sorted() {
    let dir = TempDir::new().unwrap();
    let msgs = vec![Message::user("a")];
    save_session(dir.path(), &msgs).await.unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    save_session(dir.path(), &msgs).await.unwrap();
    let sessions = list_sessions(dir.path()).await.unwrap();
    assert_eq!(sessions.len(), 2);
    // newest first
    assert!(sessions[0].modified >= sessions[1].modified);
}
```

- [ ] **Step 3.2: `SessionEntry` DTO**

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct SessionEntry {
      pub id: String,          // session_<timestamp>_<hex>
      pub modified: u64,       // file modification time
      pub message_count: usize,
      pub file_path: String,
  }
  ```

- [ ] **Step 3.3: `save_session()` — atomic write**

  ```rust
  /// Write messages to a JSONL session file.
  /// Atomic: writes to .tmp, syncs file, renames to final name.
  /// Returns the final file path.
  pub async fn save_session(dir: &Path, messages: &[Message]) -> ClawResult<PathBuf> {
      // Path safety: dir must be absolute and exist
      if !dir.is_absolute() { return Err(ClawError::SessionPathNotAbsolute); }
      if !dir.exists() { tokio::fs::create_dir_all(dir).await?; }

      let id = format!("session_{}", timestamp_hex());
      let tmp = dir.join(format!("{}.tmp", id));
      let file_path = dir.join(format!("{}.jsonl", id));

      let mut f = tokio::fs::File::create(&tmp).await?;
      for msg in messages {
          let line = serde_json::to_string(msg)?;
          use tokio::io::AsyncWriteExt;
          f.write_all(line.as_bytes()).await?;
          f.write_all(b"\n").await?;
      }
      f.sync_all().await?;
      // Atomic rename
      tokio::fs::rename(&tmp, &file_path).await?;
      Ok(file_path)
  }
  ```

- [ ] **Step 3.4: `load_session()` — read JSONL**

  ```rust
  pub async fn load_session(file_path: &Path) -> ClawResult<Vec<Message>> {
      if !file_path.exists() { return Err(ClawError::SessionNotFound); }
      let content = tokio::fs::read_to_string(file_path).await?;
      let mut messages = Vec::new();
      for line in content.lines() {
          if line.is_empty() { continue; }
          messages.push(serde_json::from_str::<Message>(line)?);
      }
      Ok(messages)
  }
  ```

- [ ] **Step 3.5: `list_sessions()` — scan directory**

  ```rust
  pub async fn list_sessions(dir: &Path) -> ClawResult<Vec<SessionEntry>> {
      // Scan for *.jsonl files, read metadata, sort by modified desc
  }
  ```

- [ ] **Step 3.6: `delete_session()`**

  ```rust
  pub async fn delete_session(file_path: &Path) -> ClawResult<bool> {
      // Path validation: must be under session_dir (no traversal)
      // Remove file, return true if existed
  }
  ```

---

## Task 4: AgentLoop Integration

**Files:**
- Modify: `mobileclaw-core/src/agent/loop_impl.rs`

- [ ] **Step 4.1: Add `ContextConfig` and `session_dir` fields to `AgentLoop`**

  ```rust
  pub struct AgentLoop<L: LlmClient> {
      // existing fields...
      ctx_config: ContextConfig,        // NEW: configurable context window
      session_dir: Option<PathBuf>,     // NEW: optional session persistence
  }
  ```

  Update `AgentLoop::new()` to accept `ContextConfig` (or use Default).

- [ ] **Step 4.2: Inject context pruning before LLM call**

  In `chat()`, after `self.history.push(Message::user(...))`, before `self.llm.stream_messages()`:

  ```rust
  let current_tokens = estimate_tokens(&self.history);
  let threshold = self.ctx_config.max_tokens.saturating_sub(self.ctx_config.buffer_tokens);
  if current_tokens > threshold {
      let pruned = prune_oldest_messages(&mut self.history, threshold, current_tokens)?;
      tracing::info!(pruned, tokens_before = current_tokens, tokens_after = estimate_tokens(&self.history), "context pruned");
  }
  ```

- [ ] **Step 4.3: Optional session save on chat completion**

  At end of `chat()` (before returning events), if `session_dir` is Some:

  ```rust
  if let Some(ref dir) = self.session_dir {
      if let Err(e) = crate::agent::session::save_session(dir, &self.history).await {
          tracing::warn!(error = %e, "failed to save session transcript");
      }
  }
  ```

  Non-fatal: session save failure must not fail the user-facing chat.

---

## Task 5: Memory Category Alignment

**Files:**
- Modify: `mobileclaw-core/src/memory/types.rs`

- [ ] **Step 5.1: Add claude-code taxonomy as serde aliases while preserving backward compat**

  Our existing:
  ```
  Core          → maps to claude-code: project
  Daily         → keep (matches claude-code daily logs)
  Conversation  → keep (maps loosely to claude-code: user/feedback context)
  Custom(s)     → keep
  ```

  New enum with aliases:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
  #[serde(rename_all = "lowercase")]
  pub enum MemoryCategory {
      #[serde(rename = "project", alias = "core")]
      Project,   // was Core
      Daily,
      #[serde(alias = "daily")]
      Daily,
      Conversation,
      #[serde(alias = "user")]
      User,
      #[serde(alias = "feedback")]
      Feedback,
      #[serde(alias = "reference")]
      Reference,
      Custom(String),
  }
  ```

  Actually — simpler and safer: keep Core as-is but add `alias = "project"` so existing DB entries deserialize. Add User, Feedback, Reference as new variants. No rename needed:

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
  #[serde(rename_all = "lowercase")]
  pub enum MemoryCategory {
      Core,              // alias "project" for claude-code compat
      Daily,             // kept
      Conversation,      // kept
      User,              // NEW
      Feedback,          // NEW
      Reference,         // NEW
      Custom(String),    // kept
  }
  ```

  With alias: `#[serde(rename = "project", alias = "core")]` — BUT this changes the serialize output. Since Core is already written to DB, we should NOT change the serialized name. Instead, add `#[serde(alias = "project")]` beside existing `#[serde(rename_all = "lowercase")]`:

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
  #[serde(rename_all = "lowercase")]
  pub enum MemoryCategory {
      #[serde(alias = "project")]
      Core,
      Daily,
      Conversation,
      User,
      Feedback,
      Reference,
      #[serde(rename = "custom")]
      Custom(String),
  }
  ```

- [ ] **Step 5.2: Add category_to_str() helper for FFI display**

  ```rust
  pub fn category_to_str(c: &MemoryCategory) -> &'static str {
      match c {
          MemoryCategory::Core => "project",  // display as claude-code name
          MemoryCategory::Daily => "daily",
          MemoryCategory::Conversation => "conversation",
          MemoryCategory::User => "user",
          MemoryCategory::Feedback => "feedback",
          MemoryCategory::Reference => "reference",
          MemoryCategory::Custom(s) => s,
      }
  }
  ```

  This ensures FFI DTOs use claude-code names while internal DB keeps existing values (backward compat via alias).

---

## Task 6: FFI Additions

**Files:**
- Modify: `mobileclaw-core/src/ffi.rs`

- [ ] **Step 6.1: `AgentConfig` optional fields**

  ```rust
  pub struct AgentConfig {
      // existing fields...
      pub session_dir: Option<String>,         // NEW: persist sessions here
      pub context_window: Option<u32>,          // NEW: max context tokens (default 200K)
  }
  ```

- [ ] **Step 6.2: Session FFI methods on AgentSession**

  ```rust
  /// Persist current session history to disk.
  pub async fn session_save(&mut self) -> anyhow::Result<String>;   // returns file path

  /// Load a session from a saved file path. Returns message count.
  pub async fn session_load(&mut self, file_path: String) -> anyhow::Result<usize>;

  /// List saved sessions. Returns vec of session summaries.
  pub fn session_list(&self) -> anyhow::Result<Vec<SessionEntryDto>>;

  /// Delete a session file. Path is validated against session_dir.
  pub async fn session_delete(&self, file_path: String) -> anyhow::Result<bool>;
  ```

- [ ] **Step 6.3: FRB glue**

  Run `cargo build` — flutter_rust_bridge codegen auto-updates from the struct changes.
  If FRB is not in use for codegen in this branch, manually update `frb_generated.rs`.

---

## Task 7: frb_generated.rs Update

**Files:**
- Modify: `mobileclaw-core/src/frb_generated.rs`

frb_generated.rs is auto-generated. After AgentConfig changes, rebuild:

```bash
cargo build -p mobileclaw-core
```

If it fails with missing fields, manually add the decode/encode for `session_dir` and `context_window` following the existing pattern for `log_dir`.

---

## Verification

**Run all tests:**
```bash
cargo test -p mobileclaw-core --features test-utils
```

**Run clippy:**
```bash
cargo clippy -p mobileclaw-core --features test-utils -- -D warnings
```

**Coverage check:**
```bash
cargo llvm-cov --package mobileclaw-core --features test-utils --all-targets --fail-under-lines 85
```

**CLI smoke test:**
```bash
cargo build && RUST_LOG=mclaw=debug ./target/debug/mclaw chat
```

---

## Task Dependencies

```
Task 1 (token_counter)  →  Task 2 (context_manager)  →  Task 4 (loop integration)
Task 3 (session)        →  Task 4 (loop integration)
Task 5 (categories)     →  independent
Task 6 (FFI)            →  Task 4, Task 5
Task 7 (frb)            →  Task 6
```

Recommended execution: parallel agents on Task 1+3+5, then Task 2, then Task 4, then Task 6+7.
