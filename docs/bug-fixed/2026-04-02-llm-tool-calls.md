# Bug Report: LLM 不调用工具 及关联问题

**日期:** 2026-04-02  
**影响版本:** mobileclaw-core (Phase 1 all commits up to this date)  
**状态:** 已修复

---

## Bug 1: LLM 从不调用工具（主 bug）

### 现象

用户通过 `mclaw chat` 输入自然语言请求，例如：

```
you> 帮我获取 work 账号最近 5 封邮件
```

LLM 直接返回纯文本回答，从未产生任何 `<tool_call>` XML，工具完全没有被调用。查看 `mclaw.log`（添加日志后）可以看到 `tool_calls_found=0`，即 parser 在 LLM 响应里一个工具调用也没有找到。

### 根本原因

`AgentLoop::chat()`（`src/agent/loop_impl.rs`）在构建每轮 system prompt 时，只拼接了 base_system 和 skill_prompt，从未向 LLM 描述过任何工具：

```rust
// 修复前（伪代码）
let system = format!("{}{}", base_system, skill_prompt);
// → LLM 不知道有工具，不知道 <tool_call> 格式
```

LLM 遵循的是 system prompt 里的指令。没有工具描述段，LLM 既不知道 `<tool_call>` XML 格式存在，也不知道注册了哪些工具，自然永远不会产生工具调用。

### 修复

新增纯函数 `build_tools_section(registry, matched_skills)` 生成 `## Available Tools` 段落，内容包括：

- 完整的 `<tool_call>` XML 调用格式说明
- 每个工具的名称、描述、参数列表（类型、是否必填）

在 `chat()` 中将该段落拼接到每轮 system prompt 末尾：

```rust
// 修复后
let matched = self.skill_mgr.match_skills(user_input);
let skill_prompt = self.skill_mgr.build_system_prompt(base_system, &matched);
let tools_section = build_tools_section(&self.registry, &matched);
let system = format!("{}{}", skill_prompt, tools_section);
```

修复后 `mclaw.log` 可以观察到 `tool_calls_found=1`，以及 `executing tool tool=email_fetch args={...}`。

### 影响范围

- CLI（`mclaw chat`）和 Flutter 均受影响——两者底层都走同一个 `AgentLoop`
- 所有注册工具（文件读写、HTTP、邮件、内存、时间等）在修复前均无法被调用
- Skill 机制不受影响（skill 匹配和 prompt 注入是独立路径，已正常工作）

---

## Bug 2: `allowed_tools=None` 的 skill 无法解除工具过滤

### 现象

`build_tools_section` 中，当同时匹配到：
- Skill A：`allowed_tools = Some(["email_fetch"])` — 只允许 email_fetch
- Skill B：`allowed_tools = None` — 无限制（应显示所有工具）

实际结果：system prompt 中只出现 `email_fetch`，其余工具全部被过滤掉，违反了 Skill B 的语义（`None` 表示"此 skill 对工具无限制"）。

### 根本原因

原实现用 `filter_map` 跳过 `None` 的 skill，然后对剩余的 `Some(...)` 求并集：

```rust
// 修复前
let allowed_filter = matched_skills
    .iter()
    .filter_map(|s| s.manifest.allowed_tools.as_deref())  // None 被静默丢弃
    .fold(None, |acc, names| { /* 求并集 */ });
```

`filter_map` 把 `allowed_tools=None` 的 skill 直接丢掉，其"不限制"的语义完全丢失。最终只有 `Some(...)` 的 skill 参与了过滤，结果等同于"所有参与 skill 的并集"，而非"任何一个 None 就放行全部"。

### 修复

先检查是否存在任何 `allowed_tools=None` 的 skill；若有，直接跳过过滤（`allowed_filter = None`）：

```rust
// 修复后
let any_unrestricted = matched_skills.iter().any(|s| s.manifest.allowed_tools.is_none());
let allowed_filter: Option<HashSet<&str>> = if any_unrestricted || matched_skills.is_empty() {
    None  // 无限制，显示所有工具
} else {
    Some(
        matched_skills
            .iter()
            .filter_map(|s| s.manifest.allowed_tools.as_deref())
            .flat_map(|names| names.iter().map(|n| n.as_str()))
            .collect(),
    )
};
```

### 设计说明

`allowed_tools` 的语义：

| 场景 | 结果 |
|---|---|
| 无匹配 skill | 显示所有工具 |
| 所有匹配 skill 都有 `allowed_tools=Some(...)` | 显示各 skill `allowed_tools` 的并集 |
| 任意一个匹配 skill 有 `allowed_tools=None` | 显示所有工具（该 skill 可能需要任何工具） |

---

## Bug 3: 测试断言误判（`section.contains("tool_c")` 假阳性）

### 现象

针对 `build_tools_section` 过滤逻辑的单元测试中，以下断言始终失败（即使过滤逻辑本身已正确）：

```rust
assert!(!section.contains("tool_c"), "tool_c not in any allowed_tools");
// 报错：tool_c not in any allowed_tools（断言 false）
```

### 根本原因

`build_tools_section` 生成的 section 固定包含如下模板头：

```
<tool_call>{"name": "tool_name", "args": {"param": "value"}}</tool_call>
```

字符串 `"tool_call"` 包含子串 `"tool_c"`（`t-o-o-l-_-c-a-l-l`），因此 `section.contains("tool_c")` 永远为 `true`，断言 `!section.contains("tool_c")` 永远失败，与过滤是否成功无关。

### 修复

将断言从"是否包含工具名"改为"是否包含工具标题行"，格式为 `#### \`{name}\``：

```rust
// 修复前（假阳性）
assert!(!section.contains("tool_c"), "...");

// 修复后（精确匹配工具标题行）
assert!(!section.contains("`tool_c`"), "...");
```

反引号包裹的工具名只出现在 `#### \`tool_c\`` 标题行中，不会与模板头中的 `tool_call` 冲突。

### 教训

对 section 级别的 `contains` 断言需注意模板本身可能包含的固定内容（`<tool_call>`、`tool_result`、`tool_name` 等），建议使用更精确的格式（如标题行）而非裸名称。

---

## 关联修复：增加 core 日志

上述 Bug 1 的排查依赖日志。修复期间同步在 `AgentLoop` 各关键路径添加了 `tracing` 日志：

| 位置 | 日志内容 |
|---|---|
| `chat()` 入口 | `user_input`, `turn` 编号 |
| system prompt 构建后 | 完整 system prompt（DEBUG 级别） |
| 每轮 LLM 请求 | `round` 编号 |
| LLM 响应解析后 | `tool_calls_found` 数量 |
| 工具执行 | `tool`, `args`, 执行结果 |
| 错误路径 | `error` 详情 |

CLI 端（`mobileclaw-cli`）通过 `init_logging()` 将 DEBUG+ 日志写入当前目录的 `mclaw.log`，日志级别由环境变量 `MCLAW_LOG` 控制（默认 `debug`）。
