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
# 查看全部（human-readable）
python3 tools/dump_memory.py build/memory.db

# 最新 20 条
python3 tools/dump_memory.py build/memory.db -n 20

# 只看 conversation 类别（per-turn summaries）
python3 tools/dump_memory.py build/memory.db -c conversation

# 只看某个 session 的历史（按 session_id 前缀）
python3 tools/dump_memory.py build/memory.db -p history/b115a1d3-

# 最新 5 条 conversation
python3 tools/dump_memory.py build/memory.db -c conversation -n 5

# JSON 输出，再用 jq 过滤
python3 tools/dump_memory.py build/memory.db --json | jq '.[].path'
python3 tools/dump_memory.py build/memory.db -c conversation --json | jq '.[] | {path, content}'

# 使用环境变量指定 data dir（与 mclaw 一致）
python3 tools/dump_memory.py "$MCLAW_DATA_DIR/memory.db" -n 10
```

### 输出格式（human-readable）

```
memory.db: build/memory.db
showing 5/94 rows (category=conversation)
────────────────────────────────────────────────────────────────────────────────
path     : history/b115a1d3-.../0000000069cf4e1f
category : conversation
created  : 2026-04-03 05:20:53 UTC
content  : User: ... ↵ Summary: ...
────────────────────────────────────────────────────────────────────────────────
```

### 路径规律

| 类别 | 路径格式 | 写入时机 |
|------|---------|---------|
| `conversation` | `history/{session_id}/{timestamp_hex}` | 每轮 `chat()` 结束后自动写入 |
| 其他 | 由调用方决定 | 通过 `memory_store` API 手动写入 |

`timestamp_hex` 为 Unix 秒的 16 位小写十六进制（`{:016x}`），可直接排序。
