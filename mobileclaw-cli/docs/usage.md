# mobileclaw-cli 使用手册

`mclaw` 是 mobileclaw-core 的命令行测试工具，直接调用与 Flutter FFI 相同的 Rust API，
无需启动移动端 App 即可验证 provider 配置、邮件账号管理和 agent 对话功能。

---

## 编译

### 前置条件

- Rust toolchain（stable）：`rustup show`
- mobileclaw-core 依赖均已在 workspace `Cargo.toml` 中声明，无需额外安装

### 调试构建（开发用）

```bash
# 在 workspace 根目录执行
cargo build -p mobileclaw-cli
# 产物：target/debug/mclaw
```

### Release 构建（推荐运行时使用）

```bash
cargo build -p mobileclaw-cli --release
# 产物：target/release/mclaw
```

### 运行测试

```bash
cargo test -p mobileclaw-cli
# 预期：2 passed（env_parser 单元测试）
```

### Clippy 检查

```bash
cargo clippy -p mobileclaw-cli -- -D warnings
# 预期：0 warnings, 0 errors
```

---

## 环境变量

| 变量 | 必填 | 说明 |
|------|------|------|
| `ANTHROPIC_API_KEY` | 否 | Anthropic API Key。设置后 `mclaw chat` 直接使用此 key，优先级高于 `provider set-active` 存储的 key。不设置则由 mobileclaw-core 从 secrets.db 读取 active provider。|
| `ANTHROPIC_MODEL` | 否 | 覆盖 active provider 的模型名，如 `claude-opus-4-6`。不设置则使用 provider 配置中保存的模型。|
| `MCLAW_DATA_DIR` | 否 | 数据目录路径，覆盖默认的 `~/.mobileclaw/`。也可通过 `--data-dir` 参数传入。|
| `HOME` | 系统提供 | 用于定位默认数据目录 `~/.mobileclaw/`。|

**优先级说明：**
- `ANTHROPIC_API_KEY` 存在 → 忽略 secrets.db 中的 active provider key
- `ANTHROPIC_API_KEY` 不存在，且已通过 `mclaw provider set-active` 配置 → 使用 secrets.db 中的 provider

---

## 数据目录

默认路径：`~/.mobileclaw/`（可通过 `--data-dir` 或 `MCLAW_DATA_DIR` 覆盖）

```
~/.mobileclaw/
├── memory.db    # agent 记忆数据库（SQLite，FTS5）
├── secrets.db   # 加密凭证数据库（AES-256-GCM）：provider key、email 密码
└── sandbox/     # agent 文件系统工具的沙箱目录
```

> **注意（Phase 1 已知限制）：** `secrets.db` 当前使用硬编码开发 key 加密，生产环境需替换为平台 keystore（见 `docs/mobileclaw-phase3-android.md`）。

---

## 命令参考

### 全局选项

```
mclaw [--data-dir <PATH>] <SUBCOMMAND>
```

| 选项 | 说明 |
|------|------|
| `--data-dir <PATH>` | 指定数据目录（也可用 `MCLAW_DATA_DIR` 环境变量）|

---

### provider — LLM Provider 管理

#### `provider add` — 添加 provider

```bash
mclaw provider add \
  --name <名称> \
  --protocol <协议> \
  --url <基础URL> \
  --model <模型名> \
  [--key <API Key>] \
  [--active true|false]
```

| 参数 | 必填 | 说明 |
|------|------|------|
| `--name` | 是 | 自定义名称，仅用于展示 |
| `--protocol` | 是 | `anthropic` \| `openai_compat` \| `ollama` |
| `--url` | 是 | API 基础 URL |
| `--model` | 是 | 模型名，如 `claude-opus-4-6`、`gpt-4o`、`llama3` |
| `--key` | 否 | API Key（Ollama 本地部署无需填写）|
| `--active` | 否 | 默认 `true`，添加后立即设为 active provider |

各协议典型配置：

```bash
# Anthropic
mclaw provider add --name "Claude Opus" --protocol anthropic \
  --url https://api.anthropic.com --model claude-opus-4-6 \
  --key sk-ant-xxxxx

# OpenAI 兼容（如 OpenAI、Together、DeepSeek 等）
mclaw provider add --name "GPT-4o" --protocol openai_compat \
  --url https://api.openai.com --model gpt-4o \
  --key sk-xxxxx

# Ollama 本地
mclaw provider add --name "Llama3 Local" --protocol ollama \
  --url http://localhost:11434 --model llama3
```

#### `provider list` — 列出所有 provider

```bash
mclaw provider list
```

输出示例：
```
ID                                     PROTOCOL       NAME                 MODEL
------------------------------------------------------------------------------------------
dab2685d-1234-...                      anthropic      Claude Opus          claude-opus-4-6 ✓ active
```

#### `provider set-active` — 切换 active provider

```bash
mclaw provider set-active <ID>
```

#### `provider delete` — 删除 provider

```bash
mclaw provider delete <ID>
```

#### `provider probe` — 测试 provider 连通性

```bash
# 测试已存储的 provider（推荐）
mclaw provider probe --id <ID>

# 临时测试，不存储配置
mclaw provider probe \
  --protocol anthropic \
  --url https://api.anthropic.com \
  --model claude-opus-4-6 \
  --key sk-ant-xxxxx
```

输出：
- `✓  OK (XXXms)` — 成功发起补全请求
- `⚠  Reachable (XXXms)` — 能到达服务器但补全失败（仅 /models 端点响应）
- `✗  Failed (XXXms): <错误信息>` — 连接失败

---

### email — 邮件账号管理

#### `email add-from-env` — 从 shell env 文件导入

```bash
mclaw email add-from-env \
  --id <账号ID> \
  --env-file /path/to/test_env.sh
```

env 文件格式（支持等号两侧有空格，支持引号，支持末尾逗号）：

```bash
export SMTP_SERVER = "smtp.163.com"
export SMTP_PORT = 25
export EMAIL_SENDER = "user@163.com"
export EMAIL_PASSWORD = "your_password"
export EMAIL_RECEIVER = "target@example.com"
```

IMAP 主机自动从 SMTP 主机推导（`smtp.xxx.com` → `imap.xxx.com`），IMAP 端口固定为 993。
如果 IMAP 地址与 SMTP 地址规律不同，请改用 `email add` 手动指定。

#### `email add` — 手动添加邮件账号

```bash
mclaw email add \
  --id <账号ID> \
  --smtp-host smtp.example.com \
  --smtp-port 465 \
  --imap-host imap.example.com \
  --imap-port 993 \
  --username user@example.com \
  --password your_password
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--smtp-port` | 465 | SMTP 端口 |
| `--imap-port` | 993 | IMAP 端口 |

#### `email delete` — 删除邮件账号

```bash
mclaw email delete <账号ID>
```

---

### chat — 交互式 agent 对话

```bash
mclaw chat [--system "自定义系统提示词"]
```

启动 REPL，输入自然语言，agent 可调用邮件工具、记忆工具、文件工具等。

```
Opening agent session...
Chat started. Type '/quit' or Ctrl-D to exit.

you> 帮我获取 work 账号最近 5 封邮件
agent> [tool call: email_fetch]
  [email_fetch: ok]
  收到 5 封邮件：...

you> /quit
Bye.
```

内置命令：
- `/quit` 或 `/exit` — 退出
- `Ctrl-D` 或 `Ctrl-C` — 退出

**依赖：** `chat` 命令需要 active provider 或 `ANTHROPIC_API_KEY` 环境变量。
若未配置任何 provider，启动时会报错。

---

### bench — 上下文窗口压测

```bash
mclaw bench [OPTIONS]
```

从 JSON 文件批量读入多轮 prompt，逐轮调用 agent，实时打印每轮的 token 估算、
pruning 触发情况、响应字符数和进程 RSS，最后输出汇总报告。用于验证 context-window
pruning 机制是否按预期保护内存。

#### 选项

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `--prompts <PATH>` | `docs/bench_prompts.json` | prompt 数据文件路径 |
| `--system <TEXT>` | 内置系统提示 | 覆盖 agent 系统提示词 |
| `--max-turns <N>` | 全部 | 只跑前 N 轮（快速冒烟用）|
| `--dry-run` | false | 打印 prompt 预览，不实际调用 LLM |

#### 快速开始

```bash
# 确认 prompts 格式正确，不消耗 API
./target/release/mclaw bench --dry-run

# 跑前 3 轮快速验证 pruning 路径
./target/release/mclaw bench --max-turns 3

# 全量压测（需配置好 provider）
./target/release/mclaw bench

# 使用自定义 prompts 文件
./target/release/mclaw bench --prompts /path/to/stress.json
```

#### 输出格式

运行时逐行打印每轮数据，pruning 触发时额外输出一行详情：

```
╔══════════════════════════════════════════════════════════════╗
║            mobileclaw context-window stress bench            ║
╠══════════════════════════════════════════════════════════════╣
║ End-to-end context-window stress test for mobileclaw ...     ║
║ turns:  10   prune threshold ≈   187000 tokens               ║
╚══════════════════════════════════════════════════════════════╝

turn  label                         elapsed  tok_bef  tok_aft  pruned  h_len  resp_ch  rss_MiB
────────────────────────────────────────────────────────────────────────────────────────────────
   1  Architecture overview request  4832ms    1240     1240       0      2    12400    45MiB
   2  Deep-dive: backpressure         6104ms    3890     3890       0      4    18200    52MiB
   3  Memory layout and SIMD          5820ms    8210     8210       0      6    14700    57MiB
   ...
   7  Security hardening             7340ms  192400   104300      18     12    21000    63MiB✂
       ✂ PRUNING FIRED: 18 msgs removed, tokens 192400 → 104300 (threshold 187000)
   8  Graceful shutdown              6910ms  113800   113800       0     14    19500    64MiB
   ...

════════════════════════════════════════════════════════════════════════════════
  BENCH SUMMARY
────────────────────────────────────────────────────────────────────────────────
  Total turns         : 10
  Total wall time     : 61.3s
  Avg turn latency    : 6130ms
  Peak token estimate : 192400 tokens
  Pruning events      : 1 (turns: 7)
  Total msgs pruned   : 18
  Final RSS           : 64 MiB

  ✓ Context-window pruning is working correctly.
════════════════════════════════════════════════════════════════════════════════
```

#### 列含义

| 列 | 说明 |
|----|------|
| `turn` | 轮次编号（来自 JSON `id` 字段）|
| `label` | 轮次标签（来自 JSON `label` 字段，截断至 28 字符）|
| `elapsed` | 本轮 wall-clock 耗时（含网络 + 工具执行），末尾 `✂` 表示本轮触发了 pruning |
| `tok_bef` | 本轮用户消息入队**后**、pruning 执行**前**的 token 估算值 |
| `tok_aft` | pruning 执行**后**的 token 估算值（未触发时与 tok_bef 相同）|
| `pruned` | 本轮被移除的消息条数（0 表示未触发）|
| `h_len` | 本轮结束后 history 中的消息总条数 |
| `resp_ch` | agent 响应的字符数（TextDelta 累计）|
| `rss_MiB` | 本轮结束后进程 RSS（来自 `/proc/self/status`，非 Linux 下显示 0）|

#### prompt JSON 格式

```json
{
  "meta": {
    "description": "测试描述",
    "pruning_threshold_approx": 187000
  },
  "turns": [
    {
      "id": 1,
      "label": "轮次标签",
      "prompt": "完整的 prompt 文本..."
    }
  ]
}
```

内置文件 `docs/bench_prompts.json` 包含 10 轮连贯的 Rust 异步架构技术调研对话，
每轮 prompt 含大量代码片段和详细问题，约 600–1200 tokens/轮，可快速将 history 推入
pruning 触发区间。

#### 关键设计

**`ContextStats` 事件（core 层打点）**

每次 `AgentSession.chat()` 返回前，core 都会在 `Done` 事件之前插入一个
`ContextStats` 事件：

```
AgentEvent::ContextStats {
    tokens_before_turn,   // 用户消息入队后、pruning 前的估算 token 数
    tokens_after_prune,   // pruning 后的 token 数（未触发时 == tokens_before_turn）
    messages_pruned,      // 本轮移除的消息数（0 = 未触发）
    history_len,          // pruning 后 history 条数
    pruning_threshold,    // 本次使用的阈值（max_tokens - buffer_tokens）
}
```

这使 bench 命令（以及任何 FFI 消费者）无需自行估算 token，直接从事件流读取
可观测数据。`ContextStats` 同样作为 `AgentEventDto` 透过 FFI 边界传递，
Flutter UI 未来可用它渲染 token 使用进度条。

**Token 估算方法**

使用 4 bytes/token 的线性估算（与 claude-code 一致），O(N) 扫描，无 API 调用，
无堆分配。误差约 ±20%，足以驱动 pruning 决策，不影响正确性。

**RSS 监控**

通过读取 `/proc/self/status` 的 `VmRSS` 字段获取当前进程的 Resident Set Size。
在 pruning 有效的情况下，RSS 应保持平稳（不随轮次单调增长），即使 prompt 持续
注入大量 token。

---

## 典型工作流

### 首次使用（Anthropic）

```bash
# 1. 添加并设置 active provider
mclaw provider add \
  --name "Claude Opus" \
  --protocol anthropic \
  --url https://api.anthropic.com \
  --model claude-opus-4-6 \
  --key $ANTHROPIC_API_KEY

# 2. 验证连通性
mclaw provider list
mclaw provider probe --id <上一步输出的 ID>

# 3. 导入邮件账号（可选）
mclaw email add-from-env --id work --env-file test_env.sh

# 4. 开始对话
mclaw chat
```

### 使用临时数据目录（隔离测试）

```bash
MCLAW_DATA_DIR=/tmp/mclaw-test mclaw provider add \
  --name "Test" --protocol anthropic \
  --url https://api.anthropic.com --model claude-opus-4-6 \
  --key $ANTHROPIC_API_KEY

MCLAW_DATA_DIR=/tmp/mclaw-test mclaw chat
```

### 切换 provider

```bash
mclaw provider list                  # 查看所有 provider 及其 ID
mclaw provider set-active <新ID>     # 切换
mclaw provider list                  # 确认 ✓ active 标记已更新
```

---

## 与 Flutter FFI 的对应关系

`mclaw` 调用的 Rust 函数与 Flutter 通过 FFI 调用的完全相同，是 mobileclaw-core 的集成测试入口。

| mclaw 命令 | 对应 Flutter/FFI 调用 |
|---|---|
| `provider add` | `AgentSession.providerSave()` |
| `provider list` | `AgentSession.providerList()` |
| `provider probe` | `providerProbe()` (free fn) |
| `email add-from-env` | `AgentSession.emailAccountSave()` |
| `email delete` | `AgentSession.emailAccountDelete()` |
| `chat` | `AgentSession.chat()` |
| `bench` | `AgentSession.chat()` × N（读取 `AgentEventDto::ContextStats` 做可观测性）|
