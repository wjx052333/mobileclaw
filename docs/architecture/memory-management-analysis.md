# Memory Management Analysis: claude-code vs mobileclaw

## Source

Study of `claude-code/src/memdir/`, `src/services/compact/`, `src/utils/tokens.ts`,
`src/utils/sessionRestore.ts`, and `docs/cc-evaluation/findings.md`.

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

- **No token counting** — message history grows unbounded
- `MAX_TOOL_ROUNDS = 10` limits tool iteration rounds, not total context size
- No mechanism to detect or prevent context overflow

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

- **No session persistence** — `chat()` is pure in-memory operation
- `AgentLoop.messages: Vec<Message>` lost on process exit
- Flutter app restart = complete context loss

---

## 3. Memory Taxonomy: Six Dimensions

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

---

## 4. Bridging to mobileclaw

| claude-code mechanism | mobileclaw existing | Gap & recommendation |
|----------------------|---------------------|---------------------|
| Token counting + auto compaction | None | **CRITICAL**: Add token estimation to `AgentLoop`, prune or compact when approaching model limits |
| JSONL session transcript | None | **IMPORTANT**: Persist each `chat()` complete output to `.jsonl`, supporting resume |
| 4 core memory types | 4 types (Core/Daily/Conversation/Custom) | **ALIGN**: semantic mapping Core→project, Conversation→conversation (keep as-is for now) |
| MEMORY.md index | Already have (CLAUDE.md system) | Already aligned |
| Daily Logs | None | **OPTIONAL**: Add for CLI/Flutter long sessions |
| Session restore | None | **IMPORTANT**: Flutter app restart restores previous session |

---

## 5. Implementation Priority

1. **Token estimation + context management** — prevents unbounded context growth and API billing overages
2. **Session persistence (JSONL + resume)** — survives process/app restart
3. **Memory category alignment** — unified naming with claude-code taxonomy
