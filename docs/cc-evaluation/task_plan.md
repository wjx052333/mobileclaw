# Task Plan: Mobile Claw Research & Design

## Goal
研究 ~/agent_eyes/thirdparty/bot 下的 claude-code、zeroclaw、ironclaw 和 nanoclaw，形成：
1. **研读报告**：各项目的架构、长期 memory 管理、工具隔离机制
2. **调研报告**：手机 claw 的技术难点（skill 引入、工具调用、WASM 容器、Rust 扩展）

## Phases

### Phase 1: 代码研读 [✅ DONE]
- [x] 研读 claude-code 架构（memory、tool、skill 机制）
- [x] 研读 zeroclaw 架构
- [x] 研读 ironclaw 架构
- [x] 研读 nanoclaw 架构
- [x] 汇总对比：memory 管理 & 工具隔离

### Phase 2: 形成研读报告 [✅ DONE — findings.md 第一部分]
- [x] 撰写各项目对比分析
- [x] 重点：长期 memory 管理方案对比（含对比表）
- [x] 重点：工具隔离机制对比（含对比表）
- [x] Skill 机制对比（含对比表）

### Phase 3: 手机 claw 难点调研 [✅ DONE — findings.md 第二部分]
- [x] 调研 Rust/WASM 在移动端的可行性（iOS JIT限制、wasm3/wamr方案）
- [x] 调研 Skill 引入机制（WASM Skill包 + 纯提示型Skill）
- [x] 调研工具调用机制（三类工具 + XML协议 + WASM边界）
- [x] 调研 Rust 扩展工具（Tool Trait + ToolRegistry设计）
- [x] 调研 Memory 持久化（SQLite + FTS5，移动端方案）
- [x] 调研 Flutter SDK 封装（flutter_rust_bridge，分层架构）

### Phase 4: 形成调研报告 [✅ DONE — findings.md 第二部分]
- [x] 技术选型推荐表
- [x] 分阶段实施路径（MVP → 扩展 → 完整）
- [x] 关键风险与缓解措施

## Decisions
- 推荐：Rust Native（.so/.a）作为 Core，wasm3/wamr 作为 Skill 沙箱
- 推荐：flutter_rust_bridge 2.x 作为 Flutter 绑定方案
- 推荐：SQLite + FTS5 作为初版 Memory，后期引入 fastembed-rs 向量
- 推荐：YAML frontmatter + Markdown 作为 Skill 格式（与 claude-code/ironclaw 生态兼容）
- 推荐：XML tool_call 格式（借鉴 zeroclaw，解析健壮）
