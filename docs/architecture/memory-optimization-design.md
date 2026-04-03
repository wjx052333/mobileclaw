# Memory Optimization Design: Per-Turn Summary + Count-Based Prune

Design date: 2026-04-02  
Branch: `feat+memory-optimization`  
Status: implemented, all tests passing

---

## Problem

Before this design, mobileclaw had three gaps:

1. **User input lost on restart** — conversation history lived only in `AgentLoop.messages` (in-memory); a process restart lost everything.
2. **Destructive context prune** — `prune_oldest_messages()` hard-deleted oldest messages with no summary; the model received no information about what was pruned, breaking conversation continuity.
3. **Token-only prune trigger** — 187K token threshold is a blunt instrument; reasoning about "how many turns fit" is easier as a message count.

---

## Design

Three coordinated features:

### Feature 1 — Per-Turn Summary (post-chat)

After each complete `chat()` call (all tool rounds finished), a second lightweight LLM call generates a one-sentence summary of the full interaction.

- **Trigger**: automatic, every turn, after `inner.chat()` returns
- **LLM call**: `summarize_interaction()` with a 150-token budget, system prompt: *"Summarize the following AI assistant interaction in exactly one sentence."*
- **Stored path**: `history/{session_id}/{timestamp_hex}` (16-char lowercase hex, second-granularity)
- **Category**: `Conversation`
- **Content format**:
  ```
  User: {user_input}
  Summary: {one-sentence summary}
  ```
- **Fail-open**: if the summary LLM call fails or returns empty, the user input is stored alone (`User: {input}`) and no `TurnSummary` event is emitted; the failure is logged as a warning but never returned as an error.
- **FFI event**: `AgentEventDto::TurnSummary { summary: String }` is injected before the final `Done` event when the summary succeeds.

### Feature 2 — Count-Based Prune with History Prefix (pre-chat)

Before each `chat()` call, if `history.len() >= max_session_messages`, the oldest unprotected messages are dropped. Before dropping, stored turn summaries are retrieved and injected as a system message prefix so the model retains continuity.

- **Trigger**: automatic, before `inner.chat()`, when `history.len() >= max_session_messages`
- **Default limit**: 100 messages (configurable via `AgentConfig.max_session_messages`)
- **What is protected from dropping** (never removed):
  - All `System` role messages
  - Last assistant turn
  - Last N user turns (`min_user_turns`, default 3)
- **History prefix construction**: queries `SqliteMemory` for all docs under `history/{session_id}/`, takes the oldest N (where N = number of messages being dropped), extracts `Summary: ...` lines, formats as:
  ```
  Previously in this session:
  - User asked about X; assistant explained Y.
  - User requested a file write; tool succeeded.
  ```
  This prefix is injected as a `System` message at position 0 of the trimmed history.
- **Approximate matching**: the prefix uses stored summaries ordered by `created_at ASC` — there is no hard timestamp binding between a summary doc and a specific history message. If fewer summaries are stored than messages dropped, the available summaries are used (partial coverage).

### Feature 3 — Fallback Token-Based Prune (unchanged)

The existing `prune_oldest_messages()` in `AgentLoop::chat()` remains as a fallback. It fires if the token count still exceeds the threshold after count-based prune. This handles pathological cases (single very large message, tool result floods, etc.).

---

## Architecture

### Data Flow per `AgentSession::chat()` call

```
AgentSession::chat(input, system)
    │
    ├── Phase A: Count prune
    │     count_prune_candidates() → [indices to drop]
    │     if non-empty:
    │       build_history_prefix(memory, session_id, n) → Option<String>
    │       inner.apply_count_prune(candidates, prefix_msg)
    │
    ├── Phase B: Main chat
    │     inner.chat(input, system) → Vec<AgentEvent>
    │     (token prune fires here if still over threshold)
    │
    ├── Phase C: Summary + store (fail-open)
    │     build_interaction_text(input, events) → String (≤4000 chars)
    │     inner.summarize_interaction(text) → Option<String>
    │     memory.store("history/{session_id}/{ts_hex}", content, Conversation)
    │
    └── Phase D: Convert to DTO
          Vec<AgentEvent> → Vec<AgentEventDto>
          if summary: insert TurnSummary before Done
```

### New public API surface

| Location | Addition |
|----------|----------|
| `AgentConfig` | `pub max_session_messages: Option<u32>` |
| `AgentEventDto` | `TurnSummary { summary: String }` (before `Done`) |
| `AgentSession` | `session_id: String` (private, generated on `create()`) |
| `AgentLoop` | `count_prune_candidates(&self) -> Vec<usize>` |
| `AgentLoop` | `apply_count_prune(&mut self, candidates, prefix_msg)` |
| `AgentLoop` | `summarize_interaction(&self, text) -> ClawResult<String>` |
| `ContextConfig` | `pub max_messages: Option<usize>` |
| `SqliteMemory` | `list_by_path_prefix(&self, prefix) -> ClawResult<Vec<MemoryDoc>>` |

### Searching stored summaries via FFI

Turn summaries are written to `SqliteMemory` with category `Conversation`. The existing `memory_recall` API can search them:

```dart
// Search conversation history only
session.memoryRecall(
  query: "file write",
  limit: 10,
  category: "conversation",  // ← filter to conversation history
);
```

FTS5 trigram tokenizer searches both the `User:` line and `Summary:` line. Tokens shorter than 3 characters will not match (trigram limitation).

For **chronological** session history retrieval (timeline view), `list_by_path_prefix` is available internally but not yet exposed via FFI. Add `session_recall(session_id, limit)` to `AgentSession` if that use case arises.

---

## Files Modified

| File | Change |
|------|--------|
| `mobileclaw-core/src/memory/sqlite.rs` | Added `list_by_path_prefix()` |
| `mobileclaw-core/src/agent/context_manager.rs` | Added `max_messages` field; added `count_prune_candidates()` fn |
| `mobileclaw-core/src/agent/loop_impl.rs` | Added `SUMMARY_SYSTEM`, `SUMMARY_MAX_TOKENS`; added 3 new methods |
| `mobileclaw-core/src/ffi.rs` | Added `max_session_messages`, `session_id`, `TurnSummary`; rewrote `chat()`; added 3 helpers |

No new files. No `Cargo.toml` changes.

---

## Test Coverage

### New tests per module

| Module | New tests | What they verify |
|--------|-----------|-----------------|
| `sqlite.rs` | 4 | `list_by_path_prefix`: order, exclusion, empty, LIKE metachar escaping |
| `context_manager.rs` | 5 | `count_prune_candidates`: empty/under-limit, correct count, protection, no-panic, all-protected |
| `loop_impl.rs` | 4 | `count_prune_candidates`, `apply_count_prune`, `summarize_interaction` (mock LLM) |
| `ffi.rs` | 9 | `AgentConfig` field, `TurnSummary` variant, `build_interaction_text` (3), `current_timestamp_hex`, `build_history_prefix` (3) |

### Coverage of edge cases

| Edge case | Covered by |
|-----------|------------|
| Summary LLM call fails → user input still stored | `loop_impl.rs` `summarize_interaction` returning error |
| Summary empty string → treated as failure | `ffi.rs` `Ok(s) if !s.is_empty()` branch |
| Memory store fails → logged, not returned | `inspect_err` on store call in Phase C |
| All messages protected → count prune is no-op | `context_manager.rs::count_prune_all_protected_returns_empty` |
| No summaries stored yet → prefix is None | `ffi.rs::build_history_prefix_returns_none_when_no_docs` |
| Prefix LIKE metachar in session_id path | `sqlite.rs::list_by_path_prefix_escapes_like_metacharacters` |
| Interaction text > 4000 chars → truncated | `ffi.rs::build_interaction_text_truncates_at_4000` |
| Fewer stored summaries than dropped messages | `build_history_prefix` uses `.take(n)` so partial coverage is fine |

---

## Configuration Reference

| Field | Default | Effect |
|-------|---------|--------|
| `AgentConfig.max_session_messages` | `None` (→ 100) | Count trigger for history prune |
| `AgentConfig.context_window` | `None` (→ 200_000) | Token threshold for fallback prune |
| `ContextConfig.buffer_tokens` | 13_000 | Headroom below token threshold |
| `ContextConfig.min_user_turns` | 3 | Protected user turns at tail of history |

---

## Design Decisions & Trade-offs

**Why count-based prune, not only token-based?**  
Message count is simpler to reason about from the product side ("show last 100 turns"). Token-based prune remains for worst-case safety (single huge message, bulk tool results).

**Why separate LLM call for summary (not part of main response)?**  
The summary prompt is strictly controlled (`≤ 150 tokens`, single-sentence instruction). Bundling it with the main response would require parsing structured output from the assistant, which is fragile. A fresh call is cheap (Haiku-class cost) and reliable.

**Why approximate matching for history prefix?**  
Exact binding of a summary doc to a specific history position would require timestamps on `Message` objects (a larger schema change). The approximate approach — "oldest N summaries ≈ oldest N dropped turns" — is correct for normal sequential chat and costs zero additional complexity.

**Why fail-open on summary and store?**  
The main `chat()` must never fail because a background summary write failed. Errors here are operational (network, DB lock) and should not interrupt the user experience. The next turn will attempt its own summary, so the impact of one missed summary is minimal.
