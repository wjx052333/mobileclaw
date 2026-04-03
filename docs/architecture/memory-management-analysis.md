# Memory Management Analysis: claude-code vs mobileclaw

## Source

Study of `claude-code/src/memdir/`, `src/services/compact/`, `src/services/tokenEstimation.ts`,
`src/utils/tokens.ts`, `src/utils/sessionRestore.ts`, and `docs/cc-evaluation/findings.md`.

Last updated: 2026-04-02 — corrected session persistence status, updated memory taxonomy,
added three-layer compaction comparison.

---

## 1. Auto Compaction & Token Counting

### claude-code Implementation

**Token counting** (`utils/tokens.ts:46-107`):

```typescript
// Exact count from last API response
getTokenCountFromUsage(usage):
  input_tokens + cache_creation_input + cache_read_input + output_tokens

// Estimation for new messages added since last API call
tokenCountWithEstimation(messages):
  = lastAPIResponse exact count + rough estimate (4 bytes/token default, 2 bytes/token for JSON)
```

**Threshold system** (`services/compact/autoCompact.ts:62-91`):

| Constant | Value | Purpose |
|----------|-------|---------|
| `MODEL_CONTEXT_WINDOW_DEFAULT` | 200,000 | Default context window |
| `MAX_OUTPUT_TOKENS_FOR_SUMMARY` | 20,000 | Reserved for compact output |
| `AUTOCOMPACT_BUFFER_TOKENS` | 13,000 | Buffer before context runs out |
| Compaction triggers at | ~167K tokens | effectiveWindow - 13K buffer |

**Two-tier compaction** (tried in order):
1. **Session Memory Compact** (experimental): prunes old messages, preserves 10K-40K tokens
2. **Legacy Conversation Compact**: LLM-generated full conversation summary

**Circuit Breaker**: stops after 3 consecutive compaction failures (BQ data: unguarded sessions waste ~250K API calls/day with 50+ consecutive failures).

### mobileclaw current state

- **Token estimation implemented** (`src/agent/token_counter.rs`): 4 bytes/token rule, same as claude-code's `roughTokenCountEstimation()` default; O(N), zero API calls
- **Threshold-based pruning implemented** (`src/agent/context_manager.rs`): default max 200K tokens, 13K buffer → prune threshold 187K, matching claude-code constants
- **Pruning strategy: delete oldest messages** — removes oldest non-protected messages in-place until under threshold; protects system messages, last assistant turn, last 3 user turns
- `MAX_TOOL_ROUNDS = 10` still limits tool iteration rounds per chat call (unchanged)

**Critical gap vs claude-code**: mobileclaw prune is **destructive** — deleted messages are gone permanently. claude-code never deletes raw messages; it first distills them into a summary, then discards the raw text. See Section 6 for the three-layer compaction comparison.

---

## 2. Session Continuity

### claude-code Implementation

**JSONL transcript** per session, stored to disk. Resume flow:

```
1. Load transcript messages from JSONL file
2. Restore session ID (unless --fork-session)
3. Restore state:
   - File history snapshots
   - Attribution state (who changed which files)
   - TodoWrite state (scanned from last assistant message)
   - Worktree state
4. Reset system prompt cache
```

### mobileclaw current state

**Implemented** (`src/agent/session.rs`): JSONL atomic write (`.tmp` → `fsync` → `rename`),
load/resume, list/delete. Auto-save wired in `src/agent/loop_impl.rs` after each `chat()` call.

**Activation gap**: `session_dir` defaults to `None` in both CLI (`mobileclaw-cli/src/session.rs`)
and the current Flutter config. Session persistence only activates when the caller passes a
non-None absolute path in `AgentConfig.session_dir`.

**Android force-kill behavior**:
- If `session_dir` is set: each `chat()` call atomically writes a full JSONL snapshot. Force-kill
  loses at most the in-progress turn. On restart, Flutter calls `session_list()` + `session_load()`
  to restore. **This scenario is covered.**
- If `session_dir` is None (current default): history lives only in `AgentLoop.messages` —
  all context is lost on process exit.

**Not implemented**:
- Session ID continuity (each `save_session()` creates a new timestamp-named file, not append)
- File/todo/worktree state restore (claude-code's full resume includes these; mobileclaw restores
  only the message history)
- Stale session cleanup (old `.jsonl` files accumulate indefinitely)

---

## 3. Memory Taxonomy

claude-code uses **4 core memory types** + **2 additional mechanisms**:

### 4 Core Types (`memdir/memoryTypes.ts`)

| Type | Purpose | Example |
|------|---------|---------|
| **user** | User profile (role, preferences, skills) | "I've written Go for 10 years, React newcomer" |
| **feedback** | Behavioral guidance (from failures AND successes) | "Don't mock DB in tests — burned us before" |
| **project** | Work context (tasks, decisions, deadlines) | "Merge freeze from 03-05, mobile release branch" |
| **reference** | External resource pointers | "Bugs tracked in Linear project INGEST" |

### 2 Additional Mechanisms

| Mechanism | Feature flag | Function |
|-----------|--------------|----------|
| **Team Memory** | `TEAMMEM` | Shared team memory directory + private/team scope tags |
| **Daily Logs** | `KAIROS` | Append-only daily logs for long-lived sessions, nightly `/dream` skill distills to MEMORY.md |

### Key Design Patterns

1. **MEMORY.md index**: ≤ 200 lines / 25KB, each line points to a standalone `.md` file
2. **Two-step write**: Step 1 write content file → Step 2 update MEMORY.md index
3. **Exclusion list** (`WHAT_NOT_TO_SAVE_SECTION` — always): code patterns/architecture, git history, debugging solutions, anything already in CLAUDE.md, ephemeral task details
4. **Verify before recommend** (`TRUSTING_RECALL_SECTION`): memory referencing a file must be `Glob` checked, memory referencing a function must be `Grep` verified
5. **Relative date absolutization**: "Thursday" → "2026-03-05" at save time, prevents stale interpretation

### mobileclaw current state

Memory taxonomy aligned with claude-code as of 2026-04-02 (`src/memory/types.rs`):

| mobileclaw type | Maps to claude-code | Notes |
|-----------------|--------------------|----|
| `Core` | `project` | serde alias `"core"` and `"project"` both accepted |
| `Daily` | Daily Logs (KAIROS) | same purpose |
| `User` | `user` | added; serde alias `"user"` |
| `Feedback` | `feedback` | added; serde alias `"feedback"` |
| `Reference` | `reference` | added; serde alias `"reference"` |
| `Conversation` | — | mobileclaw-specific; not in claude-code taxonomy |
| `Custom(String)` | — | extension point |

Legacy string `"conversation"` round-trips to `Conversation` for backwards compatibility.
`str_to_category()` in `sqlite.rs` handles all aliases without panicking on unknown strings.

**Not implemented**:
- Team Memory: no shared/private scope distinction
- Daily Logs: no append-only daily log, no `/dream`-style distillation

**Important**: SqliteMemory and session history (`Vec<Message>`) are **completely separate**.
Memory is only populated when `memory_write` is explicitly called — the agent's conversation
history is never automatically indexed into the searchable memory store.

---

## 4. Bridging to mobileclaw — Current Status

| claude-code mechanism | mobileclaw existing | Status |
|----------------------|---------------------|--------|
| Token counting (exact from API response) | Not implemented | Gap: mobileclaw uses estimation only, never reads back `usage` from API responses |
| Token estimation (4 bytes/token) | `token_counter.rs` ✓ | Aligned |
| Auto compaction threshold (187K tokens) | `context_manager.rs` ✓ | Aligned |
| Compaction strategy: distill then discard | Prune (hard delete oldest) | **Critical gap** — see Section 6 |
| Circuit breaker (max 3 consecutive failures) | Not implemented | Gap |
| JSONL session transcript | `session.rs` ✓ (atomic write) | Implemented; activation gap: `session_dir` defaults to None |
| Session resume on restart | FFI: `session_load()` ✓ | Implemented; Flutter must wire up `session_dir` |
| Full state restore (files/todos/worktrees) | Not implemented | Gap; mobileclaw restores message history only |
| Stale session cleanup | Not implemented | Gap; `.jsonl` files accumulate indefinitely |
| 4 core memory types | `User/Feedback/Reference/Core` ✓ | Aligned |
| MEMORY.md index | Via CLAUDE.md system ✓ | Aligned |
| Daily Logs (`/dream`) | Not implemented | Optional |
| Team Memory | Not implemented | Optional |

---

## 5. Implementation Priority (updated 2026-04-02)

Items 1–3 are complete. Remaining gaps ordered by impact:

1. ~~**Token estimation + context management**~~ — done (`token_counter.rs`, `context_manager.rs`)
2. ~~**Session persistence (JSONL + resume)**~~ — done (`session.rs`); activation requires `session_dir` to be wired in Flutter/CLI
3. ~~**Memory category alignment**~~ — done (User/Feedback/Reference added)
4. **Wire `session_dir` default in Flutter** — unblocks Android force-kill coverage without any Rust changes
5. **Improve prune strategy** — replace hard-delete with summary-then-discard (see Section 6)
6. **Auto memory extraction** — periodically distill conversation turns into memory store so `memory_search` can retrieve past context across sessions

---

## 6. Three-Layer Compaction: claude-code vs mobileclaw

claude-code uses three layers of progressively more aggressive compaction. mobileclaw currently
only has the most destructive layer (equivalent to "hard delete"), without the two softer layers.

### Layer 1: Microcompact (content-clear tool results)

**claude-code** (`services/compact/microCompact.ts`):
- Replaces old tool result *content* with `[Old tool result cleared]`, preserving message structure
- tool_use/tool_result message pair structure is kept intact — API invariants never violated
- Two triggers: count-based (too many tool results) and time-based (cache expired after idle gap)
- Zero LLM calls: pure local mutation

**mobileclaw**: not implemented. All tool results stay in history at full size until prune fires.

### Layer 2: Session Memory Compact (summarize unseen history, keep recent tail)

**claude-code** (`services/compact/sessionMemoryCompact.ts`):
- Fires when token count crosses threshold AND session memory file has content
- Keeps the most recent 10K–40K tokens of messages (configurable)
- Prepends a summary message sourced from session memory (already-extracted key facts)
- No LLM call for the compaction itself — summary content comes from the pre-existing session memory
- Result: model still "remembers" old decisions/facts via the injected summary; history is shorter

**mobileclaw**: not implemented. There is no session memory extraction process — nothing writes
a running summary of the conversation that could be reused at compact time.

### Layer 3: Legacy Conversation Compact (LLM-generated full summary)

**claude-code** (`services/compact/compact.ts`):
- Falls back to this when session memory is absent or too large
- Sends entire conversation to a separate LLM call with a summarization prompt
- Replaces all history with: `[summary message] + [recent N messages]`
- Most expensive: one extra LLM round-trip per compaction event

**mobileclaw**: not implemented.

### Current mobileclaw prune (beyond the three layers)

`context_manager.rs::prune_oldest_messages()` removes oldest non-protected messages by hard
deletion. No summary is generated. The model receives no information about what was pruned.
This is more destructive than any of claude-code's three layers:

| | Information preserved | API cost |
|---|---|---|
| Microcompact | Tool call names + structure | Zero |
| SM Compact | Key facts via session memory | Zero |
| Legacy Compact | Full LLM-generated summary | One extra LLM call |
| **mobileclaw prune** | **Nothing** | Zero |

### Upgrade path

A practical improvement without adding session memory infrastructure:

1. Before pruning, collect the text of messages about to be removed
2. Make a cheap LLM call (Haiku) to generate a 1–3 sentence summary
3. Insert that summary as an assistant message at the start of the surviving history
4. Then prune as normal

This gives the model a "what happened before" anchor, at the cost of one cheap LLM call per
prune event (rare — only when the 187K threshold is crossed).
