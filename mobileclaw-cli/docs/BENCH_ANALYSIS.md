# 压测分析报告

## 历史问题（已解决）

**之前的问题**：30轮bench运行到94,593 tokens时没有触发pruning，因为阈值是187,000 tokens，需要到第59轮才能达到。

| Turn | Tokens | Messages | Pruned |
|------|--------|----------|--------|
| 1    | 231    | 2        | 0      |
| 11   | 32,097 | 24       | 0      |
| 21   | 65,712 | 44       | 0      |
| 30   | 94,593 | 62       | 0      |

**根因**：只有token-based prune（阈值187k），LLM回答质量很好但每轮token增长只有~3.1k，导致需要59轮才能触发。

---

## 解决方案（已实现：feat/memory-optimization）

### 新增：Count-based prune（计数触发）

在token prune之前，先检查消息数量：

```
AgentSession::chat()
  Phase A: count_prune_candidates()
    if history.len() >= max_session_messages (default 40):
      → 注入"Previously in this session:..."前缀（从stored summaries构建）
      → 删除最旧的非保护消息
  Phase B: inner.chat()  ← token prune仍作为fallback
```

**触发点**：`max_session_messages=40`（bench默认值）
- 每轮产生2条消息（user + assistant）
- Turn ~18 时首次触发（约38-42条消息）
- 之后每隔数轮触发一次（持续管理history长度）

### 新增：Per-turn summary（轮次摘要）

每轮结束后，自动调用轻量LLM生成一句话摘要，写入 `SqliteMemory`：
- Path: `history/{session_id}/{timestamp_hex}`
- Category: `Conversation`
- Content: `User: {input}\nSummary: {one-sentence summary}`

Count prune注入的前缀正是从这些摘要中取第N条（被删的最旧N条）：
```
Previously in this session:
- User asked about CLI framework; assistant recommended clap for derive macros.
- User presented SQLite schema design; assistant raised FTS5 concerns.
```

---

## 当前测试预期

使用 `bench_prompts_50turns.json` 运行50轮：

```bash
mclaw bench \
  --prompts docs/bench_prompts_50turns.json \
  --max-session-messages 40 \     # count prune at 40 msgs (default)
  --turn-delay-ms 2000 \          # rate limit protection
  --interaction-log bench_run.jsonl
```

**预期输出变化**：
- Turn ~18：`✂ PRUNING FIRED` 首次出现（count prune）
- 每轮：`✍ [summary]: User asked about X; assistant explained Y.`（per-turn summary）
- bench summary：`Turn summaries: 50/50`（全部成功）

### 如果count prune也没触发

检查：
1. `max_session_messages` 是否生效 → 看bench header `msg limit: 40`
2. 查 `mclaw.log` 中的 `count-based history prune applied` 日志
3. `ContextStats.history_len` 是否达到40

---

## Token prune验证（补充）

若需同时验证token prune fallback，可用低阈值配置：

```bash
# 环境变量或直接修改session配置 → context_window=50000
# 此时token prune会在~16轮时触发
# Count prune(40条)仍先于token prune触发
```

---

## 行动项（更新版）

| 状态 | 行动 | 完成 |
|------|------|------|
| ✅ | Count-based prune（40条触发） | feat/memory-optimization |
| ✅ | Per-turn summary写入SqliteMemory | feat/memory-optimization |
| ✅ | Pruning前注入历史摘要前缀 | feat/memory-optimization |
| ✅ | CLI --max-session-messages=40参数 | feat/memory-optimization |
| 🔲 | 实际运行50轮bench验证（计数prune + 摘要） | 待运行 |
| 🔲 | 运行bench_prompts_memory_search.json验证memory_search可搜到摘要 | 待运行 |
| 🔲 | 检查prune后对话连贯性（"Previously"前缀效果） | 待验证 |
