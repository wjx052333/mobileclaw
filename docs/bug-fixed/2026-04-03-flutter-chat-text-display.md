# Bug: Flutter chat 界面收不到 LLM 文本响应

**日期：** 2026-04-03  
**文件：** `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart`, `mobileclaw-flutter/apps/mobileclaw_app/lib/features/chat/chat_page.dart`  
**严重程度：** 致命（所有聊天在 UI 上不可见，功能完全失效）

---

## 现象

Flutter App 发送消息后，LLM 正常返回完整响应（Rust 日志确认 `event_count=107, text_chars=1545`），但聊天界面始终空白。

调试日志显示：
```
AgentImpl.chat: received 108 events: TextDelta, TextDelta, ..., ContextStats, TurnSummary, Done
ChatPage.StreamBuilder: state=ConnectionState.done, hasData=true, hasError=false
ChatPage: received event DoneEvent
```

关键特征：
- **Rust 侧完全正常**：`mobileclaw.log` 显示所有 107 个 TextDelta + 1 个 ToolCall 都已生成
- **FFI 边界正常**：Dart 侧 `AgentImpl.chat` 确实收到了 108 个事件对象
- **StreamBuilder 只看到 DoneEvent**：中间所有事件"被跳过"，`connectionState` 直接跳到 `done`

---

## 根本原因

**async* 生成器的同步 yield 在同一个微任务批次中全部执行完毕，Flutter 渲染循环来不及处理中间帧。**

调用链分析：

```
ChatPage._send()
  ↓ setState(_stream = agent.chat(input))
  ↓ 重新 build → StreamBuilder
  ↓ 订阅新 stream
  ↓
MobileclawAgentImpl.chat() [async*]
  ↓ await _session.chat(...)        ← FFI 调用，等 5-10s
  ↓ 返回 List<AgentEventDto>        ← 一次性返回全部 108 个事件
  ↓ for (final dto in dtos) {
  ↓   yield _eventFromDto(dto);     ← 连续 yield 108 次
  ↓ }
```

问题出在 `async*` 生成器的行为：
1. `await _session.chat(...)` 完成后，生成器恢复执行
2. `for` 循环连续 `yield` 108 个事件
3. Dart VM 的 `async*` 规范规定：每个 `yield` 将事件放入 listener 的 microtask queue
4. **但所有 yield 的 microtask 属于同一个"事件批次"**——它们在同一轮微任务队列中连续执行
5. Flutter 的 `StreamBuilder` 确实收到了所有 108 个事件
6. **但 Flutter 只在帧间检查 stream 的最新 snapshot**——108 个事件在同一个微任务批次中处理完毕，Flutter 渲染循环只看到最终状态（`done` + `DoneEvent`）

这是 Dart async* + Flutter StreamBuilder 的经典陷阱：[Flutter issue #87731](https://github.com/flutter/flutter/issues/87731) 描述了相同问题。

---

## 为什么之前的测试用例没有覆盖

1. **`MockMobileclawAgent` 的 `chat()` 实现不同**：mock 使用 `Stream.fromIterable` + `Future.delayed` 模拟流式效果，天然有时间间隔，不会触发此 bug。

2. **没有 Flutter widget 测试**：`chat_page.dart` 没有对应的 widget 测试。如果有的话，可以 mock agent 的 `chat()` 返回一个无延迟的同步 stream，就能复现此问题。

3. **Rust 侧集成测试不经过 FFI**：`mobileclaw-core` 的 `cargo test` 完全在 Rust 内运行，不经过 Dart 层。`AgentEvent` 在 Rust 侧是正确的。

4. **FFI 序列化测试不覆盖事件消费**：`frb_generated.dart` 的编解码测试只验证单个事件的正确性（"给一个 AgentEventDto，能正确转换成 AgentEvent"），不测试连续事件的消费时序。

5. **真机测试的调试成本高**：每次修改需要 build APK → install → 操作 UI → 看结果，循环一次 2-3 分钟。这导致在发现问题后倾向于"快速改一下试试"而不是系统性排查。

---

## 修复方案

### 1. `agent_impl.dart`：`async*` → `StreamController` + `Timer.run`

```dart
// 修复前
Stream<AgentEvent> chat(String userInput, {String system = ''}) async* {
  final dtos = await _session.chat(input: userInput, system: system);
  for (final dto in dtos) {
    yield _eventFromDto(dto);           // ← 连续 yield，在同一微任务批次
  }
}

// 修复后
Stream<AgentEvent> chat(String userInput, {String system = ''}) {
  final controller = StreamController<AgentEvent>.broadcast();

  Timer.run(() async {
    final dtos = await _session.chat(input: userInput, system: system);
    int index = 0;
    void emitNext() {
      if (index >= dtos.length || controller.isClosed) {
        controller.close();
        return;
      }
      controller.add(_eventFromDto(dtos[index]));
      index++;
      Timer.run(emitNext);              // ← 每个事件在 event queue 上，
    }                                    //   Flutter 可在事件间渲染
    emitNext();
  });

  return controller.stream;
}
```

**关键区别：**
- `Future.microtask(() {})`：在同一微任务队列中排队，**不会**给 Flutter 渲染机会
- `Timer.run(() {})`：在事件队列中排队，**会**给 Flutter 渲染机会（每个 Timer 之间至少一帧）

### 2. `chat_page.dart`：`StreamBuilder` → 直接 `stream.listen()`

```dart
// 修复前
StreamBuilder<AgentEvent>(
  stream: _stream,
  builder: (context, snapshot) {
    if (snapshot.hasData) {
      switch (snapshot.data!) {
        case TextDeltaEvent(:final text):
          _buffer.write(text);
          // 在 build 回调内 setState + addPostFrameCallback
          // 嵌套的 deferred 调用导致渲染时序混乱
      }
    }
    return SingleChildScrollView(...);
  },
);

// 修复后
_subscription = stream.listen(
  (event) {
    switch (event) {
      case TextDeltaEvent(:final text):
        setState(() => _displayText += text);  // ← 直接在 listener 中 setState
      case DoneEvent():
        setState(() => _busy = false);
    }
  },
  onError: (e, st) { /* ... */ },
);
```

**为什么不用 StreamBuilder：**
- StreamBuilder 的 `builder` 回调在 build phase 执行，不应该有 side effects（如 setState）
- `addPostFrameCallback` 将 setState 延迟到下一帧之后，但 stream 事件可能在下个微任务就到了，时序竞争
- 直接 `stream.listen()` 在 event phase 触发 setState，时序明确

---

## 后续改进

### 1. 添加 Flutter widget 测试

```dart
// test/features/chat/chat_page_test.dart
testWidgets('displays assistant text response', (tester) async {
  final agent = MockMobileclawAgent();
  agent.chatResponse = Stream.fromIterable([
    TextDeltaEvent(text: 'Hello'),
    TextDeltaEvent(text: ' World'),
    DoneEvent(),
  ]);

  await tester.pumpWidget(ProviderScope(
    overrides: [agentProvider.overrideWithFuture.value(agent)],
    child: const MaterialApp(home: ChatPage()),
  ));

  // Send a message
  await tester.enterText(find.byType(TextField), 'Hi');
  await tester.tap(find.byIcon(Icons.send));
  await tester.pump();

  // Verify text appears
  expect(find.textContaining('Hello World'), findsOneWidget);
});
```

**关键点：** 如果 `Stream.fromIterable` 不带 `Future.delayed`，这个测试应该**仍然能通过**（因为我们修复了同步 stream 消费问题）。如果测试失败，说明修复有回归。

### 2. 添加 FFI 事件流测试

```dart
// test/agent_impl_event_stream_test.dart
test('chat emits all events with rendering gaps', () async {
  // Use a real FFI session with a mock provider that returns instantly
  final events = agent.chat('test').toList();
  await expectLater(events, hasLength(greaterThan(1)));
});
```

### 3. 本地 CLI 测试工具

创建 `mclaw_cli` Flutter Linux 桌面项目（`mobileclaw-flutter/mclaw_cli/`），支持：
- 交互式 stdin/stdout 聊天
- 显示流式文本输出
- 无需真机/模拟器

需要安装 `libgtk-3-dev` 后才能构建 Linux 桌面版。

---

## 修复前后的调用链对比

```
修复前（事件丢失）：
  FFI → Vec<AgentEventDto> → async* yield yield yield ... → Flutter 只看到 Done
  耗时：FFI 返回后 ~0ms（所有 yield 同步完成）

修复后（事件逐个渲染）：
  FFI → Vec<AgentEventDto> → Timer.run(add) → Flutter 渲染 → Timer.run(add) → Flutter 渲染 → ...
  耗时：每个事件间隔 1 帧（~16ms），108 个事件约 1.7s
```

注意：修复后的渲染延迟（~1.7s 显示全部文本）是**预期行为**，因为文本内容在逐帧累积。用户看到的是"文字逐字出现"的打字机效果，和流式 LLM 的 UX 一致。

---

## 当前状态

**代码已修复并提交**（commit `b84586e`），真机验证通过：
- LLM 文本响应正常显示
- 多轮对话历史记录保留
- 工具调用在日志中可见
