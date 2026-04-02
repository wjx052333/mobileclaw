# Bug: OpenAI-compat 客户端静默丢弃 native tool_calls

**日期：** 2026-04-02  
**文件：** `mobileclaw-core/src/llm/openai_compat.rs`  
**严重程度：** 高（导致模型响应为空，bench 30% 轮次 resp_ch=0）

---

## 现象

在 `mclaw bench` 50 轮压测中，约 15/50 轮出现 `resp_ch=0`，对应 assistant history 消息内容为空字符串。这些空响应耗时与正常轮次相当（~50s），说明模型**确实被调用并返回了内容**，只是被静默丢弃了。

从 `bench_interactions_50.jsonl` 交互日志确认：
- `history_before/after` 每轮精确 +2，上下文累积**完全正常**
- `events_seen` 空响应轮次只有 `['ContextStats', 'Done']`，无 `TextDelta`
- `history_after[-1].content = ''`：模型回答被存为空字符串

从 `mclaw.log` 确认：
```
OpenAiCompatClient: streaming response started status=200 OK   ← HTTP 200 正常
[62 秒后]
LLM response received round=0 response_len=0 response=         ← 内容为空
```

---

## 根本原因

`parse_openai_event`（以及 `stream_messages` 中的内联逻辑）只读取 `choices[0].delta.content` 提取文本，对 `choices[0].delta.tool_calls` 字段**完全忽略（返回 None 静默跳过）**：

```rust
// 修复前
let text = v["choices"][0]["delta"]["content"]
    .as_str()
    .unwrap_or("")
    .to_string();
if text.is_empty() {
    Ok(None)   // ← tool_calls 分片全部走这条路，静默丢弃
} else {
    Ok(Some(StreamEvent::TextDelta { text }))
}
```

当 OpenAI-compat 模型（如 `step-3.5-flash`、OpenRouter 上的各模型）收到含工具描述的 system prompt 后，倾向于用 **native function calling 格式**响应（`choices[0].delta.tool_calls`），而不是在 content 文本里嵌入 XML。这些分片全部被过滤，`full_text` 保持空字符串，最终：
- `extract_tool_calls("")` 返回空 → 不触发工具执行
- `Message::assistant("")` 存入 history → 上下文积累了空消息
- 模型后续轮次看到空的 assistant 消息，行为越来越异常

OpenAI streaming tool_calls 的 SSE 分片格式：
```
chunk 1: choices[0].delta.tool_calls[{index:0, id:"c1", type:"function", function:{name:"memory_recall", arguments:""}}]
chunk 2: choices[0].delta.tool_calls[{index:0, function:{arguments:"{\"query\":"}}]
chunk 3: choices[0].delta.tool_calls[{index:0, function:{arguments:"\"rust async\"}"}}]
[DONE]
```

---

## 为什么 ironclaw 没有此问题

ironclaw 和 mobileclaw 是定位不同的 AI Agent 项目：

| 维度 | ironclaw | mobileclaw |
|------|----------|------------|
| **目标平台** | 服务器/桌面进程（Rust binary + HTTP server） | Flutter 移动 App（Rust core via FFI） |
| **对外接口** | 暴露 OpenAI 兼容 HTTP API（`channels/web/openai_compat.rs` 是**服务端接收器**，接受外部 OpenAI 客户端连接） | Flutter FFI（`AgentSession` 跨 Dart/Rust 边界） |
| **LLM 接入** | 8+ 后端：NEAR AI、OpenAI Codex、AWS Bedrock、GitHub Copilot、Gemini、Anthropic… | Anthropic + OpenAI-compat + Ollama |
| **工具调用格式** | **native function calling**（rig-core 库统一处理，`complete_with_tools()` 返回结构化 `ToolCall`，不需要解析 SSE 分片） | **XML-in-text 自定义格式**（`<tool_call>` 嵌入响应文本） |
| **响应方式** | **非流式**，blocking 等全量结果，rig-core 做 JSON 反序列化 | **SSE 流式**，逐事件解析 `choices[0].delta.*` |
| **韧性层** | circuit breaker + retry + failover + smart routing | 无 |
| **认证** | OAuth 流程（NEAR AI session token、Codex device flow、Copilot token exchange） | API key only |
| **LLM 抽象** | rig-core 库 + RigAdapter（不直接解析 SSE） | 自实现 HTTP 客户端，直接解析 SSE 分片流 |

ironclaw 的 `src/channels/web/openai_compat.rs` 是**服务端接收器**（让外部 OpenAI 客户端能连接到 ironclaw），与 mobileclaw 的 `openai_compat.rs`（作为 OpenAI API 的客户端）角色完全相反。

ironclaw 使用 rig-core 的 `complete_with_tools()` 方法，返回完整的 `ToolCompletionResponse { tool_calls: Vec<ToolCall>, ... }`，native tool_calls 由 rig-core 解析，根本不需要处理 `choices[0].delta.tool_calls` 的 SSE 分片。该 bug 在 ironclaw 中没有对应代码路径。

---

## 修复方案

在 `stream_messages` 中引入 **`ToolCallAcc`** 状态机，跨多个 SSE 事件累积 tool_calls 分片，在 `[DONE]` 时转换为 agent 兼容的 XML TextDelta：

```rust
// ─── ToolCallAcc ──────────────────────────────────────────────────────────────
// 跨 SSE 事件累积 choices[0].delta.tool_calls 分片
// [DONE] 时 to_xml() → "<tool_call>{...}</tool_call>" → agent XML parser 正常处理
#[derive(Default)]
pub(crate) struct ToolCallAcc {
    calls: BTreeMap<usize, ToolCallEntry>,  // index → entry
}

// ─── stream_messages 修复 ─────────────────────────────────────────────────────
let tool_acc = Arc::new(Mutex::new(ToolCallAcc::default()));
let data_stream = resp.bytes_stream().eventsource().filter_map(move |ev| {
    let acc = tool_acc.clone();
    async move {
        match ev {
            Ok(e) if e.data == "[DONE]" => {
                let locked = acc.lock().unwrap();
                if locked.has_calls() {
                    // native tool_calls → XML TextDelta，agent XML parser 照常处理
                    Some(Ok(StreamEvent::TextDelta { text: locked.to_xml() }))
                } else {
                    Some(Ok(StreamEvent::MessageStop))
                }
            }
            Ok(e) => {
                let v = serde_json::from_str(&e.data)?;
                acc.lock().unwrap().feed(&v);  // 累积 tool_calls 分片
                // 同时提取 content 文本（模型也可能同时返回 content + tool_calls）
                let text = v["choices"][0]["delta"]["content"].as_str().unwrap_or("");
                if text.is_empty() { None } else { Some(Ok(StreamEvent::TextDelta { text.into() })) }
            }
            Err(e) => Some(Err(ClawError::Llm(e.to_string()))),
        }
    }
});
```

**兼容性：**
- 模型只返回 content → 行为与修复前完全相同
- 模型只返回 tool_calls → `[DONE]` 时发出 XML TextDelta
- 模型同时返回 content + tool_calls → text 先流出，XML 在 `[DONE]` 追加
- tool_calls arguments JSON 截断 → 降级为 `{}`，不 panic

---

## 新增测试（16 个，全部通过）

| 测试 | 覆盖点 |
|------|--------|
| `test_parse_text_content` | content 提取正常 |
| `test_parse_role_only_skipped` | role-only delta 返回 None |
| `test_parse_done_sentinel` | `[DONE]` 返回 MessageStop |
| `test_parse_null_content_skipped` | null content 返回 None |
| `test_parse_tool_calls_only_returns_none` | tool_calls-only chunk 返回 None（acc 负责） |
| `test_parse_never_panics` (proptest) | 任意输入不 panic |
| `test_acc_empty_initially` | 初始状态 |
| `test_acc_single_tool_call_single_chunk` | 单工具单分片 |
| `test_acc_arguments_assembled_from_chunks` | 三分片拼接 arguments |
| `test_acc_multiple_tool_calls` | 多工具按 index 排序 |
| `test_acc_xml_is_parseable_by_agent` | to_xml() 被 `extract_tool_calls` 正确解析 |
| `test_acc_malformed_arguments_fallback_to_empty_object` | 截断 JSON 降级 `{}` |
| `test_acc_no_tool_calls_field_is_noop` | 无 tool_calls 字段时 acc 不变 |
| `test_acc_feed_ignores_missing_choices` | 畸形 JSON 不 panic |
| `test_acc_feed_never_panics` (proptest) | 任意 JSON Value 不 panic |
| `test_normalise_base_url_appends_v1` | URL 规范化 |

---

## 当前状态

**代码已修复**（worktree `feat+memory-optimization`，commit 待提交），**需 `cargo build -p mobileclaw-cli --release` 重新构建后生效。**

最新 bench 运行（修复前 binary）：
```
turn  1  resp_ch=15507  ✅
turn  2  resp_ch=0      ← tool_calls 被丢弃（旧 binary，修复未生效）
turn  3  Error: 429 Too Many Requests  ← 独立问题，见下方
```

turn 2 的 `resp_ch=0` 确认 bug 复现，等待重新构建后验证。

---

## 关联问题：bench 遭遇 429 直接崩溃

错误信息：
```
Error: turn 3 chat failed
Caused by:
    llm error: OpenAI-compat 429 Too Many Requests:
    {"error":{"message":"litellm.RateLimitError: ...OpenrouterException - rate_limit_exceeded"...}}
```

**原因：** bench 命令在 `session.chat()` 返回 Err 时使用 `?` 直接传播，整个 bench 进程终止。

```rust
// bench.rs 第 184 行（修复前）
let events = session
    .chat(turn.prompt.clone(), system.clone())
    .await
    .with_context(|| format!("turn {} chat failed", turn.id))?;  // ← ? 直接崩溃
```

429 是临时的 rate limit 错误，不应终止整个压测。**已修复**（见下方 bench 错误处理修复）。
