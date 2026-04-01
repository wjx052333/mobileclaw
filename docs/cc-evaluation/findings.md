# Findings: 四项目研读 + 移动端调研

---

# 一、研读报告：四项目对比分析

## 1.1 项目概览

| 维度 | claude-code | zeroclaw | ironclaw | nanoclaw |
|------|------------|----------|----------|----------|
| **语言** | TypeScript / React (Ink) | Rust | Rust | TypeScript / Node.js |
| **运行时** | Bun | Tokio async | Tokio + Docker | Node.js + Docker |
| **规模** | ~37个src目录，数百KB | 大型，含50+工具 | 中大型，含WASM沙箱 | 小型，~8.2K行 |
| **Memory 后端** | Markdown 文件（MEMORY.md索引） | SQLite混合搜索 / Markdown / PostgreSQL / Qdrant / Mem0 | PostgreSQL + pgvector / libSQL | SQLite（消息DB）+ Markdown文件 |
| **工具隔离** | 多层安全检查 + 可选Sandbox（SandboxManager） | SecurityPolicy + Docker/Landlock/Firejail/Bubblewrap | WASM（wasmtime）+ Docker容器 | Docker容器 + IPC命名空间 + Mount allowlist |
| **Skill 机制** | Markdown SKILL.md，frontmatter元数据，SkillTool执行 | TOML/Markdown，安全审计，可自动生成 | SKILL.md + YAML frontmatter，信任衰减（Installed < Trusted） | SKILL.md + 分支模型，容器内复制 |
| **规划机制** | Plan Mode V2（多代理并行探索） | SOP引擎（MQTT/Webhook/Cron触发，冷却+并发控制） | ActionPlan（LLM生成，可选） | Task Scheduling（cron/interval/once + 前置脚本） |
| **部署形态** | CLI 工具（本地） | 服务器/守护进程/嵌入式 | 服务器 | 服务器（Docker） |

---

## 1.2 长期 Memory 管理机制对比

### claude-code 的 Memory 设计

**哲学**：Memory 是写给未来自己的笔记，结构化但轻量，以 Markdown 文件为载体。

**架构要点**：
- **入口点**：`MEMORY.md`（索引，≤200行/25KB），每行指向一个独立 `.md` 文件
- **存储位置**：`~/.claude/projects/<project-slug>/memory/`
- **四类 Memory**：`user`（用户偏好）、`feedback`（行为指导）、`project`（项目上下文）、`reference`（外部资源指针）
- **写入方式**：模型用 Write 工具写文件，两步操作（写内容文件 → 更新 MEMORY.md 索引）
- **读取方式**：会话开始时同步加载 MEMORY.md 注入系统提示
- **Frontmatter 格式**：`name`, `description`, `type` 三字段
- **自动提取**：后台代理（extract memories）分析对话，自动写入 memory 目录
- **团队扩展**：可选 TEAMMEM 特性，支持私有+团队双目录

**关键限制**：
- MEMORY.md 超过200行会截断，需要保持索引精简
- 单文件内容不限，但建议简洁
- 不提供语义搜索，靠 LLM 判断相关性

**适合场景**：个人开发者工具，会话间持续学习用户偏好和项目上下文

---

### zeroclaw 的 Memory 设计

**哲学**：工业级内存管理，多后端可插拔，混合搜索（向量+关键字融合），嵌入式友好。

**架构要点**：
- **核心 Trait**：`Memory`（store/recall/get/list/forget/count/store_procedural）
- **存储后端**：SQLite（推荐，2031行实现）、Lucid（本地+云桥接）、Markdown、PostgreSQL、Qdrant、Mem0、None（可插拔）
- **分类系统**：`Core`（长期事实）、`Daily`（日志）、`Conversation`（上下文）、`Custom`
- **会话隔离**：所有操作支持 `session_id` 隔离
- **时间查询**：`since`/`until` RFC 3339 时间范围过滤

**SQLite 后端核心技术**：
```
向量搜索（余弦相似度，BLOB存储）
    ↓ 权重 0.7
混合融合（加权合并）
    ↑ 权重 0.3
关键字搜索（FTS5 BM25评分）
```
- 嵌入缓存：SHA-256哈希 + LRU驱逐（10,000条上限）
- WAL模式 + MMAP + 缓存优化，适合嵌入式
- FTS5 触发器自动同步索引

**Auto-save 过滤**：排除 Cron 输出、心跳、蒸馏索引等合成噪声

**过程性内存**：`store_procedural()` 从 LLM 提取"如何做"模式

---

### ironclaw 的 Memory 设计

**哲学**：数据库支持的工作区，提供文件系统语义，全文+向量混合搜索，安全边界防护。

**架构要点**：
- **存储模型**：`MemoryDocument`（文档）+ `MemoryChunk`（块，用于搜索）
- **路径系统**：类文件系统路径（`MEMORY.md`, `daily/2024-01-15.md`, `projects/alpha/notes.md`）
- **两种后端**：PostgreSQL+pgvector（生产）、libSQL（嵌入式/边缘）
- **搜索策略**：RRF（Reciprocal Rank Fusion）融合全文+向量结果
- **嵌入提供商**：OpenAI、Ollama（本地）、NEAR AI（可插拔）
- **Identity 保护**：4个受保护文件（IDENTITY.md, SOUL.md, AGENTS.md, USER.md）不可被工具覆写
- **自动清洁**：hygiene.rs 定期去重、删除过时文档、合并相似块

**写入工具的安全设计**：
- 拒绝本地文件系统路径（`/Users/...`、`C:\...`）
- 保护身份文件
- 速率限制（20/分钟，200/小时）
- 路径验证和类型检查

---

### nanoclaw 的 Memory 设计

**哲学**：最简单直接，用文件系统 + SQLite 消息数据库，没有向量搜索，靠长上下文窗口。

**架构要点**：
- **两层存储**：SQLite（消息历史 + 会话ID + 任务）+ Markdown文件（groups目录）
- **groups 目录结构**：
  ```
  groups/
  ├── main/CLAUDE.md          # 系统提示（長期记忆写在这里）
  ├── global/CLAUDE.md        # 跨组共享只读
  └── {channel}_{name}/
      ├── CLAUDE.md           # 组系统提示
      └── conversations/      # 自动归档对话
  ```
- **会话连续性**：通过 `sessionId` 跨次调用保持 Claude Agent SDK 会话
- **Stale Session 恢复**：检测到会话失效时自动清除并重建
- **对话归档**：session compact 前的 hook 把 `.jsonl` 转为 Markdown 存档
- **没有向量搜索**：完全依赖 Claude 的长上下文理解历史

---

### Memory 设计对比总结

| 特性 | claude-code | zeroclaw | ironclaw | nanoclaw |
|------|------------|----------|----------|----------|
| 存储形式 | Markdown文件 | SQLite/多后端 | PostgreSQL/libSQL | SQLite + Markdown |
| 向量搜索 | ❌ | ✅（SQLite FTS5+向量） | ✅（pgvector+RRF） | ❌ |
| 会话隔离 | ✅（project slug隔离） | ✅（session_id参数） | ✅（agent_id隔离） | ✅（group_folder隔离） |
| 自动提取 | ✅（后台代理） | ✅（store_procedural） | ❌（工具调用写入） | ❌（手动写CLAUDE.md） |
| 分类系统 | 4类（user/feedback/project/reference） | 4类（Core/Daily/Conversation/Custom） | 路径即分类 | 2层（全局/组） |
| 嵌入式适配 | ✅（纯文件，零依赖） | ✅（SQLite WAL+MMAP，<5MB RAM） | ✅（libSQL特性） | ❌（需要Docker） |
| 离线可用 | ✅ | ✅（SQLite后端） | ✅（libSQL后端） | ✅（本地SQLite） |
| 团队共享 | ✅（TEAMMEM特性） | ❌ | ❌ | ✅（global目录） |

---

## 1.3 工具隔离机制对比

### claude-code 的工具隔离

**核心设计**：多层安全检查，代码侧防御，可选 Sandbox。

**六层防御**：
1. **输入验证**：Zod schema 验证工具参数
2. **安全扫描**：23项Bash安全检查（命令AST分析，tree-sitter）
3. **权限规则**：白名单/黑名单规则（来自 settings.json / 命令行 / 会话）
4. **路径验证**：符号链接解析，边界检查
5. **沙箱隔离**：SandboxManager（可选，部分命令强制）
6. **事后审计**：Hooks系统 + 遥测日志

**工具注册**：静态注册表（`tools.ts`），条件编译（`feature()`特性标志），受保护工具名不可覆盖。

**权限决策链**：
```
安全扫描 → 路径验证 → 规则匹配（allow/deny/ask）→ 分类器（可选）→ 用户提示
```

**评价**：设计精细，安全性高，但是耦合度高，全部在主进程执行（Bash沙箱可选）。

---

### zeroclaw 的工具隔离

**核心设计**：Trait驱动，后端可插拔，多沙箱选项。

**工具执行流程**：
1. XML 格式解析（`<tool_call>` 块）
2. 按名称查找工具（allowed_tools 白名单过滤）
3. 并行或顺序执行（configurable）
4. 返回 `<tool_result name="..." status="ok">...</tool_result>`

**沙箱后端**（Security Trait，可选）：Docker / Landlock（Linux 5.13+）/ Firejail / Bubblewrap / Noop

**SecurityPolicy**：
- `AutonomyLevel`：Supervised（每步批准）/ Trusted（自动执行）/ Autonomous（完全自主）
- 网络域白名单/黑名单
- 可选 OTP 二次认证

**评价**：极度模块化，Rust零成本抽象，适合嵌入式和服务器双场景。

---

### ironclaw 的工具隔离

**核心设计**：WASM沙箱（代码隔离）+ Docker容器（执行隔离）+ HTTP代理（网络隔离）+ 凭证注入（秘密隔离）

**四层隔离**：

**层1 — WASM 代码隔离**（wasmtime component model）：
```
WASM Tool（不受信任）
  ↓ 主机函数边界（安全检查）
  ↓ Allowlist 验证器（网络检查）
  ↓ 凭证注入（秘密替换）
  ↓ 执行请求（隔离HTTP客户端）
  ↓ 泄漏检测（扫描响应）
  ↓ 返回WASM（清洁，无秘密）
```
- 每次执行创建新实例（无全局状态）
- 燃料计量（CPU时间限制）
- 内存限制（每实例10MB）

**层2 — Docker 容器执行隔离**（Shell/文件操作）：
- 非root用户 (uid 1000)
- 只读根文件系统
- 删除所有 Linux Capabilities
- 2GB内存限制 + CPU限额
- 无网络（除非通过代理）

**层3 — HTTP 代理网络隔离**：
- 所有HTTP请求必须通过代理
- URL白名单验证
- 凭证从不直接暴露给工具

**层4 — ToolRegistry 名称保护**：
- ~80个内置工具名受保护
- 动态注册时检查是否与内置名冲突，冲突则拒绝

**评价**：最完整的隔离体系，适合多租户/企业级场景，但复杂度最高。

---

### nanoclaw 的工具隔离

**核心设计**：Docker容器物理隔离 + IPC命名空间 + Mount allowlist。

**工具系统**（两类）：
1. **MCP工具**（容器内，MCP Server提供）：`send_message`, `schedule_task`, 等
2. **容器工具**：Claude Code标准工具（Bash, Glob, Grep, Read, Write等）

**隔离机制**：
- 每次运行生成独立Docker容器，运行完毕即销毁
- 每组独立的 `/home/node/.claude/` 目录（防止会话泄漏）
- IPC通过文件系统命名空间（`/workspace/ipc/{groupFolder}/`）
- 原子IPC写入（`.tmp` 临时文件 → `rename` 原子操作）
- Mount allowlist 在主机外部，容器无法修改

**权限设计**：
- `isMain` 特权组：可向任何组发送消息，可为任何组调度任务
- 普通组：只能操作自己的 IPC 命名空间

**评价**：设计简洁实用，物理隔离可靠，适合多租户多频道部署。

---

### 工具隔离对比总结

| 特性 | claude-code | zeroclaw | ironclaw | nanoclaw |
|------|------------|----------|----------|----------|
| 执行沙箱 | 可选（SandboxManager） | 可选（Docker/Landlock/Firejail） | WASM + Docker（默认） | Docker容器（默认） |
| 代码隔离 | ❌ | ❌ | ✅（WASM） | ❌ |
| 网络隔离 | 部分（路径验证） | ✅（域白名单） | ✅（HTTP代理） | ✅（容器无网络） |
| 凭证保护 | ✅（settings.json规则） | ✅（OTP+审计） | ✅（WASM边界注入） | ✅（OneCLI代理注入） |
| 并行工具 | ❌（顺序） | ✅（join_all并行） | ❌（顺序） | ❌（顺序） |
| 工具名保护 | ✅（条件编译） | ✅（allowed_tools白名单） | ✅（~80个保护名） | ✅（MCP命名空间） |
| 可扩展性 | 静态+条件编译 | Trait插拔（50+工具） | Trait+WASM动态 | SKILL.md + MCP |
| 嵌入式适配 | ✅ | ✅（<5MB RAM支持） | 较重 | ❌（依赖Docker） |

---

## 1.4 Skill 机制对比

| 特性 | claude-code | zeroclaw | ironclaw | nanoclaw |
|------|------------|----------|----------|----------|
| 格式 | Markdown + YAML frontmatter | TOML/Markdown（SKILL.toml 或 SKILL.md） | YAML frontmatter + Markdown | Markdown（SKILL.md） |
| 加载路径 | ~/.claude/skills/, .claude/skills/, bundled | ~/.zeroclaw/workspace/skills/ + open-skills社区 | workspace/skills/, user/~/.ironclaw/skills/, bundled | .claude/skills/ + container/skills/ |
| 信任模型 | loadedFrom字段（bundled/user/plugin/mcp/managed） | allow_scripts 安全审计 | SkillTrust（Installed=只读工具 / Trusted=完整权限） | isMain 特权 / 无强制隔离 |
| 激活方式 | 用户显式调用（/skill-name） | 配置化（系统提示注入） | 关键字/正则自动激活 + 分数排序 | 用户显式调用（/skill-name） |
| 参数传递 | {{arg}} 占位符替换 | TOML args 字段 | 嵌入提示 | 自由文本 |
| 自动创建 | ❌ | ✅（skill-creation特性） | ❌ | ❌ |
| 社区仓库 | ❌ | ✅（open-skills weekly sync） | ❌ | ❌ |
| 工具约束 | allowed-tools frontmatter | kind字段（shell/http/script） | 信任衰减控制工具集 | allowed-tools frontmatter |

---

# 二、调研报告：手机 Claw 技术难点分析

## 2.1 整体架构设想

Mobile Claw（手机上的智能体引擎）面向以下核心需求：

```
用户（手机App）
    ↓ 文字/语音输入
[Flutter UI层]
    ↓ SDK调用
[MobileClaw SDK（Dart/FFI）]
    ↓ 调用
[Rust Core Engine（WASM或Native）]
    ├── 规划引擎（Plan）
    ├── Memory 管理（SQLite本地）
    ├── Tool 注册/执行
    ├── Skill 加载/激活
    └── LLM 接口（Claude API）
```

核心问题：**手机环境的约束（沙箱、无Docker、低内存、电池）如何重新设计这些机制？**

---

## 2.2 难点一：Rust/WASM 在移动端的可行性

### 现状评估

**结论：可行，但需要分平台策略。**

**方案A：Rust 编译为 Native 库（推荐主路）**

```
Rust 代码
    ↓ cross-compile
Android: libmobileclaw.so (.so, aarch64-linux-android / armv7)
iOS:     libmobileclaw.a  (.a, aarch64-apple-ios)
    ↓ FFI
Flutter (dart:ffi)
    ↓ Dart bindings（用 ffigen 自动生成）
```

**优点**：
- 性能最优，接近原生
- 支持 Tokio 异步运行时
- Android/iOS 均有完善工具链（`cargo-ndk`, `cargo-lipo`）
- 可用 `flutter_rust_bridge` crate 自动生成类型安全的 Dart 绑定

**缺点**：
- 包体积较大（基础约3-8MB），需要 strip + LTO
- iOS 静态库需要通过 CocoaPods/Swift Package Manager 集成

**方案B：Rust 编译为 WASM（插件/Skill 沙箱用途）**

```
Skill/Tool 代码（Rust/其他）
    ↓ 编译为 .wasm
WasmRuntime（移动端）
```

**移动端 WASM 运行时选项**：

| 运行时 | iOS支持 | Android支持 | 包大小 | JIT | AOT | 备注 |
|--------|---------|-------------|--------|-----|-----|------|
| **wasmtime** | ⚠️（无JIT，纯解释） | ✅ | ~5MB | ❌（iOS禁止） | ✅ | ironclaw的选择，iOS需要解释模式 |
| **wasmer** | ⚠️（Cranelift禁止JIT） | ✅ | ~8MB | ❌ | ✅ | 类似wasmtime |
| **wasm3** | ✅ | ✅ | ~300KB | ❌ | ❌（解释器） | 最轻量，纯解释，适合移动端 |
| **wamr（WebAssembly Micro Runtime）** | ✅ | ✅ | ~300KB-1MB | 有限 | ✅（AoT需预编译） | Intel开源，最适合嵌入式/移动 |
| **V8/JavaScriptCore** | ❌（不适合WASM工具） | ❌ | - | - | - | 系统JS引擎，不适合工具沙箱 |

**iOS 关键限制**：
- **禁止 JIT（Just-In-Time 编译）**：iOS不允许运行时生成可执行内存页（App Store规则）
- 因此 wasmtime/wasmer 的 JIT 编译器后端在 iOS 上必须禁用
- 必须使用解释器模式（性能损失3-10x）或提前 AoT 编译

**推荐策略**：
- **主 Core（内存、规划、Agent循环）**：Rust Native（.so/.a），不走WASM
- **Skill/扩展工具**：WASM（用 wasm3 或 wamr，纯解释器，轻量安全）
- **原因**：Skill是用户扩展点，安全隔离必要；Core性能敏感，用Native

---

## 2.3 难点二：Skill 引入机制（移动端）

### 现有设计的问题

桌面端 Skill 依赖文件系统（`~/.claude/skills/`），移动端问题：
- iOS 沙箱严格限制文件系统访问，无"用户主目录"概念
- Android 外部存储权限繁琐
- 动态加载代码（.so/.dylib）在 iOS App Store **明确禁止**

### 移动端 Skill 方案

**方案A：WASM Skill 包（推荐）**

```
Skill 定义：
├── skill.yaml          # 元数据（name, description, activation, tools）
├── skill.md            # 提示内容（注入系统提示）
└── tools/
    └── my_tool.wasm    # 可选：工具实现（WASM字节码）
```

**存储位置**：
- iOS: `NSApplicationSupportDirectory/skills/`（沙箱内）
- Android: `context.filesDir/skills/`

**加载来源**：
1. **Bundle内置**：随App打包，零权限要求
2. **远程下载**：从 Skill 仓库（HTTPS）下载 `.skillpkg`（zip格式）
3. **本地文件**（Android）：用户选择器导入

**安全审计**：
- `skill.yaml` 中声明工具能力（capability声明制，类似Android权限）
- WASM 工具在 wasm3/wamr 中沙箱执行
- 下载的 Skill 需要代码签名验证
- Installed（下载的）< Trusted（内置的）信任衰减

**方案B：纯提示型 Skill（最简单，先行）**

只包含 Markdown 提示，没有 WASM 工具，完全靠 LLM 实现功能：
```yaml
# skill.yaml
name: code-review
description: 代码审查专家
activation:
  keywords: ["review", "代码审查", "check code"]
```
```markdown
# code-review

你是一个严格的代码审查专家...
```

**优势**：完全安全，无代码执行，适合初版

---

## 2.4 难点三：工具调用机制（移动端沙箱约束）

### 桌面工具 vs 移动端约束

| 工具类型 | 桌面 | 移动端问题 | 移动端方案 |
|---------|------|-----------|-----------|
| Bash/Shell | ✅完整支持 | ❌ iOS无shell，Android受限 | 预定义工具集（Rust实现），不支持任意Shell |
| 文件读写 | ✅任意路径 | ⚠️ 沙箱路径限制 | 限制在App沙箱目录，提供FileTools |
| HTTP请求 | ✅ | ✅（需要允许列表） | HttpTool + URL白名单 |
| grep/glob | ✅ | ✅（Rust实现） | 用Rust重写，作为Native工具 |
| 浏览器 | ✅ | ⚠️ 需要WKWebView/WebView | 通过系统WebView，受限 |
| Docker | ✅ | ❌ | 不支持，用WASM替代 |

### 工具分类设计

**三类工具**：

**类型1：内置 Native 工具**（Rust实现，编译进.so/.a）
```
- memory_search / memory_write / memory_read  （本地SQLite）
- http_request（URL白名单）
- file_read / file_write（沙箱目录）
- grep / glob（Rust实现）
- json / calc / time（无副作用）
- notification（系统通知）
- clipboard_read / clipboard_write
```

**类型2：WASM 扩展工具**（用户Skill提供，wasm3/wamr执行）
```
- 自定义数据处理逻辑
- 第三方API封装（通过http_request主机函数）
- 格式转换工具
```
**隔离边界**：
```
WASM Tool
  ↓ 只能调用宿主提供的函数
宿主函数（Rust实现）：
  - log(message) — 日志
  - http_fetch(url, method, body) — 经白名单验证的HTTP
  - memory_read(key) / memory_write(key, value) — 内存访问
  - return_result(json) — 返回结果
```

**类型3：Agent 工具**（通过LLM编排，无代码执行）
```
- 调用其他Agent（子任务委托）
- 调用另一个Claude API请求（分解子问题）
```

### 工具调用协议设计

借鉴 zeroclaw 的 XML 格式或 ironclaw 的 JSON Schema，推荐：

```
LLM 输出（XML格式，避免JSON解析问题）：
<tool_call>
{"name": "http_request", "args": {"url": "https://...", "method": "GET"}}
</tool_call>

工具执行后返回：
<tool_result name="http_request" status="ok">
{"status": 200, "body": "..."}
</tool_result>
```

---

## 2.5 难点四：通过 Rust 扩展工具的机制

### 扩展模型设计

**Rust Tool Trait**（类似 zeroclaw/ironclaw）：

```rust
// mobileclaw-sdk/src/tools/trait.rs

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError>;

    // 移动端特有
    fn required_permissions(&self) -> Vec<Permission> { vec![] }
    fn is_sandboxed(&self) -> bool { true }
    fn timeout_ms(&self) -> u64 { 5000 }
}

pub struct ToolContext {
    pub memory: Arc<dyn Memory>,
    pub http_client: Arc<dyn HttpClient>,   // 白名单验证的HTTP
    pub sandbox_dir: PathBuf,               // 工具可写的沙箱目录
}
```

**Tool 注册表**：

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    protected_names: HashSet<String>,       // 不可覆盖的内置工具名
    permission_checker: Arc<dyn PermissionChecker>,
}

impl ToolRegistry {
    pub fn register_builtin(&mut self, tool: Arc<dyn Tool>) { /* 无检查 */ }

    pub fn register_extension(&mut self, tool: Arc<dyn Tool>) -> Result<()> {
        if self.protected_names.contains(tool.name()) {
            return Err(ToolError::NameConflict);
        }
        self.tools.insert(tool.name().to_string(), tool);
        Ok(())
    }

    pub fn register_wasm_tool(&mut self, wasm_bytes: &[u8], manifest: SkillManifest) -> Result<()> {
        // 编译为 wasm3/wamr 模块
        let tool = WasmTool::new(wasm_bytes, manifest)?;
        self.register_extension(Arc::new(tool))
    }
}
```

**Flutter 侧调用**（通过 flutter_rust_bridge）：

```dart
// 自动生成的 Dart 绑定
final result = await MobileClawApi.executeTool(
  name: 'http_request',
  args: {'url': 'https://api.example.com', 'method': 'GET'},
);
```

---

## 2.6 难点五：Memory 持久化（移动端）

### 推荐方案：SQLite + Markdown 混合（借鉴 zeroclaw SQLite 方案）

**为什么不用 PostgreSQL / Qdrant**：
- 移动端不能运行数据库服务器
- 依赖包体积约束
- 离线优先要求

**为什么 SQLite 适合**：
- iOS/Android 均有原生 SQLite 支持
- zeroclaw 已证明 SQLite 可实现 FTS5 + 向量混合搜索
- WAL 模式支持并发读写
- 数据库文件在应用沙箱目录，无需额外权限

**Mobile Memory 设计**：

```
memory/
├── memory.db            # SQLite（主存储）
│   ├── documents        # MemoryDocument（路径, 内容, 时间戳）
│   ├── chunks           # MemoryChunk（文本块, 向量嵌入）
│   └── sessions         # 会话ID映射
└── exports/             # 可选：Markdown导出（用于调试/备份）
```

**向量嵌入方案**：
- 本地嵌入（无需网络）：`fastembed-rs`（Rust，约100MB模型）或 `ort`（ONNX Runtime）
- 远程嵌入（需要网络）：调用 Claude API 或 OpenAI API
- 初版可无向量，只用 FTS5 全文搜索

**Memory 容量管理**：
- 移动端存储珍贵，需要清理策略
- 建议：chunks 按 LRU 清理，documents 保留最新N篇
- 用户可手动清理

---

## 2.7 难点六：Flutter SDK 封装方案

### 整体分层

```
┌─────────────────────────────────────────┐
│              Flutter App                │
│  ┌──────────────────────────────────┐  │
│  │     mobileclaw_flutter (Dart)    │  │  ← 上层SDK（用户使用）
│  │  MobileClaw, SkillManager, etc.  │  │
│  └──────────────┬───────────────────┘  │
│                 │ dart:ffi              │
│  ┌──────────────▼───────────────────┐  │
│  │    mobileclaw_core (Rust/Native) │  │  ← 核心引擎
│  │  Agent Loop, Memory, Tools, LLM  │  │
│  └──────────────────────────────────┘  │
│                                         │
│  iOS: libmobileclaw.a (静态库)          │
│  Android: libmobileclaw.so (动态库)     │
└─────────────────────────────────────────┘
```

**Dart API 设计**（SDK用户侧）：

```dart
// 初始化
final claw = await MobileClaw.init(
  apiKey: 'sk-...',
  skillsDir: appDocDir.path + '/skills',
  memoryDir: appDocDir.path + '/memory',
);

// 注册工具
claw.registerTool(HttpTool(allowedDomains: ['api.example.com']));
claw.registerWasmTool(wasmBytes, manifest: SkillManifest(...));

// 加载 Skill
await claw.loadSkillsFromDirectory(skillsDir);

// 执行任务
final stream = claw.chat('帮我查一下明天的天气');
await for (final event in stream) {
  switch (event) {
    case TextEvent(text: final t): print(t);
    case ToolCallEvent(tool: final name): print('执行工具: $name');
    case DoneEvent(): break;
  }
}
```

**关键 Flutter 集成细节**：

```
工具链：
- flutter_rust_bridge 2.x：自动生成 Dart ↔ Rust 绑定
- cargo-ndk：Android cross-compile
- cargo-lipo：iOS universal binary
- build.rs：编译时生成绑定

异步模型：
- Rust → Dart：通过 StreamSink<DartDynamic> 推送事件
- Dart → Rust：通过生成的异步函数调用
- 工具执行：在 Rust Tokio 线程池中，不阻塞 Flutter UI

线程隔离：
- Flutter UI 线程：只调用 SDK API
- Rust Tokio 运行时：独立线程池处理 Agent 循环 + 工具执行
- WASM工具：在 Tokio 任务中执行（非 UI 线程）
```

---

## 2.8 难点汇总与推荐方案

### 技术选型推荐

| 模块 | 推荐方案 | 备选 | 理由 |
|------|---------|------|------|
| **Core 引擎** | Rust Native (.so/.a) | ❌ | 性能最优，离线可用 |
| **Flutter 绑定** | flutter_rust_bridge 2.x | dart:ffi 手动 | 自动生成，类型安全 |
| **Skill 执行** | wasm3 或 wamr（解释器） | wasmtime（Android） | iOS兼容，轻量（300KB-1MB） |
| **Memory 存储** | SQLite + FTS5（rusqlite） | 纯Markdown | 搜索能力，成熟稳定 |
| **向量嵌入** | 初版无向量（FTS5），后期 fastembed-rs | ONNX Runtime | 体积控制，按需引入 |
| **工具协议** | XML tool_call 格式 | JSON（易被LLM在内容中误写） | 借鉴 zeroclaw，解析健壮 |
| **Skill 格式** | YAML frontmatter + Markdown | TOML | 与 claude-code/ironclaw 一致，生态兼容 |
| **HTTP 工具** | reqwest（Rust） + URL白名单 | - | 安全控制必要 |
| **任务规划** | 参考 nanoclaw：cron/interval/once | 完整SOP（zeroclaw） | 初版足够，可迭代 |

### 分阶段实施路径

**阶段1（MVP）**：
- Rust Core：Agent循环 + SQLite Memory（FTS5）+ 内置工具集（HTTP/文件/grep）
- 纯提示型 Skill（只有 Markdown，无 WASM 工具）
- flutter_rust_bridge 绑定
- 基础对话功能

**阶段2（扩展）**：
- WASM 工具支持（wasm3/wamr）
- Skill 包管理（下载/安装/签名验证）
- 向量嵌入（本地 fastembed-rs 或远程API）
- 任务调度（cron/interval）

**阶段3（完整）**：
- 多 Agent 协作
- Skill 社区仓库
- 本地模型支持（llama.cpp/mlc-llm）
- 语音输入

---

## 2.9 关键风险与缓解

| 风险 | 严重程度 | 缓解措施 |
|------|---------|---------|
| iOS 禁止 JIT，WASM解释器性能差 | 中 | 使用 wamr AoT 预编译 + 限制工具复杂度 |
| Rust 二进制包大（+5-15MB） | 中 | LTO + strip + 按feature选择性编译 |
| App Store 审核：动态代码加载 | 高 | WASM字节码不属于"原生代码"，已有先例（多款App使用） |
| SQLite 并发写冲突 | 低 | WAL模式 + Tokio Mutex 序列化写入 |
| 内存不足（低端机） | 中 | 限制Agent循环的消息历史窗口 + 懒加载Skill |
| Claude API 延迟（网络差） | 中 | 流式输出 + 超时重试 + 本地模型备用 |
