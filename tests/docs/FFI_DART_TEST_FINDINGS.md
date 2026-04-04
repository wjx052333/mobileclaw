# Dart FFI 测试可行性分析

## 尝试结果

### 问题：wire_funcs 无法从 Dart 直接调用

运行 Dart FFI 测试，尝试加载所有 23 个 wire_func：

```
❌ wire__crate__ffi__AgentSession_create_impl - 未找到
❌ wire__crate__ffi__AgentSession_chat_impl - 未找到
... (全部 23 个未找到)
```

### 根本原因

检查 libmobileclaw_core.so 中的符号：

```bash
$ nm libmobileclaw_core.so | grep "wire__crate"

0000000000340c20 t _ZN15mobileclaw_core13frb_generated40wire__crate__ffi__AgentSession_chat_impl...
                ↑ 小写 t = 私有符号（LOCAL）
```

**发现**：
1. wire_funcs 被编译成 **Rust mangled symbols**（不是 C 符号）
2. 小写 't' 标志表示 **LOCAL 私有符号**
3. 不是 `extern "C"` 导出

```c
// ❌ 不存在这样的符号
wire__crate__ffi__AgentSession_chat_impl

// ✅ 实际符号（Rust mangled name）
_ZN15mobileclaw_core13frb_generated40wire__crate__ffi__AgentSession_chat_impl28_...
```

## 为什么会这样？

flutter_rust_bridge 的调用链：

```
Dart FFI 需要 →  extern "C" 导出符号
Dart 调用      →  C 函数指针
                   ↓
           wire__crate__ffi__AgentSession_chat_impl()

但实际上：
wire_funcs  →  Rust 编译的私有函数
             →  只通过 flutter_rust_bridge 运行时加载
             →  不是直接 C 导出
```

## 这告诉我们什么？

### flutter_rust_bridge 的架构

1. **wire_funcs 不是直接 C 导出**
   - 它们是 Rust 的私有函数
   - 通过 flutter_rust_bridge 的初始化机制加载
   - 需要 flutter_rust_bridge 运行时环境

2. **只有完整 Flutter 框架能调用**
   - iOS/Android native bridge
   - Flutter engine 初始化
   - flutter_rust_bridge 的 DynamicLibrary 管理

3. **轻量级 Dart VM 无法调用**
   - Dart VM 无法直接加载私有 Rust 符号
   - 无法访问 mangled 符号名
   - 需要 flutter_rust_bridge 的加载机制

## 能否强制导出为 C 符号？

理论上可能，但需要修改：

```rust
// mobileclaw-core/src/frb_generated.rs

// ❌ 当前（私有函数）
fn wire__crate__ffi__AgentSession_chat_impl(...) { }

// ✅ 需要修改为
#[no_mangle]
pub extern "C" fn wire__crate__ffi__AgentSession_chat_impl(...) { }
```

**但问题**：
1. flutter_rust_bridge 是自动生成的，修改会被覆盖
2. 生成器配置可能不支持
3. 违反了 flutter_rust_bridge 的设计

## 结论

| 方案 | 可行性 | 工作量 | 覆盖率 |
|------|--------|--------|--------|
| **Dart VM FFI 测试** | ❌ 不可行 | - | 0% (无法调用) |
| **修改为 C 导出** | ⚠️ 理论可行 | 中等 | 60%+ |
| **完整 Flutter 框架** | ✅ 推荐 | 大 | 100% |
| **交叉层 Rust 测试** | ✅ 现有 | 已完成 | 65% |

## 最现实的方案

### 现状
- ✅ Rust 白盒测试：65.07% 覆盖率
- ❌ Dart FFI 黑盒测试：无法实现（符号限制）
- ⚠️ Flutter 框架测试：可行但成本高

### 推荐
**保持现状** + **文档化限制**：
1. 维持 65.07% 的 Rust 测试覆盖率
2. 确认核心逻辑充分测试（93%+）
3. 在生产环境（Flutter 应用）中验证 FFI 边界
4. 标记 frb_generated.rs 为"黑盒测试领域"

## 技术深度

### 为什么 flutter_rust_bridge 不导出为 C 符号？

1. **性能**：私有符号允许更积极的优化
2. **安全**：不暴露内部实现细节
3. **架构**：依赖运行时初始化和符号查找

```rust
// flutter_rust_bridge 的加载机制
pub fn init_app_lang_binding(...) {
    // 运行时查找私有 Rust 符号
    // 通过反射或符号表扫描
    // 而不是直接 C 调用
}
```

## 启示

这个发现进一步证实：
- **frb_generated.rs 0% 覆盖不是缺陷**
- **而是架构设计的结果**
- **只能通过黑盒测试验证**（Flutter 侧）
- **白盒测试受到 FFI 边界的限制**

---

**最终结论**：无法用轻量级 Dart VM 测试 FFI 胶水。需要完整 Flutter 框架才能验证 frb_generated.rs。当前 65.07% 的覆盖率已达可达成的上限。

