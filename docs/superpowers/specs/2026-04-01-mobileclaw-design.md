# MobileClaw 设计规范文档

**版本**: 0.3.0
**日期**: 2026-04-01
**状态**: 已审查并修订（修复 C1-C4 / M1-M8 / m1-m6 / R2-1 至 R2-12）

---

## 目录

1. [项目概述](#1-项目概述)
2. [核心原则](#2-核心原则)
3. [整体架构](#3-整体架构)
4. [安全边界模型](#4-安全边界模型)
5. [Memory 管理系统](#5-memory-管理系统)
6. [工具系统](#6-工具系统)
7. [Skill 系统](#7-skill-系统)
8. [Agent 循环](#8-agent-循环)
9. [LLM 提供商抽象](#9-llm-提供商抽象)
10. [媒体处理管道](#10-媒体处理管道)
11. [存储层](#11-存储层)
12. [后台任务调度](#12-后台任务调度)
13. [Flutter 集成与 UI 架构](#13-flutter-集成与-ui-架构)
14. [SDK 化架构](#14-sdk-化架构)
15. [项目目录结构](#15-项目目录结构)
16. [Rust Crate 依赖图](#16-rust-crate-依赖图)
17. [编译与打包](#17-编译与打包)
18. [测试策略](#18-测试策略)
19. [性能目标](#19-性能目标)
20. [版本路线图](#20-版本路线图)
21. [错误代码目录](#21-错误代码目录)

---

## 1. 项目概述

MobileClaw 是一个跨平台 AI Agent SDK，以 Flutter + Rust 实现，面向移动平台（iOS/Android），兼容桌面（Linux/macOS/Windows，仅用于开发测试）。

### 核心定位

- **个人 AI 助手引擎**：处理用户的文字、图片、视频输入，自主规划并完成多步骤任务
- **严格沙箱隔离**：不访问手机其他文件，不调用其他程序，所有能力来自内置工具或 Skill 扩展
- **SDK 优先**：整体设计为可嵌入其他 Flutter App 的 SDK，演示 App 是引用 SDK 的壳

### 技术栈

| 层次 | 技术 |
|------|------|
| UI | Flutter 3.41.5 (Dart) |
| 状态管理 | Riverpod |
| FFI 绑定 | flutter_rust_bridge 2.x |
| 核心引擎 | Rust 1.94+ (2021 edition) |
| 异步运行时 | Tokio |
| 数据库 | SQLite (rusqlite + FTS5) |
| WASM 沙箱 | wamr (Android AoT) / wasm3 (iOS 解释器) |
| 媒体处理 | image-rs / platform-native-video / whisper-rs |
| HTTP | reqwest (TLS, 证书固定) |
| 云备份 | WebDAV (reqwest_dav) |
| LLM | Claude API (Anthropic), 可扩展 |

---

## 2. 核心原则

### P1：极致性能
- Rust 零成本抽象，关键路径无动态分配
- 流式输出，首字节延迟 < 500ms
- SQLite WAL 模式，写入不阻塞读取
- 所有媒体处理在 Tokio 线程池，不阻塞 UI

### P2：安全第一
- 5 条强制安全边界，每条有独立验证逻辑
- WASM 沙箱隔离第三方 Skill 工具代码
- 最小权限原则：每个模块只持有必要的依赖
- 不信任输入：LLM 输出、Skill 包、WebDAV 数据均视为不可信

### P3：模块化 SDK
- Rust workspace 多 crate，每个 crate 可独立测试和发布
- Flutter Plugin Package，可通过 pubspec.yaml 引入
- UI 组件可选：使用者可只用逻辑层，自定义 UI

### P4：透明可控
- 所有后台任务对用户可见，可随时取消
- 工具调用过程在 UI 中实时展示
- Memory 操作有审计日志

---

## 3. 整体架构

```
┌──────────────────────────────────────────────────────────────────┐
│                        Flutter App Layer                          │
│  聊天界面 / 设置 / Skill管理 / 后台任务面板 / Media输入           │
│  (mobileclaw_app — 演示 App，引用 mobileclaw_sdk)                 │
└──────────────────────────┬───────────────────────────────────────┘
                           │ pubspec dependency
┌──────────────────────────▼───────────────────────────────────────┐
│                    mobileclaw_sdk (Flutter Plugin)                 │
│  ClawEngine / ClawEvent / Models / 可选 UI Widgets                │
└──────────────────────────┬───────────────────────────────────────┘
                           │ flutter_rust_bridge (类型安全 FFI)
┌──────────────────────────▼───────────────────────────────────────┐
│                      claw_ffi (Rust)                              │
│  唯一 Dart↔Rust 边界；全量参数验证；无 unsafe 外露                │
└──────────────────────────┬───────────────────────────────────────┘
                           │
┌──────────────────────────▼───────────────────────────────────────┐
│                  Rust Workspace: mobileclaw                        │
│                                                                    │
│   claw_core          ←─ 核心 Agent 循环、规划、熔断               │
│   claw_memory        ←─ Memory 管理 (MEMORY.md 体系)             │
│   claw_tools         ←─ 内置工具集 (http/file/db)                │
│   claw_skills        ←─ Skill 加载、信任模型、激活                │
│   claw_wasm_runtime  ←─ WASM 沙箱 (wamr/wasm3)                  │
│   claw_llm           ←─ LLM Provider Trait + Claude 实现         │
│   claw_media         ←─ 图片/视频帧/ASR 处理                     │
│   claw_storage       ←─ SQLite + 文件 + WebDAV                   │
│   claw_scheduler     ←─ 后台任务调度器                           │
└──────────────────────────┬───────────────────────────────────────┘
                           │ OS 沙箱边界
┌──────────────────────────▼───────────────────────────────────────┐
│  系统资源（受限访问）                                              │
│  App 数据目录 / HTTPS 网络 / 系统相册（用户授权）                  │
│  ✗ 其他 App 数据  ✗ 通讯录  ✗ 位置  ✗ 后台麦克风               │
└──────────────────────────────────────────────────────────────────┘
```

---

## 4. 安全边界模型

### 五条强制安全边界

#### B1：Dart → Rust（claw_ffi）
**威胁**：Flutter 传入恶意参数，绕过 Rust 逻辑
**措施**：
- 所有字符串长度上限检查（路径 ≤4096，消息 ≤1MB）
- 拒绝包含 null 字节的字符串
- 枚举型参数只接受已知变体
- Path 参数拒绝包含 `..` 的原始输入
- 返回值序列化为 JSON，不暴露 Rust 内部类型

#### B2：Rust → LLM API
**威胁**：API Key 泄露，请求内容包含本地敏感路径，中间人攻击
**措施**：
- API Key 由 `claw_ffi::keystore::read_secret(alias)` 在 Rust 层直接读取，不经 Dart
- **SPKI Hash Pinning**（而非固定根证书，以支持证书轮转）：
  ```rust
  // 固定 Anthropic 叶证书 (leaf certificate) 的 SPKI SHA-256 Hash
  // （维护 2-3 个备用 pin，证书轮转前服务端通过 CertPinUpdate 事件推送新 pin）
  // 注：叶证书 pin 随证书续期更新（一般 1-2 年），比中间 CA 更频繁，
  //     但 pin 精度更高、攻击面更小，适合单一 API 服务场景。
  const ANTHROPIC_SPKI_PINS: &[&str] = &[
      "sha256/PRIMARY_SPKI_HASH_BASE64==",
      "sha256/BACKUP_SPKI_HASH_1_BASE64==",
      "sha256/BACKUP_SPKI_HASH_2_BASE64==",
  ];
  // 使用 reqwest + rustls，在 TLS handshake 回调中验证 SPKI Hash（见 §9 SpkiPinVerifier）
  // FfiConfig.disable_cert_pinning = true 仅供 enterprise MDM 场景
  ```
- `FfiEvent::CertPinUpdate { new_pins, signature }` 支持服务端推送更新 pin（证书轮转前触发，见 §13 FfiEvent）
- 请求体扫描：拒绝包含绝对路径（`/home/...`、`/data/...`）的消息
- 仅允许 HTTPS，TLS 1.2+，明确拒绝 SSLv3/TLS 1.0/1.1

#### B3：Rust → 文件系统（path jail）
**威胁**：路径穿越攻击（`../../etc/passwd`）、TOCTOU 竞态条件（symlink race）、访问 App 沙箱外文件
**措施**：
```rust
// 使用 cap-std crate 提供基于能力（capability-based）的文件系统访问
// 彻底消除 TOCTOU：所有操作通过 openat() 在已打开的目录 fd 上完成
// 不存在"检查时刻"和"使用时刻"之间的窗口

use cap_std::fs::Dir;
use cap_std::ambient_authority;

fn open_jailed_dir(root: &Path) -> cap_std::io::Result<Dir> {
    Dir::open_ambient_dir(root, ambient_authority())
}

// 所有文件操作通过 Dir::open_file / Dir::create / Dir::metadata 进行
// cap-std 在内部使用 O_NOFOLLOW + openat()，符号链接无法逃逸沙箱
```
- **禁止使用** `std::fs::canonicalize()` + 后续 `open()`（经典 TOCTOU）
- 全部文件操作通过 `cap_std::fs::Dir` 完成，不暴露 `PathBuf`
- 符号链接：`Dir::open_file` 默认拒绝越界符号链接（`O_NOFOLLOW`）
- 写操作单文件大小上限 50MB

#### B4：Rust → WebDAV
**威胁**：凭证泄露，中间人攻击，恶意 WebDAV 服务器推送恶意文件
**措施**：
- WebDAV URL、用户名、密码加密存储（同 B2 Keystore）
- 仅允许 HTTPS WebDAV（拒绝 http://）
- 下载文件大小上限 100MB
- 下载的 Skill 包需经独立签名验证流程（非直接信任 WebDAV 内容）

#### B5：Rust → WASM Tool（最关键）
**威胁**：第三方 Skill 代码访问敏感数据、发起任意网络请求、消耗过多资源
**措施**：
- WASM 实例无直接文件系统访问（WASI 禁用）
- WASM 实例无原生网络（只能通过宿主白名单函数发起 HTTP）
- 每次执行创建新实例（无持久全局状态）
- 燃料计量（Fuel Metering）：CPU 周期上限防死循环
- 内存上限：8MB per 实例
- 宿主白名单函数完整 ABI（WASM 侧仅能 import 以下函数，加载时拒绝任何其他 import）：
  ```
  wasm_log(level: i32, msg_ptr: i32, msg_len: i32)

  // HTTP — 无 header 参数（防 header 注入），宿主只添加 Content-Type
  // method 编码：0=GET, 1=POST, 2=PUT, 3=DELETE, 4=PATCH
  //   其他值 → 宿主立即返回负 handle（WASM 侧应视为错误）
  wasm_http_send(url_ptr: i32, url_len: i32, method: i32,
                 body_ptr: i32, body_len: i32) -> request_handle: i32
  wasm_http_ready(handle: i32) -> i32   // 0=pending 1=done -1=error
  wasm_http_status(handle: i32) -> i32  // HTTP 状态码
  wasm_http_body_len(handle: i32) -> i32
  wasm_http_body_read(handle: i32, out_ptr: i32, out_len: i32) -> bytes_written: i32
  wasm_http_free(handle: i32)           // 释放宿主资源

  // Memory 只读（key 必须在 manifest.permissions.memory 声明，否则 -1）
  wasm_memory_get(key_ptr: i32, key_len: i32, out_ptr: i32, out_max: i32) -> i32

  // 工具输出（调用一次即结束）
  wasm_result(json_ptr: i32, json_len: i32)
  wasm_error(code_ptr: i32, code_len: i32, msg_ptr: i32, msg_len: i32)
  ```
- `wasm_http_send` 调用时在宿主侧验证域名白名单，非法 URL 返回负 handle
- 输出经泄漏检测（扫描 API Key 模式、绝对路径）后返回给 Agent

### 安全审计日志

所有边界违规写入 SQLite `audit_log` 表（以下为权威定义，§5 Schema 中同步）：
```sql
CREATE TABLE audit_log (
    id       INTEGER PRIMARY KEY,
    ts       INTEGER NOT NULL,          -- Unix timestamp
    category TEXT NOT NULL DEFAULT 'security',
                                        -- 'security'：B1-B5 边界违规（需即时告警）
                                        -- 'operational'：LLM/TOOL/SKILL 运行时错误（统计用）
    boundary TEXT NOT NULL,             -- B1..B5（安全类）或错误前缀如 LLM_/TOOL_（运行时类）
    event    TEXT NOT NULL,             -- 事件类型，对应 §21 错误代码
    detail   TEXT,                      -- JSON 详情
    blocked  INTEGER NOT NULL DEFAULT 0 -- 1=已拦截，0=已记录但未拦截
);
CREATE INDEX idx_audit_ts       ON audit_log(ts);
CREATE INDEX idx_audit_category ON audit_log(category, ts);
```

---

## 5. Memory 管理系统

### 设计哲学

完全参考 claude-code 的 Memory 机制：**Memory 是写给未来自己的笔记**，结构化、类型化、跨会话持久，通过 MEMORY.md 索引入口注入系统提示。

### 文件系统布局

```
{data_dir}/claw/
├── memory/
│   ├── MEMORY.md                  ← 索引入口（硬限制 ≤200行/≤25KB）
│   ├── user_{name}.md             ← 用户偏好、背景、知识
│   ├── feedback_{name}.md         ← 行为规则（含 Why + How to apply）
│   ├── project_{name}.md          ← 项目上下文（绝对日期格式）
│   └── reference_{name}.md        ← 外部资源指针
├── memory.db                      ← SQLite（FTS5 + 向量块索引）
├── sessions/
│   └── {session_id}.jsonl         ← 对话历史
├── conversations/
│   └── {date}-{slug}.md           ← 归档对话（compact 时生成）
├── skills/
│   └── {skill_name}/              ← 已安装 Skill
├── workspace/                     ← 工具可写沙箱目录
├── temp/                          ← 临时文件（会话结束清理）
├── models/                        ← 按需下载的模型（Whisper 等）
└── plans/
    └── active_plan.md             ← Plan Mode 活跃计划（执行完删除）
```

### Memory 四类型系统

| 类型 | 用途 | 保存时机 | Frontmatter type 值 |
|------|------|---------|---------------------|
| `user` | 用户角色、偏好、知识背景、技能水平 | 学到用户信息时 | `user` |
| `feedback` | 行为规则（做什么/不做什么）+ Why + How to apply | 用户纠正或明确确认时 | `feedback` |
| `project` | 正在进行的工作、目标、截止日期（相对日期转绝对） | 学到项目状态时 | `project` |
| `reference` | 外部系统资源指针（URL、频道、仓库） | 学到外部资源位置时 | `reference` |

### Memory 文件 Frontmatter 格式

```yaml
---
name: 简短名称（≤50字符）
description: 一句话描述（≤150字符，用于 MEMORY.md 索引行）
type: user | feedback | project | reference
created_at: 2026-04-01T10:00:00Z    # ISO 8601 UTC
updated_at: 2026-04-01T10:00:00Z
---
正文内容

# feedback 类型必须包含：
**Why:** 原因说明
**How to apply:** 应用场景和边界条件
```

### MEMORY.md 索引格式

```markdown
- [用户是后端工程师](user_role.md) — 10年Go经验，首次接触Flutter
- [不要在响应末尾总结](feedback_no_summary.md) — 用户明确反感冗余输出
- [MobileClaw 项目目标](project_mobileclaw.md) — MVP 
- [天气 API](reference_weather_api.md) — wttr.in 无需 Key，JSON 格式
```

**硬性约束**：
- 行数 ≤200 行（超出截断并追加警告行）
- 字节数 ≤25,600 bytes（25KB，超出在最后换行处截断）
- 每行格式：`- [标题](file.md) — 钩子文字`（钩子 ≤150 字符）
- MEMORY.md 本身无 frontmatter

### SQLite 数据库 Schema

```sql
-- 文档元数据（与文件系统对应）
CREATE TABLE memory_docs (
    id        INTEGER PRIMARY KEY,
    path      TEXT NOT NULL UNIQUE,  -- 相对路径，如 "user_role.md"
    name      TEXT NOT NULL,
    type      TEXT NOT NULL,         -- user/feedback/project/reference
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- FTS5 全文检索（BM25 评分）
CREATE VIRTUAL TABLE memory_fts USING fts5(
    path, name, description, content,
    content=memory_docs_content,
    content_rowid=rowid,
    tokenize='unicode61'
);

-- 文档内容（与 FTS 关联）
CREATE TABLE memory_docs_content (
    rowid INTEGER PRIMARY KEY,
    path  TEXT NOT NULL,
    content TEXT NOT NULL
);

-- 向量块（语义搜索，可选，初版 embedding 列为 NULL）
CREATE TABLE memory_chunks (
    id        INTEGER PRIMARY KEY,
    doc_path  TEXT NOT NULL,
    chunk_idx INTEGER NOT NULL,
    content   TEXT NOT NULL,
    embedding BLOB,              -- f32 数组，NULL 表示未计算嵌入
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_chunks_path ON memory_chunks(doc_path);
CREATE INDEX idx_chunks_doc ON memory_chunks(doc_path, chunk_idx);

-- 审计日志（权威定义见 §4；此处与 §4 保持一致）
CREATE TABLE audit_log (
    id       INTEGER PRIMARY KEY,
    ts       INTEGER NOT NULL,
    category TEXT NOT NULL DEFAULT 'security',  -- 'security' | 'operational'
    boundary TEXT NOT NULL,
    event    TEXT NOT NULL,
    detail   TEXT,
    blocked  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_audit_ts       ON audit_log(ts);
CREATE INDEX idx_audit_category ON audit_log(category, ts);

-- 后台任务
CREATE TABLE scheduled_tasks (
    id          TEXT PRIMARY KEY,    -- UUID
    name        TEXT NOT NULL,
    prompt      TEXT NOT NULL,
    schedule    TEXT NOT NULL,       -- JSON: {type, value}
    context_mode TEXT NOT NULL,      -- "group" | "isolated"
    status      TEXT NOT NULL,       -- "active" | "paused" | "completed"
    next_run_at INTEGER NOT NULL,    -- Unix timestamp
    last_run_at INTEGER,
    created_at  INTEGER NOT NULL
);
CREATE INDEX idx_tasks_next ON scheduled_tasks(next_run_at, status);

-- 已安装 Skill
CREATE TABLE installed_skills (
    name        TEXT PRIMARY KEY,
    version     TEXT NOT NULL,
    trust_level TEXT NOT NULL,       -- "bundled" | "verified" | "community" | "local"
    permissions TEXT NOT NULL,       -- JSON
    installed_at INTEGER NOT NULL,
    checksum    TEXT NOT NULL,       -- SHA-256 of .skillpkg
    enabled     INTEGER NOT NULL DEFAULT 1  -- 0=disabled，禁用时不激活但不删除文件
);
```

### SQLite Schema 迁移策略

使用 SQLite 内置的 `PRAGMA user_version` 追踪 schema 版本：

```rust
// claw_storage/src/schema.rs
pub const CURRENT_VERSION: u32 = 1;

// migrate() 要求 &mut Connection 以支持 transaction()
pub fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for v in version..CURRENT_VERSION {
        // 每步迁移独立包裹在事务中：失败时只回滚当前步骤，user_version 不前进
        let tx = conn.transaction()?;
        match v {
            0 => migration_v0_to_v1(&tx)?,
            // 未来版本在此添加
            _ => unreachable!(),
        }
        // user_version 在事务内更新，commit 后才生效
        // 下次打开数据库时从正确的 version 继续，避免重复执行已完成的迁移
        tx.pragma_update(None, "user_version", v + 1)?;
        tx.commit()?;
    }
    Ok(())
}
```

规则：
- `LocalStorage::open()` 在任何其他操作前运行 `migrate()`
- 每次迁移在事务内执行（失败时回滚）
- 迁移文件命名：`migration_v{n}_to_v{n+1}.sql`，存储于 `claw_storage/migrations/`
- 生产版本只允许向前迁移（不支持降级）

**SQLite PRAGMA 配置**（性能优化）：
```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA mmap_size = 8388608;      -- 8MB mmap
PRAGMA cache_size = -2000;       -- 约 2MB 页缓存
PRAGMA temp_store = MEMORY;
PRAGMA foreign_keys = ON;
```

### Memory Rust API

```rust
// claw_memory/src/lib.rs

pub struct MemoryManager {
    root: PathBuf,           // jail 根目录
    db: Arc<Database>,       // claw_storage 提供
}

impl MemoryManager {
    /// 会话启动时加载，注入系统提示
    pub async fn load_prompt(&self) -> Result<String>;

    /// 写入一条 Memory（自动更新 MEMORY.md 索引）
    pub async fn save(&self, entry: MemoryEntry) -> Result<()>;

    /// FTS5 全文检索
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;

    /// 读取单个 Memory 文件
    pub async fn read(&self, path: &str) -> Result<MemoryDoc>;

    /// 列出所有 Memory（按类型过滤）
    pub async fn list(&self, type_filter: Option<MemoryType>) -> Result<Vec<MemoryMeta>>;

    /// 删除一条 Memory（同步更新索引）
    pub async fn delete(&self, path: &str) -> Result<bool>;

    /// MEMORY.md 索引维护（截断、去重、重建）
    pub async fn reindex(&self) -> Result<IndexStats>;
}

pub struct MemoryEntry {
    pub path: String,            // 如 "user_role.md"（自动加前缀）
    pub name: String,
    pub description: String,
    pub type_: MemoryType,
    pub content: String,
}

pub enum MemoryType { User, Feedback, Project, Reference }
```

### 后台 Memory 压缩 Agent

```
触发条件：
  · 对话结束（post-processing hook）
  · 每日 02:00 定时任务
  · 用户手动触发（设置 → 整理 Memory）

执行流程：
  1. 读取最近 K 条对话（sessions/*.jsonl，K=5 默认）
  1a. **安全过滤**：对每条 session 内容：
      - 剥除所有 `tool_result` 消息块（只保留 user/assistant 文字，工具调用结果不发给 LLM）
      - 对剩余内容运行 `LeakDetector`（扫描 API Key 模式、绝对路径、信用卡号等）
      - 超过 LeakDetector 阈值的消息整条排除
  2. 调用 LLM："从以下对话中提取值得长期记忆的内容，
     按类型（user/feedback/project/reference）分类，
     以 JSON 数组返回，每条包含 type/name/description/content"
  3. 对每条候选：
     a. FTS5 查询 MEMORY.md 中是否已有相似条目
     b. 相似度 < 阈值 → 写入新 .md 文件 + 更新索引
     c. 相似度 ≥ 阈值 → 调用 LLM 判断是否需要更新
  4. MEMORY.md 行数检查 → 超 180 行时触发精简：
     删除 updated_at 超过 90 天且 feedback 类型外的低价值条目
  5. 压缩 sessions/*.jsonl → 归档为 conversations/{date}-{slug}.md
  6. 向 Flutter 推送 MemoryUpdated 事件
```

---

## 6. 工具系统

### 工具分层

```
Layer 0: System Tools（Rust Native，最高信任，无沙箱开销）
  memory_read, memory_write, memory_search, memory_list
  file_read, file_write, file_delete, file_list, file_search
  db_query, db_execute
  http_request
  time, json_parse, json_format, hash_sha256, base64_encode, base64_decode
  notify（系统通知）

Layer 1: Skill Native Tools（Rust Native，中等信任，Skill Store 官方认证）
  [未来扩展]

Layer 2: WASM Extension Tools（WASM 沙箱，最低信任，Skill 包提供）
  [第三方 Skill 的自定义逻辑]
```

### Tool Trait

```rust
// claw_tools/src/trait_.rs

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn parameters_schema(&self) -> &'static str;  // JSON Schema（静态字符串）
    fn trust_level(&self) -> TrustLevel;
    fn timeout_ms(&self) -> u64 { 5_000 }
    fn max_output_bytes(&self) -> usize { 1_048_576 }  // 1MB

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError>;
}

pub struct ToolContext {
    /// 工具沙箱目录（cap-std Dir 句柄，已通过 openat 打开，所有子操作继承 O_NOFOLLOW）
    /// 使用 Arc<Dir> 而非 PathBuf，确保工具实现无法绕过 cap-std 进行任意路径访问。
    /// 工具应仅调用 Dir::open_file / Dir::create / Dir::read_dir 等方法，不得持有 PathBuf。
    pub sandbox_dir: Arc<cap_std::fs::Dir>,
    pub http: Arc<HttpClient>,          // 白名单 HTTP 客户端
    /// Memory 访问（只应调用 load_prompt / search / read / list 等只读方法）
    /// 使用 Arc<MemoryManager>（不单独定义 MemoryReader 接口，YAGNI）
    pub memory: Arc<MemoryManager>,
    pub db: Arc<Database>,              // SQLite 访问（db_query 专用）
}

pub struct ToolOutput {
    pub content: serde_json::Value,
    pub duration_ms: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    Wasm   = 0,
    Native = 1,
    System = 2,
}
```

### 内置工具安全规范

#### `http_request`
```
参数：
  url: string（必须 HTTPS，域名在白名单内）
  method: "GET" | "POST" | "PUT" | "DELETE"
  headers: object（禁止 Authorization 覆盖，防止 SSRF 伪造）
  body: string（可选）
  timeout_ms: number（可选，上限 30000）

安全检查：
  · URL 解析后域名必须在用户配置的白名单内
  · 禁止 localhost/127.0.0.1/0.0.0.0/::1
  · 禁止 192.168.x.x/10.x.x.x/172.16-31.x.x（内网防护）
  · 响应体大小上限 10MB，超出则截断并标记
  · 响应经 LeakDetector 扫描后返回

返回：
  { status: number, headers: object, body: string }
```

#### `file_read` / `file_write` / `file_list`
```
参数：
  path: string（相对于 workspace/ 的路径）

安全检查（file_read/write）：
  · jail(workspace_dir, path) 验证
  · 拒绝 .. 穿越
  · file_write 单文件上限 50MB
  · 拒绝写入 .db / .jsonl 后缀（保护数据库和会话文件）

安全检查（file_list）：
  · 只列出 workspace/ 目录，不递归超过 3 层
```

#### `db_query`
```
参数：
  sql: string（只允许 SELECT 语句）
  params: array（绑定参数，防 SQL 注入）

安全检查：
  · AST 解析验证为 SELECT（非正则，防绕过）
  · 禁止 SELECT ... FROM sqlite_master（防止 schema 泄露）
  · 结果行数上限 1000
  · 使用 prepared statement，params 绑定
```

### ToolRegistry

```rust
pub struct ToolRegistry {
    system_tools: IndexMap<&'static str, Arc<dyn Tool>>,
    skill_tools:  HashMap<String, Arc<dyn Tool>>,
    protected:    HashSet<&'static str>,  // system tool 名称集合
}

impl ToolRegistry {
    /// 注册 Skill 提供的工具（不得覆盖 system 工具）
    /// 命名规则：skill.{skill_name}.{tool_name}（三段式，防止跨 Skill 冲突）
    pub fn register_skill_tool(
        &mut self,
        skill_name: &str,
        tool: Arc<dyn Tool>,
    ) -> Result<(), RegistryError> {
        if self.protected.contains(tool.name()) {
            return Err(RegistryError::ProtectedName(tool.name().to_string()));
        }
        let namespaced = format!("skill.{}.{}", skill_name, tool.name());
        self.skill_tools.insert(namespaced, tool);
        Ok(())
    }

    /// 查找工具（system 优先于 skill）
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.system_tools.get(name).cloned()
            .or_else(|| self.skill_tools.get(name).cloned())
    }

    /// 当前会话可用工具列表（供 LLM 参考）
    pub fn specs(&self) -> Vec<ToolSpec>;
}
```

### LLM 工具调用协议（XML）

```xml
<!-- LLM 输出中的工具调用 -->
<tool_call id="tc_001">
{"name":"http_request","args":{"url":"https://wttr.in/Beijing?format=j1","method":"GET"}}
</tool_call>

<!-- 执行结果返回给 LLM -->
<tool_result id="tc_001" status="ok" duration_ms="312">
{"status":200,"body":"{\"current_condition\":[{\"temp_C\":\"22\"}]}"}
</tool_result>

<!-- 执行失败 -->
<tool_result id="tc_001" status="error" duration_ms="5001">
{"error":"timeout","code":"TOOL_TIMEOUT"}
</tool_result>
```

---

## 7. Skill 系统

### Skill 包格式（`.skillpkg`）

```
{name}-{version}.skillpkg  （ZIP 格式）
├── manifest.yaml       ← 元数据、权限、激活条件（必须，schema 严格验证）
├── skill.md            ← 注入系统提示的 Markdown 内容（必须）
├── tools/
│   └── {tool_name}.wasm ← WASM 扩展工具（可选）
├── assets/              ← 静态资源（可选，合计 ≤10MB）
└── manifest.sig         ← Ed25519 签名（签名覆盖 manifest.yaml + skill.md + tools/**）
```

### manifest.yaml Schema

```yaml
# 必填字段
name: weather-assistant          # 字母数字-下划线，≤64字符
version: 1.2.0                   # semver
author: "Skill Store Official"
description: "实时天气查询与预报"  # ≤200字符
min_claw_version: "0.1.0"

# 权限声明（必须明确，用户安装时展示确认弹窗）
permissions:
  network:                       # 允许访问的域名白名单
    - "wttr.in"
    - "api.openweathermap.org"
  memory: read                   # none | read | write
  files: none                    # none | read | write

# 激活条件
activation:
  keywords:                      # 精确子串匹配，≤20条
    - "天气"
    - "weather"
    - "气温"
  patterns:                      # 正则表达式，≤5条
    - "(?i)weather.*today"
  exclude_keywords:              # 任一匹配则不激活
    - "历史气象数据"
  max_context_tokens: 2000       # Skill 提示最大 token 预算

# WASM 工具声明（每个工具对应 tools/ 目录下的 .wasm 文件）
tools:
  - name: get_weather            # 注册时命名为 skill.weather-assistant.get_weather（三段式）
    wasm: tools/get_weather.wasm
    description: "查询指定城市天气"
    timeout_ms: 8000             # 单次执行超时
    memory_limit_mb: 8           # WASM 实例内存上限
    network:                     # 工具级网络权限（必须是 manifest.permissions.network 的子集）
      - "wttr.in"
```

### 信任等级（四级）

| 等级 | 来源 | 工具权限 | 安装要求 |
|------|------|---------|---------|
| `Bundled` | App 内置 | 完整 System 工具集 | 无需签名 |
| `Verified` | Skill Store 官方签名 | 完整 Skill 工具集 | Ed25519 官方签名 |
| `Community` | Skill Store 社区 | 只读工具集 + 限制网络 | Ed25519 + 自动安全扫描 |
| `Local` | 用户本地导入 | 只读工具集 + 最严限制 | 用户二次确认 |

**信任衰减规则**：若当前会话激活了 `Community` 或 `Local` 级 Skill，整个会话自动降级为只读模式（禁止 `file_write`、`memory_write`、`db_execute`）。

### Skill 安装流程

```
用户选择从 Skill Store 安装
    ↓
1. 下载 .skillpkg 到 temp/{uuid}.skillpkg
    ↓
2. 验证 Ed25519 签名（公钥硬编码于 App）
   失败 → 删除文件 + 写审计日志 + 报错返回
    ↓
3. 解压到 temp/{uuid}/，验证 manifest.yaml JSON Schema
   失败 → 清理 temp + 报错返回
    ↓
4. 向用户展示权限确认弹窗：
   "此 Skill 需要以下权限：
    · 网络访问：wttr.in
    · Memory：只读"
   用户拒绝 → 清理 temp + 返回
    ↓
5. WASM 静态分析（禁止调用白名单外的 import）
   失败 → 清理 temp + 报错返回
    ↓
6. SHA-256 校验和写入 installed_skills 表
    ↓
7. 复制到 skills/{name}/ 目录
    ↓
8. 注册到 SkillRegistry，立即生效
```

### Skill 激活评分算法

```rust
fn activation_score(skill: &LoadedSkill, message: &str) -> f32 {
    let text = message.to_lowercase();
    let mut score = 0.0f32;

    // 关键字匹配（每个 +0.3，上限 +0.9）
    let kw_matches = skill.manifest.activation.keywords.iter()
        .filter(|kw| text.contains(kw.as_str()))
        .count();
    score += (kw_matches as f32 * 0.3).min(0.9);

    // 否决关键字（任一匹配 → 立即返回 0.0）
    if skill.manifest.activation.exclude_keywords.iter()
        .any(|kw| text.contains(kw.as_str())) {
        return 0.0;
    }

    // 正则匹配（每个 +0.5，上限 +1.0）
    let re_matches = skill.compiled_patterns.iter()
        .filter(|re| re.is_match(&text))
        .count();
    score += (re_matches as f32 * 0.5).min(1.0);

    score.clamp(0.0, 2.0)
}

/// 激活阈值：score >= ACTIVATION_THRESHOLD 时 Skill 被包含进系统提示
/// 默认 0.3（可通过 FfiConfig.activation_threshold 配置）
pub const DEFAULT_ACTIVATION_THRESHOLD: f32 = 0.3;
```

---

## 8. Agent 循环

### 状态机

```
Idle
  ↓ 用户发送消息 / 定时任务触发
Preprocessing
  ↓ Memory 加载 + Skill 激活 + 媒体处理完成
Thinking（LLM 调用）
  ↓ 收到工具调用
ToolExecution（并行/顺序）
  ↓ 所有工具完成
Thinking（LLM 继续）
  ↓ 收到纯文字响应
Postprocessing（写会话 + 触发 Memory 压缩）
  ↓
Idle
```

### Agent Loop 实现

```rust
// claw_core/src/agent_loop.rs

pub struct AgentLoop {
    config: LoopConfig,
    llm: Arc<dyn LlmProvider>,
    tools: Arc<ToolRegistry>,
    memory: Arc<MemoryManager>,
    event_tx: UnboundedSender<ClawEvent>,
}

pub struct LoopConfig {
    pub max_iterations: usize,      // 默认 20
    pub max_tokens: u64,            // 默认 200_000（累计）
    pub max_duration: Duration,     // 默认 5 分钟
    pub max_consecutive_failures: u8, // 默认 3（安全熔断）
    pub parallel_tools: bool,       // 默认 true
}

impl AgentLoop {
    pub async fn run(
        &self,
        request: AgentRequest,
        abort: AbortSignal,
    ) -> Result<AgentResponse, AgentError> {
        let mut history = self.build_history(&request).await?;
        let mut breaker = CircuitBreaker::new(&self.config);
        let mut consecutive_failures: u8 = 0;

        for iteration in 0..self.config.max_iterations {
            breaker.check()?;  // 检查熔断条件

            // LLM 调用（流式）
            let response = self.llm.chat_stream(
                ChatRequest::from_history(&history, &self.tools.specs()),
                abort.clone(),
            ).await?;

            // 解析响应
            let parsed = self.parse_response(response).await?;

            match parsed {
                ParsedResponse::Text(text) => {
                    self.event_tx.send(ClawEvent::TurnComplete { ... })?;
                    return Ok(AgentResponse::text(text));
                }
                ParsedResponse::ToolCalls(calls) => {
                    // 并行执行独立工具
                    let results = if self.config.parallel_tools {
                        self.execute_parallel(calls, &abort).await
                    } else {
                        self.execute_sequential(calls, &abort).await
                    };

                    // 统计失败，检查熔断
                    let failures = results.iter().filter(|r| r.is_err()).count();
                    consecutive_failures = if failures > 0 {
                        consecutive_failures + failures as u8
                    } else {
                        0
                    };
                    if consecutive_failures >= self.config.max_consecutive_failures {
                        return Err(AgentError::CircuitBreaker);
                    }

                    history.push_tool_results(results);
                }
            }
        }

        Err(AgentError::MaxIterations)
    }
}
```

### Plan Mode（规划模式）

```
触发：
  · 用户输入 /plan 前缀
  · LLM 判断任务复杂度 > 阈值（tool_count 估计 > 5）

阶段 1：探索
  · 限制为只读工具（memory_read, file_read, http_request, db_query）
  · LLM 收集信息，不执行写操作
  · 最多 10 轮

阶段 2：生成计划
  · 写入 plans/active_plan.md：
    # 计划：{目标}
    ## 步骤
    1. [工具] 描述
    2. [工具] 描述
    ...
  · 推送 PlanReady 事件给 Flutter

阶段 3：用户确认
  · Flutter 展示计划，等待用户 approve/reject
  · Reject → 询问修改意见，重新生成
  · Approve → 进入执行阶段

阶段 4：执行
  · 按计划逐步执行，每步推送 TaskProgress 事件
  · 执行完成 → 删除 plans/active_plan.md
  · 写入 Memory（project 类型）
```

### 安全熔断器

```rust
pub struct CircuitBreaker {
    start_time: Instant,
    total_tokens: u64,
    config: LoopConfig,
}

impl CircuitBreaker {
    pub fn check(&self) -> Result<(), AgentError> {
        if self.start_time.elapsed() > self.config.max_duration {
            return Err(AgentError::Timeout);
        }
        if self.total_tokens > self.config.max_tokens {
            return Err(AgentError::TokenBudgetExceeded);
        }
        Ok(())
    }
}
```

---

## 9. LLM 提供商抽象

### Provider Trait

```rust
// claw_llm/src/lib.rs

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_model(&self) -> &str;
    fn supports_vision(&self) -> bool { false }
    fn supports_streaming(&self) -> bool { true }

    async fn chat_stream(
        &self,
        request: ChatRequest,
        abort: AbortSignal,
    ) -> Result<BoxStream<'static, Result<ChatChunk, LlmError>>, LlmError>;

    async fn embed(&self, text: &str) -> Result<Vec<f32>, LlmError>;

    fn validate_config(&self) -> Result<(), LlmError>;

    /// 重试策略（默认：指数退避，3次，初始1s，最大30s）
    /// ClaudeProvider 实现遵守 `Retry-After` 响应头
    fn retry_policy(&self) -> RetryPolicy {
        RetryPolicy::exponential(3, Duration::from_secs(1), Duration::from_secs(30))
    }
}

pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    // Vec<u16> 而非 &'static [u16]，允许运行时配置（如 enterprise 用户自定义可重试状态码）
    pub retryable_status: Vec<u16>,  // 默认 vec![429, 500, 502, 503, 504]
}

pub struct ChatRequest {
    pub system:     String,
    pub messages:   Vec<ChatMessage>,
    pub tools:      Vec<ToolSpec>,
    pub model:      String,
    pub max_tokens: u32,
    pub temperature: f32,
}

pub enum ChatChunk {
    TextDelta(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args_fragment: String },
    ToolCallEnd   { id: String },
    Usage { input_tokens: u32, output_tokens: u32 },
}
```

### Claude 实现

```rust
pub struct ClaudeProvider {
    client: reqwest::Client,      // 带证书固定的 HTTP 客户端
    api_key: SecretString,        // 运行时从 Keystore 读取
    base_url: &'static str,
}

// SPKI Hash 固定实现（rustls 自定义 ServerCertVerifier，不替换根证书）
// 使用 add_root_certificate() 属于根证书固定，当 Anthropic 证书轮转时会断联。
// SPKI Hash 固定仅验证公钥指纹，与证书有效期无关，支持平滑轮转。
fn build_spki_pinned_client() -> reqwest::Client {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::DigitallySignedStruct;

    #[derive(Debug)]
    struct SpkiPinVerifier {
        inner: Arc<rustls::client::WebPkiServerVerifier>,
        pins: &'static [&'static str],  // "sha256/BASE64==" 格式
    }

    impl ServerCertVerifier for SpkiPinVerifier {
        fn verify_server_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            intermediates: &[CertificateDer<'_>],
            server_name: &ServerName<'_>,
            ocsp: &[u8],
            now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            // 第一步：正常 WebPKI 验证（CA 信任链 + 域名 + 有效期）
            self.inner.verify_server_cert(end_entity, intermediates, server_name, ocsp, now)?;
            // 第二步：提取叶证书 SPKI，计算 SHA-256，与 pins 比对
            let spki_hash = extract_spki_sha256(end_entity);
            let fingerprint = format!("sha256/{}", BASE64.encode(spki_hash));
            if self.pins.iter().any(|p| *p == fingerprint) {
                Ok(ServerCertVerified::assertion())
            } else {
                Err(rustls::Error::General("SPKI pin mismatch".into()))
            }
        }
        // verify_tls12_signature / verify_tls13_signature 委托给 inner
        fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct)
            -> Result<HandshakeSignatureValid, rustls::Error> { self.inner.verify_tls12_signature(m, c, d) }
        fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct)
            -> Result<HandshakeSignatureValid, rustls::Error> { self.inner.verify_tls13_signature(m, c, d) }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.inner.supported_verify_schemes()
        }
    }

    let tls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SpkiPinVerifier {
            inner: rustls::client::WebPkiServerVerifier::builder(
                Arc::new(rustls_native_certs::load_native_certs().unwrap())
            ).build().unwrap(),
            pins: ANTHROPIC_SPKI_PINS,
        }))
        .with_no_client_auth();

    reqwest::ClientBuilder::new()
        .use_preconfigured_tls(tls_config)
        .build()
        .unwrap()
}
```

**API Key 存储**：
- iOS：`SecItemAdd` 写入 Keychain，`kSecAttrAccessible = kSecAttrAccessibleAfterFirstUnlock`
- Android：`KeyStore.getInstance("AndroidKeyStore")`，AES-256-GCM 加密后写文件
- Linux（测试）：存入 `~/.config/mobileclaw/secrets`（文件权限 0600）
- **绝不写入 SQLite 或任何应用数据文件**

---

## 10. 媒体处理管道

### 图片处理

```rust
// claw_media/src/image.rs

pub struct ImageProcessor;

impl ImageProcessor {
    /// 压缩并转换为 Claude Vision 可用格式
    pub async fn process(
        data: Bytes,
        mime: &str,
    ) -> Result<ProcessedImage, MediaError> {
        tokio::task::spawn_blocking(move || {
            let img = image::load_from_memory(&data)?;
            // 等比缩放，长边 ≤ 1920
            let img = resize_if_needed(img, 1920);
            // 编码为 JPEG，quality=85
            let mut buf = Vec::new();
            img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)?;
            Ok(ProcessedImage {
                data: Bytes::from(buf),
                mime: "image/jpeg",
                width: img.width(),
                height: img.height(),
            })
        }).await?
    }
}
```

**支持格式**：JPEG / PNG / WebP（通过 `image-rs` 解码）；HEIC 通过平台原生解码：
- iOS：`CGImageSource` (via Swift platform channel → 解码为 PNG bytes → 传回 Rust)
- Android：`BitmapFactory.decodeFile()` (via JNI → 解码为 JPEG bytes → 传回 Rust)
- Linux：`libheif-rs`（仅测试，`#[cfg(target_os="linux")]`）

**大小限制**：输入 ≤ 20MB

### 视频处理（双轨并行）

```rust
pub struct VideoProcessor;

impl VideoProcessor {
    pub async fn process(
        path: &Path,
        temp_dir: &Path,
    ) -> Result<ProcessedVideo, MediaError> {
        // 双轨并行处理
        let (frames_result, transcript_result) = tokio::join!(
            Self::extract_frames(path, temp_dir),
            Self::transcribe_audio(path, temp_dir),
        );

        Ok(ProcessedVideo {
            frames: frames_result?,
            transcript: transcript_result.ok(),  // ASR 失败不影响视觉
        })
    }

    /// 视觉轨：提取关键帧（每秒1帧，≤20帧）
    /// 实现策略：
    ///   iOS:     AVAssetImageGenerator (via flutter platform channel → Swift → Rust 回调)
    ///   Android: MediaMetadataRetriever (via JNI)
    ///   Linux:   ffmpeg-sys-next (仅 #[cfg(target_os="linux")] feature，测试用)
    async fn extract_frames(path: &Path, temp_dir: &Path) -> Result<Vec<ProcessedImage>>;

    /// 音频轨：提取音频 + Whisper ASR
    /// 音频提取同样使用平台原生 API（AVAudioFile / MediaExtractor）
    async fn transcribe_audio(path: &Path, temp_dir: &Path) -> Result<String>;
}
```

**注意**：禁止在移动端依赖 `ffmpeg-next` / `libffmpeg`（包体积 +10-20MB，LGPL/GPL 许可证风险，App Store 审核风险）。视频处理通过 Dart platform channel 调用系统 API，结果以临时文件路径传回 Rust。

**Whisper 模型管理**：
- 模型文件（`ggml-base.bin`，~150MB）按需下载
- 存储于 `{data_dir}/models/whisper/`
- 下载进度推送 `ModelDownloadProgress` 事件
- 不随 App 打包（控制包体积）

**视频限制**：
- 输入大小 ≤ 200MB
- 时长 ≤ 10 分钟（超出截取前 10 分钟）
- 帧数上限 20 帧（防 token 爆炸）

---

## 11. 存储层

### Storage Trait

```rust
// claw_storage/src/lib.rs

#[async_trait]
pub trait Storage: Send + Sync {
    // 文件操作（所有 path 经 jail() 验证）
    async fn read_file(&self, path: &Path) -> Result<Bytes, StorageError>;
    async fn write_file(&self, path: &Path, data: &[u8]) -> Result<(), StorageError>;
    async fn delete_file(&self, path: &Path) -> Result<bool, StorageError>;
    async fn list_dir(&self, dir: &Path, max_depth: u8) -> Result<Vec<DirEntry>, StorageError>;
    async fn exists(&self, path: &Path) -> Result<bool, StorageError>;

    // 数据库操作
    async fn db_query(&self, sql: &str, params: &[SqlValue]) -> Result<Vec<SqlRow>, StorageError>;
    async fn db_execute(&self, sql: &str, params: &[SqlValue]) -> Result<u64, StorageError>;
    async fn db_transaction<F, R>(&self, f: F) -> Result<R, StorageError>
    where F: FnOnce(&Transaction) -> Result<R, rusqlite::Error> + Send;

    // WebDAV
    async fn webdav_sync(&self, config: &WebDavConfig, direction: SyncDirection)
        -> Result<SyncStats, StorageError>;
}

pub struct WebDavConfig {
    pub url: String,         // WebDAV 根 URL（必须 https://）
    pub username: String,
    pub password: SecretString,  // 运行时从 Keystore 读取
    pub remote_path: String, // 远端同步目录，如 "/claw/"
}

pub enum SyncDirection {
    Push,
    Pull,
    Bidirectional { conflict: ConflictStrategy },
}

pub enum ConflictStrategy {
    LocalWins,   // 本地覆盖远端（默认）
    RemoteWins,  // 远端覆盖本地
    KeepBoth,    // 保留冲突副本（`{name}.conflict.{timestamp}` 后缀）
                 // 冲突检测：ETag 不匹配 AND 内容 SHA-256 不同
                 // 推送 FfiEvent::SyncConflict 通知用户
}

/// KeepBoth 模式下，用户通过 resolve_conflict() 做最终选择
pub enum ConflictChoice {
    KeepLocal,    // 丢弃冲突副本，以本地版本为准并推送到远端
    KeepConflict, // 以冲突副本覆盖本地，删除冲突后缀文件
}

// FfiEvent 中新增（见 §13 事件流）：
// SyncConflict { local_path: String, conflict_path: String }
// 用户可通过 resolve_conflict() 解决冲突

// WebDAV 冲突解决 API（存储层，返回 StorageError；FFI 层包装后返回 FfiError）
pub async fn resolve_conflict(
    &self,
    local_path: String,
    keep: ConflictChoice,
) -> Result<(), StorageError>;
```

### Database 类型定义

```rust
// claw_storage/src/db.rs
// SQLite WAL 支持并发读取，使用读写分离连接

pub struct Database {
    /// 写连接（串行化，Mutex 保护）
    write: Mutex<Connection>,
    /// 读连接池（r2d2-sqlite，允许并发只读查询）
    read_pool: Pool<SqliteConnectionManager>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let mut write_conn = Connection::open(path)?;
        apply_pragmas(&write_conn)?;
        migrate(&mut write_conn)?;  // 运行 schema 迁移（要求 &mut，因为每步使用 conn.transaction()）

        let manager = SqliteConnectionManager::file(path)
            .with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI);
        let read_pool = Pool::builder().max_size(4).build(manager)?;

        Ok(Self { write: Mutex::new(write_conn), read_pool })
    }

    pub fn read<F, R>(&self, f: F) -> Result<R, StorageError>
    where F: FnOnce(&Connection) -> Result<R, rusqlite::Error> {
        let conn = self.read_pool.get()?;
        f(&conn).map_err(Into::into)
    }

    pub fn write<F, R>(&self, f: F) -> Result<R, StorageError>
    where F: FnOnce(&mut Connection) -> Result<R, rusqlite::Error> {
        // MutexGuard 提供独占访问，通过 &mut *conn 将其转为 &mut Connection
        // rusqlite 的写操作（execute / transaction 等）均需要 &mut self
        let mut conn = self.write.lock().unwrap();
        f(&mut *conn).map_err(Into::into)
    }
}
```

`Arc<Database>` 在 `MemoryManager`、`ToolContext`、`Scheduler` 间共享，是 `claw_storage` 的核心类型。

### 本地存储实现

```rust
pub struct LocalStorage {
    root: cap_std::fs::Dir,  // cap-std Dir（已 jail 的目录句柄）
    db: Arc<Database>,
}
```

---

## 12. 后台任务调度

### 任务类型

```rust
pub enum Schedule {
    Cron(CronSchedule),        // "0 8 * * *"（使用 cron crate 解析）
    Interval(Duration),        // 每隔固定时间
    Once(DateTime<Utc>),       // 一次性执行
}

pub enum ContextMode {
    Group,      // 沿用上一次会话 session_id（有记忆连续性）
    Isolated,   // 新建独立会话（干净执行）
}
```

### 调度器实现

```rust
// claw_core/src/agent_factory.rs（定义在 claw_core，scheduler 依赖 claw_core）

pub trait AgentFactory: Send + Sync {
    /// 为后台任务创建一个 Agent Loop 实例
    fn create_agent(
        &self,
        session_id: Option<&str>,    // Some = Group 模式复用，None = Isolated 新建
        context_mode: ContextMode,
        event_tx: UnboundedSender<ClawEvent>,
    ) -> Arc<AgentLoop>;
}
```

```rust
// claw_scheduler/src/lib.rs

pub struct Scheduler {
    db: Arc<Database>,
    event_tx: UnboundedSender<ClawEvent>,
    agent_factory: Arc<dyn AgentFactory>,  // 由 claw_core 提供实现
}

impl Scheduler {
    pub async fn run(self: Arc<Self>, mut shutdown: broadcast::Receiver<()>) {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.tick().await;
                }
                _ = shutdown.recv() => break,
            }
        }
    }

    async fn tick(&self) {
        let now = Utc::now().timestamp();
        let due_tasks = self.db.get_due_tasks(now).await.unwrap_or_default();

        for task in due_tasks {
            let scheduler = self.clone();
            let task_id = task.id.clone();

            // 推送 TaskStarted 事件（UI 透明化）
            self.event_tx.send(ClawEvent::TaskStarted {
                task_id: task_id.clone(),
                name: task.name.clone(),
            }).ok();

            tokio::spawn(async move {
                let result = scheduler.run_task(&task).await;
                match result {
                    Ok(summary) => scheduler.event_tx.send(
                        ClawEvent::TaskCompleted { task_id, summary }
                    ).ok(),
                    Err(e) => scheduler.event_tx.send(
                        ClawEvent::TaskFailed { task_id, error: e.to_string() }
                    ).ok(),
                };
            });

            // 更新 next_run_at
            self.db.update_task_next_run(&task).await.ok();
        }
    }
}
```

### 任务透明化事件序列

```
TaskStarted   { task_id, name }           → UI 显示任务卡片，状态：运行中
TaskProgress  { task_id, step, detail }   → UI 更新步骤描述（LLM 工具调用时推送）
TaskCompleted { task_id, summary }        → UI 显示完成，绿色 ✓
TaskFailed    { task_id, error }          → UI 显示失败，红色 ✗，可查看详情
```

---

## 13. Flutter 集成与 UI 架构

### flutter_rust_bridge 边界

```rust
// claw_ffi/src/lib.rs（frb 自动生成 Dart 绑定）

pub struct ClawEngine(Arc<InnerEngine>);

impl ClawEngine {
    /// 初始化引擎（异步，frb 2.x 映射为 Dart Future）
    /// 内部创建 Tokio Runtime 单例，打开 SQLite，运行 schema 迁移
    pub async fn init(config: FfiConfig) -> Result<ClawEngine, FfiError>;

    /// 发送消息（返回事件流）
    pub fn send_message(
        &self,
        message: String,
        media: Vec<FfiMedia>,
    ) -> Result<(), FfiError>;

    /// 中断当前 Agent Loop
    pub fn abort(&self) -> Result<(), FfiError>;

    /// 订阅事件流
    pub fn event_stream(&self) -> StreamSink<FfiEvent>;

    /// 查询后台任务列表
    pub fn list_tasks(&self) -> Result<Vec<FfiTask>, FfiError>;

    /// 创建后台任务
    pub fn create_task(&self, req: FfiTaskRequest) -> Result<String, FfiError>;

    /// 取消后台任务
    pub fn cancel_task(&self, task_id: String) -> Result<(), FfiError>;

    /// 查询 Memory 列表
    pub fn list_memories(&self) -> Result<Vec<FfiMemory>, FfiError>;

    /// 搜索 Memory
    pub fn search_memory(&self, query: String) -> Result<Vec<FfiMemory>, FfiError>;

    /// 已安装 Skill 列表
    pub fn list_skills(&self) -> Result<Vec<FfiSkill>, FfiError>;

    /// 安装 Skill（返回进度事件流）
    pub fn install_skill(&self, pkg_path: String) -> Result<(), FfiError>;

    /// 卸载已安装的 Skill（从 skills/ 目录删除 + 数据库记录删除）
    pub fn uninstall_skill(&self, skill_name: String) -> Result<(), FfiError>;

    /// 启用/禁用 Skill（不卸载，只停止激活）
    pub fn set_skill_enabled(&self, skill_name: String, enabled: bool) -> Result<(), FfiError>;

    /// 配置 WebDAV
    pub fn configure_webdav(&self, config: FfiWebDavConfig) -> Result<(), FfiError>;

    /// 手动触发 WebDAV 同步
    pub fn sync_webdav(&self) -> Result<(), FfiError>;

    /// 解决 WebDAV 同步冲突（收到 FfiEvent::SyncConflict 后调用）
    /// keep=KeepLocal  → 删除冲突副本，保留本地版本（推送到远端）
    /// keep=KeepConflict → 将冲突副本重命名为正式文件，覆盖本地版本
    pub async fn resolve_conflict(
        &self,
        local_path: String,
        keep: ConflictChoice,
    ) -> Result<(), FfiError>;
}

// 跨 FFI 边界的数据类型（只用基础类型）
pub struct FfiConfig {
    pub data_dir: String,          // App 沙箱目录
    /// Keystore 中存储 API Key 的 alias（不传明文 Key）。
    /// claw_ffi 在 Rust 层通过 JNI(Android) / Security.framework(iOS) 直接读取，
    /// 明文 Key 仅存在于 Rust SecretString，不进入 Dart heap 或 frb 序列化缓冲区。
    pub keystore_alias: String,
    pub model: String,
    pub http_allowlist: Vec<String>,
    pub max_loop_iterations: u32,
    pub activation_threshold: f32, // Skill 激活分数阈值，默认 0.3
    pub disable_cert_pinning: bool, // 仅 enterprise MDM 场景使用，默认 false
}

// Keystore 读取接口（Rust 内部，不暴露给 Dart）
// claw_ffi/src/keystore.rs
// iOS:   SecItemCopyMatching via security-framework crate
// Android: KeyStore.getInstance("AndroidKeyStore") via jni crate + AES-GCM 解密
// Linux:  ~/.config/mobileclaw/secrets (权限 0600)

/// 图片（≤20MB）用 InlineData，视频用 FilePath（避免跨 FFI 200MB 复制）
pub enum FfiMediaSource {
    InlineData(Vec<u8>),   // 图片等小文件，直接传字节
    FilePath(String),      // 视频等大文件，传沙箱内临时文件路径
                           // Rust 侧用 cap-std Dir 打开，jail 验证后流式读取
}

pub struct FfiMedia {
    pub mime: String,            // "image/jpeg", "video/mp4" 等
    pub source: FfiMediaSource,
}

// FfiEvent 对应 ClawEvent（扁平化枚举）
pub enum FfiEvent {
    TextDelta       { text: String },
    ToolCallStart   { name: String, id: String },
    ToolCallEnd     { id: String, result: String, duration_ms: u64, success: bool },
    TurnComplete    { session_id: String },
    Error           { code: String, message: String },
    TaskStarted     { task_id: String, name: String },
    TaskProgress    { task_id: String, step: String },
    TaskCompleted   { task_id: String, summary: String },
    TaskFailed      { task_id: String, error: String },
    MemoryUpdated   { added: u32, updated: u32 },
    SyncStarted,
    SyncCompleted   { pushed: u32, pulled: u32 },
    SyncFailed      { error: String },
    SyncConflict    { local_path: String, conflict_path: String },
    PlanReady       { plan_content: String },
    /// 证书 pin 更新通知（证书轮转前由 Anthropic 服务端推送）
    /// 认证机制：
    ///   · new_pins 字段附带服务端 Ed25519 签名（由内置的 Anthropic 公钥验证）
    ///   · Rust 层验签通过后，更新内存中的 pin 列表（不修改 App 内置常量）
    ///   · 写入加密存储，下次启动时优先使用已更新的 pin（降级保护：内置 pins 作兜底）
    ///   · 签名验证失败 → 忽略更新，写审计日志 B2_PIN_UPDATE_SIG_FAIL，不通知 Dart
    CertPinUpdate   { new_pins: Vec<String>, signature: String },
}
```

### Flutter 应用架构

```
mobileclaw_app/lib/
├── main.dart                    ← App 入口，初始化 ClawEngine
├── core/
│   ├── engine_provider.dart     ← Riverpod Provider（ClawEngine 单例）
│   └── event_bus.dart           ← FfiEvent → 各 Provider 分发
├── features/
│   ├── chat/
│   │   ├── chat_provider.dart   ← 消息状态管理
│   │   ├── chat_page.dart       ← 聊天主界面
│   │   ├── message_bubble.dart  ← 消息气泡（支持 Markdown 渲染）
│   │   ├── tool_call_card.dart  ← 工具调用可视化卡片
│   │   └── media_input_bar.dart ← 输入栏（文字/图片/视频）
│   ├── tasks/
│   │   ├── task_provider.dart
│   │   ├── task_panel.dart      ← 后台任务透明面板（可折叠）
│   │   └── task_create_sheet.dart
│   ├── memory/
│   │   ├── memory_provider.dart
│   │   └── memory_browser.dart
│   ├── skills/
│   │   ├── skill_provider.dart
│   │   ├── skill_manager.dart
│   │   └── skill_store.dart     ← 未来 Skill Store 入口
│   └── settings/
│       ├── settings_page.dart
│       ├── api_key_page.dart    ← 安全存储 API Key
│       ├── webdav_page.dart
│       └── http_allowlist_page.dart
└── shared/
    ├── theme.dart
    └── widgets/
        ├── loading_indicator.dart
        └── error_banner.dart
```

### 后台任务透明化 UI 设计

```
聊天界面底部固定区域：

┌──────────────────────────────────────┐
│ [后台任务]  ① 运行中  ✓ 3 已完成   ▼ │ ← 可折叠栏
├──────────────────────────────────────┤
│ ⏰ 每日早报         ··· 运行中       │
│    步骤 2/4：正在获取热点新闻        │
│    已用时 12s                [取消]  │
├──────────────────────────────────────┤
│ 🔄 Memory 压缩      ✓ 刚刚完成       │
│    新增 3 条，更新 1 条              │
├──────────────────────────────────────┤
│ ☁️  WebDAV 同步      ✓ 刚刚完成       │
│    ↑ 5 个文件  ↓ 2 个文件           │
└──────────────────────────────────────┘

规则：
· 有活跃任务 → 自动展开 + App 图标角标数字
· 全部完成 → 5秒后自动折叠
· 点击任务卡片 → 展开完整日志
· [取消] 按钮立即终止任务
```

---

## 14. SDK 化架构

### Flutter Plugin Package 结构

```
mobileclaw_sdk/
├── lib/
│   ├── mobileclaw_sdk.dart     ← 统一导出入口
│   └── src/
│       ├── engine.dart         ← ClawEngine 包装
│       ├── events.dart         ← ClawEvent、FfiEvent 转换
│       ├── models.dart         ← Message, Task, Memory, Skill 等模型
│       ├── config.dart         ← ClawConfig
│       └── bridge/             ← frb 自动生成代码（不手写）
├── android/
│   ├── src/main/
│   │   └── jniLibs/            ← libmobileclaw.so（各 ABI）
│   └── build.gradle
├── ios/
│   ├── MobileClawSDK.xcframework  ← 预编译 XCFramework
│   └── mobileclaw_sdk.podspec
├── linux/
│   └── libmobileclaw.so        ← Linux 桌面（测试用）
├── example/                    ← 最小集成示例
└── pubspec.yaml
```

### SDK 公开 API（Dart）

```dart
// 最简集成
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';

// ClawConfig 使用 keystoreAlias，不接受明文 API Key。
// 使用前须先用平台 API 将 Key 写入 Keychain/Keystore：
//   iOS:     await SecureStorage.write(alias: 'anthropic_key', value: rawKey);
//   Android: await SecureStorage.write(alias: 'anthropic_key', value: rawKey);
// Rust 层在初始化时通过 JNI / Security.framework 直接读取，Key 不经过 Dart heap。
final engine = await ClawEngine.init(ClawConfig(
  keystoreAlias: 'anthropic_key',   // Keystore/Keychain alias，绝不传明文 Key
  dataDir: (await getApplicationSupportDirectory()).path,
  httpAllowlist: ['wttr.in', 'api.github.com'],
));

// 方式 A：使用内置 Widget
MobileClawChatView(engine: engine)

// 方式 B：只用逻辑层，自定义 UI
engine.events.listen((event) {
  switch (event) {
    case TextDeltaEvent(:final text): _appendText(text);
    case ToolCallStartEvent(:final name): _showToolCard(name);
    case TaskStartedEvent(:final name): _showTaskNotification(name);
  }
});
await engine.sendMessage('帮我查一下今天北京的天气');
```

---

## 15. 项目目录结构

```
~/rust/mobileclaw/                     ← Rust Workspace
├── Cargo.toml
├── crates/
│   ├── claw_core/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── agent_loop.rs
│   │   │   ├── plan_mode.rs
│   │   │   └── circuit_breaker.rs
│   │   └── Cargo.toml
│   ├── claw_memory/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── manager.rs
│   │       ├── index.rs           ← MEMORY.md 索引管理
│   │       ├── compressor.rs      ← 后台压缩 Agent
│   │       └── types.rs
│   ├── claw_tools/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── trait_.rs
│   │       ├── registry.rs
│   │       ├── http.rs
│   │       ├── file.rs
│   │       ├── db.rs
│   │       └── utils.rs
│   ├── claw_skills/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── loader.rs
│   │       ├── manifest.rs
│   │       ├── selector.rs
│   │       ├── trust.rs
│   │       └── installer.rs
│   ├── claw_wasm_runtime/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── runtime.rs         ← wamr/wasm3 抽象
│   │       ├── host_funcs.rs      ← 宿主白名单函数
│   │       ├── leak_detector.rs
│   │       └── sandbox.rs
│   ├── claw_llm/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── provider.rs        ← Provider Trait
│   │       ├── claude.rs          ← Claude 实现
│   │       └── message.rs
│   ├── claw_media/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── image.rs
│   │       ├── video.rs
│   │       └── asr.rs             ← Whisper 集成
│   ├── claw_storage/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── local.rs           ← 本地 SQLite + 文件
│   │       ├── webdav.rs          ← WebDAV 同步
│   │       ├── schema.rs          ← 建表 SQL
│   │       └── jail.rs            ← 路径安全验证
│   ├── claw_scheduler/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── scheduler.rs
│   │       └── task.rs
│   └── claw_ffi/
│       └── src/
│           ├── lib.rs
│           ├── types.rs           ← FfiConfig, FfiEvent 等
│           └── validation.rs      ← B1 边界验证
│
~/agent_eyes/bot/mobileclaw/          ← Flutter 工程根
├── packages/
│   └── mobileclaw_sdk/
│       ├── lib/
│       │   ├── mobileclaw_sdk.dart
│       │   └── src/
│       ├── android/
│       ├── ios/
│       ├── linux/
│       └── pubspec.yaml
├── apps/
│   └── mobileclaw_app/
│       ├── lib/
│       │   ├── main.dart
│       │   └── features/
│       └── pubspec.yaml
├── docs/
│   ├── superpowers/specs/
│   │   └── 2026-04-01-mobileclaw-design.md   ← 本文件
│   ├── requirements.md
│   ├── architecture.md
│   ├── detailed_design.md
│   ├── test_cases.md
│   └── test_conclusions.md
├── tests/
│   ├── rust_integration/          ← Rust 集成测试
│   └── flutter_integration/       ← Flutter 集成测试
└── venv/                          ← Python 工具链
```

---

## 16. Rust Crate 依赖图

```
claw_ffi
  └─▶ claw_core           ← 共享类型定义：ChatMessage/ChatContent/MediaRef 等
         ├─▶ claw_memory
         │       └─▶ claw_storage
         ├─▶ claw_tools
         │       ├─▶ claw_storage
         │       └─▶ claw_wasm_runtime
         ├─▶ claw_skills
         │       ├─▶ claw_wasm_runtime
         │       └─▶ claw_storage
         ├─▶ claw_llm      ← 仅依赖 claw_core（使用其中的 ChatMessage/MediaRef 类型）
         ├─▶ claw_media    ← 仅依赖 claw_core（将处理结果填充到 MediaRef 中）
         └─▶ claw_scheduler
                 ├─▶ claw_storage
                 └─▶ claw_core   (AgentFactory trait)

规则：
· 单向依赖（无循环）
· **媒体相关的共享数据类型**（ChatContent::Image/Video、MediaRef）定义在 claw_core，
  claw_llm 和 claw_media 均依赖 claw_core，两者之间无直接依赖
· claw_storage 是最底层，无外部 crate 依赖（除 rusqlite）
· claw_ffi 是唯一对外暴露的 crate
· 每个 crate 有独立的 #[cfg(test)] 单元测试
```

---

## 17. 编译与打包

### Android 编译

```bash
# 安装 Android 目标
rustup target add aarch64-linux-android armv7-linux-androideabi

# 使用 cargo-ndk
cargo install cargo-ndk
cargo ndk -t aarch64-linux-android \
          -t armv7-linux-androideabi \
          -o ../mobileclaw_sdk/android/src/main/jniLibs \
          build --release -p claw_ffi
```

**NDK 版本**：28.2.13676358（已安装）

### iOS 编译

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo build --release --target aarch64-apple-ios -p claw_ffi
cargo build --release --target aarch64-apple-ios-sim -p claw_ffi
# 合并为 XCFramework
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libclaw_ffi.a \
  -library target/aarch64-apple-ios-sim/release/libclaw_ffi.a \
  -output MobileClawSDK.xcframework
```

### Linux 桌面（测试用）

```bash
cargo build --release -p claw_ffi
# 产物：target/release/libclaw_ffi.so
```

### Cargo.toml 优化配置

```toml
[profile.release]
lto = "thin"          # 链接时优化
codegen-units = 1     # 最大优化，牺牲编译速度
strip = true          # 去除符号表
panic = "abort"       # 减小二进制体积
opt-level = 3
```

**预期包体积**：
- Android `.so`：~8MB（含 WASM 运行时）
- iOS `.a`：~12MB
- Linux `.so`：~6MB

---

## 18. 测试策略

### 测试分层

| 层次 | 工具 | 覆盖目标 |
|------|------|---------|
| Rust 单元测试 | `cargo test` | 每个 crate 的核心逻辑 |
| Rust 集成测试 | `cargo test --test *` | crate 间交互，SQLite 实际读写 |
| Python 脚本测试 | pytest (venv) | API 边界、安全边界回归 |
| Flutter 单元测试 | `flutter test` | Dart 模型、事件解析 |
| Flutter 集成测试 | `flutter test integration_test` | 端到端对话流程 |

### 关键测试用例（详见 test_cases.md）

- **B1-B5 安全边界**：路径穿越、SSRF、WASM 越权、SQL 注入
- **Memory 管理**：MEMORY.md 截断、索引去重、FTS5 检索
- **Agent 熔断**：最大轮次、超时、连续失败
- **Skill 安装**：签名验证失败、权限拒绝、WASM 静态分析拦截
- **媒体处理**：超大文件拒绝、格式不支持、ASR 失败降级

---

## 19. 性能目标

| 指标 | 目标值 | 测量方式 |
|------|--------|---------|
| 首字节延迟 | < 500ms | 从 sendMessage 到第一个 TextDelta |
| MEMORY.md 加载 | < 5ms | 25KB 文件同步读取 |
| FTS5 检索 | < 20ms | 1000 条记录，10 个词 |
| 工具执行（http_request） | < 3s（P95） | 网络请求端到端 |
| WASM 工具启动 | < 50ms | 实例化到第一次执行 |
| SQLite 写入（WAL） | < 2ms | 单行 INSERT |
| 媒体处理（1张图片） | < 200ms | 压缩 + base64 |
| 视频帧提取（30s视频） | < 2s | 提取 20 帧 |
| 内存占用（空闲） | < 30MB | Rust heap |
| 内存占用（会话中） | < 80MB | 包含 Whisper 模型 |

---

## 20. 版本路线图

### v0.1.0 - MVP（当前目标）
- [x] 设计文档完成
- [ ] Rust workspace 骨架搭建
- [ ] claw_storage（SQLite + 文件 + 路径 jail）
- [ ] claw_memory（MEMORY.md 体系）
- [ ] claw_llm（Claude Provider）
- [ ] claw_tools（http/file/db 内置工具）
- [ ] claw_core（基础 Agent Loop，无规划）
- [ ] claw_ffi（基础 FFI）
- [ ] mobileclaw_sdk（Flutter Plugin 骨架）
- [ ] mobileclaw_app（最简聊天界面）
- [ ] 基础测试套件通过

### v0.2.0 - 完整功能
- [ ] claw_wasm_runtime（WASM 沙箱）
- [ ] claw_skills（Skill 加载 + WASM 工具）
- [ ] claw_scheduler（后台任务）
- [ ] claw_media（图片 + 视频处理）
- [ ] Plan Mode
- [ ] WebDAV 同步
- [ ] 后台 Memory 压缩
- [ ] 完整 UI（任务面板、Memory 浏览器、设置）

---

## 21. 错误代码目录（Error Code Catalog）

`FfiEvent::Error.code` 和 `audit_log.event` 使用以下前缀体系。

`audit_log.category` 字段区分**安全类**（实时告警、需人工审查）与**运行时类**（统计监控、可自动恢复）：

| 前缀 | category | 来源边界 | 示例 |
|------|---------|---------|------|
| `B1_` | `security` | Dart→Rust FFI 验证 | `B1_PATH_TOO_LONG`, `B1_NULL_BYTE` |
| `B2_` | `security` | Rust→LLM API | `B2_CERT_PIN_FAIL`, `B2_TLS_DOWNGRADE`, `B2_KEY_INVALID`, `B2_PIN_UPDATE_SIG_FAIL` |
| `B3_` | `security` | Rust→文件系统 | `B3_PATH_ESCAPE`, `B3_SYMLINK_ESCAPE`, `B3_FILE_TOO_LARGE` |
| `B4_` | `security` | Rust→WebDAV | `B4_HTTP_ONLY`, `B4_AUTH_FAIL`, `B4_CONFLICT` |
| `B5_` | `security` | Rust→WASM | `B5_IMPORT_DENIED`, `B5_FUEL_EXHAUSTED`, `B5_MEMORY_OOM`, `B5_LEAK_DETECTED` |
| `LLM_` | `operational` | LLM 调用 | `LLM_RATE_LIMIT`, `LLM_CONTEXT_TOO_LONG`, `LLM_TIMEOUT`, `LLM_AUTH_ERROR` |
| `TOOL_` | `operational` | 工具执行 | `TOOL_NOT_FOUND`, `TOOL_TIMEOUT`, `TOOL_INVALID_ARGS`, `TOOL_SSRF_BLOCKED` |
| `SKILL_` | `operational` | Skill 安装/执行 | `SKILL_SIG_INVALID`, `SKILL_SCHEMA_ERROR`, `SKILL_PERM_DENIED`, `SKILL_WASM_INVALID` |
| `MEDIA_` | `operational` | 媒体处理 | `MEDIA_TOO_LARGE`, `MEDIA_FORMAT_UNSUPPORTED`, `MEDIA_ASR_FAILED` |
| `AGENT_` | `operational` | Agent 循环 | `AGENT_MAX_ITER`, `AGENT_TIMEOUT`, `AGENT_CIRCUIT_BREAK`, `AGENT_ABORTED` |
| `STORAGE_` | `operational` | 存储层 | `STORAGE_DB_CORRUPT`, `STORAGE_DISK_FULL`, `STORAGE_MIGRATION_FAIL` |

查询示例：
```sql
-- 仅查安全告警
SELECT * FROM audit_log WHERE category = 'security' AND blocked = 1 ORDER BY ts DESC;
-- 查运行时错误统计
SELECT event, COUNT(*) FROM audit_log WHERE category = 'operational' GROUP BY event;
```

---

### v0.3.0 - Skill Store
- [ ] Skill Store API 对接
- [ ] Ed25519 签名验证
- [ ] 信任模型完整实现
- [ ] 更多 LLM 提供商（OpenAI、Gemini）
- [ ] Whisper ASR 集成
