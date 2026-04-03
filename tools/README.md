# tools/

调试和分析工具，不依赖 Rust 编译，直接运行。

---

## dump_memory.py

将 `memory.db`（SQLite）中的 `documents` 表内容输出到终端。

### 依赖

Python 3.6+，仅用标准库（`sqlite3`、`json`、`argparse`）。

### 用法

```
dump_memory.py [DB_PATH] [-n LIMIT] [-c CATEGORY] [-p PREFIX] [--json]
```

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `DB_PATH` | memory.db 路径 | `~/.mobileclaw/memory.db` |
| `-n, --limit N` | 最多显示 N 条（取最新 N 条） | 0（全部） |
| `-c, --category CAT` | 按类别过滤：`conversation` / `core` / `daily` / `user` / `feedback` / `reference` | 不过滤 |
| `-p, --prefix PATH` | 按路径前缀过滤，如 `history/` | 不过滤 |
| `--json` | 输出 JSON 数组（而非人类可读格式） | 否 |

### 示例

```bash
python3 tools/dump_memory.py build/memory.db
python3 tools/dump_memory.py build/memory.db -n 20
python3 tools/dump_memory.py build/memory.db -c conversation
python3 tools/dump_memory.py build/memory.db -p history/b115a1d3-
python3 tools/dump_memory.py build/memory.db -c conversation --json | jq '.[] | {path, content}'
```

---

## e2e_tool_accuracy.sh + analyze_tool_e2e.py

**端到端工具调用准确率测试**：跑 30 轮固定 prompt，检验 LLM 是否能准确调用指定工具，输出成功率。

### 前置条件

- `MCLAW_DATA_DIR` 环境变量已设置
- Release 二进制已构建：`cargo build --release -p mobileclaw-cli`
- Python 3.6+

### 一键运行

```bash
export MCLAW_DATA_DIR=/home/wjx/agent_eyes/bot/mobileclaw/.claude/worktrees/feat+memory-optimization/build
./tools/e2e_tool_accuracy.sh
```

如果未设置 `MCLAW_DATA_DIR`，脚本会打印提示并退出（exit 2）。

### 单独分析已有日志

```bash
python3 tools/analyze_tool_e2e.py \
    build/bench_tool_e2e.jsonl \
    mobileclaw-cli/docs/bench_prompts_tool_e2e.json
```

### 输出示例

```
──────────────────────────────────────────────────────────────────────────────
  Tool-Call E2E Accuracy Report
──────────────────────────────────────────────────────────────────────────────
   ID  Label                               Expected                Actual                  Result
──────────────────────────────────────────────────────────────────────────────
    1  time: what time is it               time                    time                    PASS
    2  memory_write: store a fact          memory_write            memory_write            PASS
    3  memory_get: retrieve stored fact    memory_get              memory_get              PASS
   ...
──────────────────────────────────────────────────────────────────────────────

  Total turns evaluated : 30
  Passed                : 29
  Failed                : 1
  Success rate          : 96.7%
```

### Prompts 文件

`mobileclaw-cli/docs/bench_prompts_tool_e2e.json` — 30 条固定 prompt，每条带 `expected_tools` 字段。

工具覆盖：`time` / `memory_write` / `memory_get` / `memory_search` / `memory_delete` / `file_write` / `file_read`

### 退出码

| 退出码 | 含义 |
|--------|------|
| 0 | 所有期望工具全部被调用（100%） |
| 1 | 有至少一轮缺失期望工具 |
| 2 | `MCLAW_DATA_DIR` 未设置 |
