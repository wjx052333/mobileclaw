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
