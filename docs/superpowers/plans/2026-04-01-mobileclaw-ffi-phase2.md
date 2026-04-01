# MobileClaw Flutter Phase 2: FFI Binding Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire `mobileclaw-core` (Rust) to `mobileclaw_sdk` (Flutter/Dart) via `flutter_rust_bridge 2.x`, replacing `MockMobileclawAgent` with a real FFI-backed implementation on Linux.

**Architecture:** Add a non-generic `AgentSession` wrapper in `mobileclaw-core/src/ffi.rs` that monomorphises `AgentLoop<ClaudeClient>`. Run frb codegen to produce Dart bindings. Implement a real `MobileclawAgentImpl` in the SDK that delegates to the bridge. Keep mock as a dev-only fallback.

**Tech Stack:** `flutter_rust_bridge 2.x`, `flutter_rust_bridge_codegen` CLI, `cargo` (Linux `.so`), `flutter test`

---

## File Map

### Rust (modify / create)

| File | Action | Purpose |
|------|--------|---------|
| `mobileclaw-core/Cargo.toml` | modify | Add `flutter_rust_bridge`, `crate-type = ["cdylib","lib"]` |
| `Cargo.toml` (workspace) | modify | Add `flutter_rust_bridge` to workspace deps |
| `mobileclaw-core/src/lib.rs` | modify | Expose `pub mod ffi` |
| `mobileclaw-core/src/ffi.rs` | create | `AgentSession` + all DTOs; frb annotations |
| `mobileclaw-core/src/agent/loop_impl.rs` | modify | Add `pub fn skills()` getter |
| `mobileclaw-core/tests/integration_agent.rs` | modify | Add `#![cfg(feature="test-utils")]` guard |

### Flutter / Dart (modify / create)

| File | Action | Purpose |
|------|--------|---------|
| `mobileclaw-flutter/packages/mobileclaw_sdk/pubspec.yaml` | modify | Add `flutter_rust_bridge` runtime + dev dep |
| `mobileclaw-flutter/packages/mobileclaw_sdk/flutter_rust_bridge.yaml` | create | codegen config (rust_input → dart_output) |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/bridge/` | generated | frb-generated Dart bindings (commit) |
| `mobileclaw-flutter/packages/mobileclaw_sdk/linux/libmobileclaw_core.so` | built | native library for Linux tests |
| `mobileclaw-flutter/packages/mobileclaw_sdk/linux/CMakeLists.txt` | modify | Link `libmobileclaw_core.so` |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart` | create | Real `MobileclawAgentImpl` using bridge |
| `mobileclaw-flutter/packages/mobileclaw_sdk/lib/mobileclaw_sdk.dart` | modify | Export `agent_impl.dart` |
| `mobileclaw-flutter/apps/mobileclaw_app/lib/core/engine_provider.dart` | modify | Use real impl with mock fallback |
| `mobileclaw-flutter/packages/mobileclaw_sdk/test/mobileclaw_sdk_test.dart` | modify | Add integration tests for real bridge |

---

## Task 1: Fix Rust test failures

**Files:**
- Modify: `mobileclaw-core/tests/integration_agent.rs:1`

The test file imports `MockLlmClient` which only exists under `--features test-utils`. Without that flag, all 3 tests fail to compile.

- [ ] **Step 1.1 — Reproduce the failure**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw
~/.cargo/bin/cargo test -p mobileclaw-core 2>&1 | head -30
```

Expected: compile errors about `MockLlmClient` / type annotations.

- [ ] **Step 1.2 — Add feature guard at top of file**

Open `mobileclaw-core/tests/integration_agent.rs` and add as the very first line:

```rust
#![cfg(feature = "test-utils")]
```

- [ ] **Step 1.3 — Verify tests compile and pass without the feature**

```bash
~/.cargo/bin/cargo test -p mobileclaw-core 2>&1 | tail -10
```

Expected: `test result: ok. 0 passed; 0 failed` (file skipped).

- [ ] **Step 1.4 — Verify tests pass with the feature**

```bash
~/.cargo/bin/cargo test -p mobileclaw-core --features test-utils 2>&1 | tail -10
```

Expected: `test result: ok. N passed; 0 failed`.

- [ ] **Step 1.5 — Commit**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev
git add mobileclaw-core/tests/integration_agent.rs
git commit -m "fix: gate integration_agent tests behind test-utils feature flag"
```

---

## Task 2: Add `skills()` getter to `AgentLoop`

**Files:**
- Modify: `mobileclaw-core/src/agent/loop_impl.rs`

The FFI layer needs to enumerate loaded skills. Currently `skill_mgr` is private.

- [ ] **Step 2.1 — Write the failing test**

Add to the `#[cfg(test)]` block at the bottom of `mobileclaw-core/src/agent/loop_impl.rs`:

```rust
#[cfg(test)]
mod tests {
    // existing tests ...

    #[tokio::test]
    #[cfg(feature = "test-utils")]
    async fn skills_getter_returns_loaded_skills() {
        use crate::{
            llm::client::test_helpers::MockLlmClient,
            skill::{Skill, SkillManifest, SkillActivation, SkillTrust},
            tools::{ToolContext, ToolRegistry, PermissionChecker},
            memory::sqlite::SqliteMemory,
        };
        use std::sync::Arc;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let mem = Arc::new(SqliteMemory::open(dir.path().join("m.db")).await.unwrap());
        let ctx = ToolContext {
            memory: mem,
            sandbox_dir: dir.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
        };
        let skill = Skill {
            manifest: SkillManifest {
                name: "test-skill".into(),
                description: "A test skill".into(),
                trust: SkillTrust::Bundled,
                activation: SkillActivation { keywords: vec!["test".into()] },
                allowed_tools: None,
            },
            prompt: "You are a test skill.".into(),
        };
        let agent = AgentLoop::new(
            MockLlmClient { response: "ok".into() },
            ToolRegistry::new(),
            ctx,
            SkillManager::new(vec![skill]),
        );
        assert_eq!(agent.skills().len(), 1);
        assert_eq!(agent.skills()[0].manifest.name, "test-skill");
    }
}
```

- [ ] **Step 2.2 — Run to verify failure**

```bash
~/.cargo/bin/cargo test -p mobileclaw-core --features test-utils skills_getter 2>&1 | tail -10
```

Expected: compile error — `no method named 'skills'`.

- [ ] **Step 2.3 — Add the getter**

In `mobileclaw-core/src/agent/loop_impl.rs`, inside `impl<L: LlmClient> AgentLoop<L>`, after `pub fn history()`:

```rust
/// Returns a reference to the loaded skills.
pub fn skills(&self) -> &[crate::skill::Skill] {
    self.skill_mgr.skills()
}
```

- [ ] **Step 2.4 — Run to verify pass**

```bash
~/.cargo/bin/cargo test -p mobileclaw-core --features test-utils skills_getter 2>&1 | tail -5
```

Expected: `test result: ok. 1 passed`.

- [ ] **Step 2.5 — Run full suite**

```bash
~/.cargo/bin/cargo test -p mobileclaw-core --features test-utils 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 2.6 — Commit**

```bash
git add mobileclaw-core/src/agent/loop_impl.rs
git commit -m "feat(agent): add skills() getter to AgentLoop"
```

---

## Task 3: Add `flutter_rust_bridge` to workspace + crate

**Files:**
- Modify: `Cargo.toml` (workspace)
- Modify: `mobileclaw-core/Cargo.toml`

- [ ] **Step 3.1 — Add to workspace deps**

In `/home/wjx/agent_eyes/bot/mobileclaw/Cargo.toml`, append to `[workspace.dependencies]`:

```toml
flutter_rust_bridge = "2"
```

- [ ] **Step 3.2 — Add to crate deps and set crate-type**

In `mobileclaw-core/Cargo.toml`, add after the `[package]` section:

```toml
[lib]
crate-type = ["cdylib", "lib"]
```

And add to `[dependencies]`:

```toml
flutter_rust_bridge = { workspace = true }
```

- [ ] **Step 3.3 — Verify crate compiles**

```bash
~/.cargo/bin/cargo build -p mobileclaw-core 2>&1 | tail -10
```

Expected: `Finished` — no errors. (A warning about unused `cdylib` is fine.)

- [ ] **Step 3.4 — Commit**

```bash
git add Cargo.toml mobileclaw-core/Cargo.toml Cargo.lock
git commit -m "build: add flutter_rust_bridge 2.x to workspace and mobileclaw-core"
```

---

## Task 4: Create `mobileclaw-core/src/ffi.rs`

**Files:**
- Create: `mobileclaw-core/src/ffi.rs`
- Modify: `mobileclaw-core/src/lib.rs`

This is the single API surface that frb codegen reads.

- [ ] **Step 4.1 — Expose `ffi` module in lib.rs**

In `mobileclaw-core/src/lib.rs` add:

```rust
pub mod ffi;
```

- [ ] **Step 4.2 — Create ffi.rs with DTOs and AgentSession**

Create `mobileclaw-core/src/ffi.rs`:

```rust
//! FFI API layer — the only module exposed to flutter_rust_bridge codegen.
//!
//! All public types here must use only primitive/String/Vec types
//! or `#[frb(opaque)]` wrappers. No references, no generics, no Arcs.

use flutter_rust_bridge::frb;
use std::sync::Arc;
use std::path::Path;

use crate::{
    agent::{AgentLoop, AgentEvent},
    llm::client::ClaudeClient,
    memory::{sqlite::SqliteMemory, traits::Memory, types::{MemoryCategory, SearchQuery}},
    tools::{
        ToolContext, ToolRegistry, PermissionChecker,
        builtin::register_all_builtins,
    },
    skill::{SkillManager, SkillTrust, loader::load_skills_from_dir},
    error::ClawResult,
};

// ─────────────────────────────────────────────────────────────────────────────
// DTOs — plain data, safe to cross FFI boundary
// ─────────────────────────────────────────────────────────────────────────────

/// Config passed from Dart when creating an agent session.
pub struct AgentConfig {
    /// Anthropic API key (plaintext for now; Phase 3 will use keystore alias).
    pub api_key: String,
    /// Absolute path to the SQLite database file.
    pub db_path: String,
    /// Root directory for the file-system sandbox tools.
    pub sandbox_dir: String,
    /// URL prefixes the HTTP tool may fetch (e.g. "https://api.example.com/").
    pub http_allowlist: Vec<String>,
    /// LLM model identifier (e.g. "claude-opus-4-6").
    pub model: String,
    /// Optional path to a directory of skill bundles.
    pub skills_dir: Option<String>,
}

/// One event emitted during a chat turn.
pub enum AgentEventDto {
    TextDelta { text: String },
    ToolCall  { name: String },
    ToolResult { name: String, success: bool },
    Done,
}

impl From<AgentEvent> for AgentEventDto {
    fn from(e: AgentEvent) -> Self {
        match e {
            AgentEvent::TextDelta { text }          => AgentEventDto::TextDelta { text },
            AgentEvent::ToolCall { name }            => AgentEventDto::ToolCall { name },
            AgentEvent::ToolResult { name, success } => AgentEventDto::ToolResult { name, success },
            AgentEvent::Done                         => AgentEventDto::Done,
        }
    }
}

/// One turn in the conversation history.
pub struct MessageDto {
    /// "user" or "assistant"
    pub role: String,
    pub content: String,
}

/// Metadata for a loaded skill bundle.
pub struct SkillManifestDto {
    pub name: String,
    pub description: String,
    /// "bundled" | "installed"
    pub trust: String,
    pub keywords: Vec<String>,
    pub allowed_tools: Option<Vec<String>>,
}

/// A stored memory document.
pub struct MemoryDocDto {
    pub id: String,
    pub path: String,
    pub content: String,
    /// "core" | "daily" | "conversation" | "custom:<label>"
    pub category: String,
    pub created_at: u64,
    pub updated_at: u64,
}

/// A memory search result.
pub struct SearchResultDto {
    pub doc: MemoryDocDto,
    pub score: f32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers — DTO conversions
// ─────────────────────────────────────────────────────────────────────────────

fn category_to_string(c: &MemoryCategory) -> String {
    match c {
        MemoryCategory::Core         => "core".into(),
        MemoryCategory::Daily        => "daily".into(),
        MemoryCategory::Conversation => "conversation".into(),
        MemoryCategory::Custom(s)    => format!("custom:{s}"),
    }
}

fn string_to_category(s: &str) -> MemoryCategory {
    match s {
        "core"         => MemoryCategory::Core,
        "daily"        => MemoryCategory::Daily,
        "conversation" => MemoryCategory::Conversation,
        other => {
            let label = other.strip_prefix("custom:").unwrap_or(other);
            MemoryCategory::Custom(label.into())
        }
    }
}

fn doc_to_dto(doc: crate::memory::types::MemoryDoc) -> MemoryDocDto {
    MemoryDocDto {
        id:         doc.id,
        path:       doc.path,
        content:    doc.content,
        category:   category_to_string(&doc.category),
        created_at: doc.created_at,
        updated_at: doc.updated_at,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AgentSession — opaque handle exposed to Dart
// ─────────────────────────────────────────────────────────────────────────────

/// Non-generic wrapper around `AgentLoop<ClaudeClient>`.
/// Opaque to Dart — all interaction is via methods below.
#[frb(opaque)]
pub struct AgentSession {
    inner:  AgentLoop<ClaudeClient>,
    memory: Arc<SqliteMemory>,
}

impl AgentSession {
    /// Create and initialise an agent session.
    ///
    /// Opens (or creates) the SQLite database at `config.db_path`,
    /// registers builtin tools, and optionally loads skills from disk.
    pub async fn create(config: AgentConfig) -> ClawResult<AgentSession> {
        let memory = Arc::new(SqliteMemory::open(Path::new(&config.db_path)).await?);

        let mut registry = ToolRegistry::new();
        register_all_builtins(&mut registry);

        let ctx = ToolContext {
            memory:       memory.clone() as Arc<dyn crate::memory::traits::Memory>,
            sandbox_dir:  config.sandbox_dir.into(),
            http_allowlist: config.http_allowlist,
            permissions:  Arc::new(PermissionChecker::allow_all()),
        };

        let skills = if let Some(dir) = &config.skills_dir {
            load_skills_from_dir(Path::new(dir)).await.unwrap_or_default()
        } else {
            vec![]
        };

        let llm   = ClaudeClient::new(config.api_key, config.model);
        let inner = AgentLoop::new(llm, registry, ctx, SkillManager::new(skills));

        Ok(AgentSession { inner, memory })
    }

    /// Run one user turn and return all events.
    ///
    /// Dart converts the returned list to a `Stream<AgentEvent>` via
    /// `Stream.fromIterable()`.
    pub async fn chat(
        &mut self,
        input:  String,
        system: String,
    ) -> ClawResult<Vec<AgentEventDto>> {
        let events = self.inner.chat(&input, &system).await?;
        Ok(events.into_iter().map(AgentEventDto::from).collect())
    }

    /// Full conversation history for the current session.
    pub fn history(&self) -> Vec<MessageDto> {
        self.inner.history().iter().map(|m| MessageDto {
            role:    format!("{:?}", m.role).to_lowercase(),
            content: m.text_content(),
        }).collect()
    }

    /// Manifests of all currently loaded skills.
    pub fn skills(&self) -> Vec<SkillManifestDto> {
        self.inner.skills().iter().map(|s| SkillManifestDto {
            name:          s.manifest.name.clone(),
            description:   s.manifest.description.clone(),
            trust:         match s.manifest.trust {
                SkillTrust::Bundled   => "bundled".into(),
                SkillTrust::Installed => "installed".into(),
            },
            keywords:      s.manifest.activation.keywords.clone(),
            allowed_tools: s.manifest.allowed_tools.clone(),
        }).collect()
    }

    /// Load skill bundles from a directory.
    pub async fn load_skills_from_dir(&mut self, dir: String) -> ClawResult<()> {
        let skills = load_skills_from_dir(Path::new(&dir)).await?;
        // Replace the SkillManager in the inner loop.
        // Because skill_mgr is private, we expose a dedicated method on AgentLoop.
        self.inner.replace_skills(SkillManager::new(skills));
        Ok(())
    }

    // ── Memory methods ────────────────────────────────────────────────────────

    pub async fn memory_store(
        &self,
        path:     String,
        content:  String,
        category: String,
    ) -> ClawResult<MemoryDocDto> {
        let doc = self.memory.store(&path, &content, string_to_category(&category)).await?;
        Ok(doc_to_dto(doc))
    }

    pub async fn memory_recall(
        &self,
        query:    String,
        limit:    usize,
        category: Option<String>,
        since:    Option<u64>,
        until:    Option<u64>,
    ) -> ClawResult<Vec<SearchResultDto>> {
        let mut q = SearchQuery::new(query);
        q.limit    = limit;
        q.category = category.map(|s| string_to_category(&s));
        q.since    = since;
        q.until    = until;
        let results = self.memory.recall(&q).await?;
        Ok(results.into_iter().map(|r| SearchResultDto {
            doc:   doc_to_dto(r.doc),
            score: r.score,
        }).collect())
    }

    pub async fn memory_get(&self, path: String) -> ClawResult<Option<MemoryDocDto>> {
        Ok(self.memory.get(&path).await?.map(doc_to_dto))
    }

    pub async fn memory_forget(&self, path: String) -> ClawResult<bool> {
        self.memory.forget(&path).await
    }

    pub async fn memory_count(&self) -> ClawResult<usize> {
        self.memory.count().await
    }
}
```

- [ ] **Step 4.3 — Add `replace_skills()` to AgentLoop**

`load_skills_from_dir` above calls `self.inner.replace_skills(...)`. Add this method to `mobileclaw-core/src/agent/loop_impl.rs` inside `impl<L: LlmClient> AgentLoop<L>`:

```rust
/// Replace the skill manager (used by FFI layer after loading new skills).
pub fn replace_skills(&mut self, mgr: crate::skill::SkillManager) {
    self.skill_mgr = mgr;
}
```

- [ ] **Step 4.4 — Verify it compiles**

```bash
~/.cargo/bin/cargo build -p mobileclaw-core 2>&1 | tail -10
```

Expected: `Finished` — no errors.

- [ ] **Step 4.5 — Run full test suite**

```bash
~/.cargo/bin/cargo test -p mobileclaw-core --features test-utils 2>&1 | tail -5
```

Expected: all pass.

- [ ] **Step 4.6 — Commit**

```bash
git add mobileclaw-core/src/ffi.rs mobileclaw-core/src/lib.rs \
        mobileclaw-core/src/agent/loop_impl.rs
git commit -m "feat(ffi): add AgentSession + DTOs in ffi.rs for flutter_rust_bridge"
```

---

## Task 5: Configure frb codegen and generate Dart bindings

**Files:**
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/pubspec.yaml`
- Create: `mobileclaw-flutter/packages/mobileclaw_sdk/flutter_rust_bridge.yaml`
- Generated: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/bridge/`

- [ ] **Step 5.1 — Install codegen CLI** (skip if already installed)

```bash
~/.cargo/bin/cargo install flutter_rust_bridge_codegen 2>&1 | tail -5
```

Expected: installed or already up to date.

- [ ] **Step 5.2 — Add frb to mobileclaw_sdk pubspec**

In `mobileclaw-flutter/packages/mobileclaw_sdk/pubspec.yaml`, add to `dependencies`:

```yaml
  flutter_rust_bridge: ^2.9.0
  ffi: ^2.1.3
```

And to `dev_dependencies`:

```yaml
  ffigen: ^14.0.0
```

Run `flutter pub get`:

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk
/home/wjx/flutter/bin/flutter pub get 2>&1 | tail -5
```

- [ ] **Step 5.3 — Create `flutter_rust_bridge.yaml`**

Create `mobileclaw-flutter/packages/mobileclaw_sdk/flutter_rust_bridge.yaml`:

```yaml
rust_input: "../../mobileclaw-core/src/ffi.rs"
dart_output: "lib/src/bridge/"
rust_root: "../../mobileclaw-core"
dart_entrypoint_class_name: "MobileclawCoreBridge"
```

- [ ] **Step 5.4 — Run codegen**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk
~/.cargo/bin/flutter_rust_bridge_codegen generate 2>&1 | tail -20
```

Expected: generates files under `lib/src/bridge/`:
- `bridge_generated.dart` (Dart API)
- `bridge_generated.web.dart` (web stub)
- Possibly `frb_generated.rs` additions back in the Rust crate

If codegen fails due to Rust compilation errors, fix them before proceeding.

- [ ] **Step 5.5 — Verify generated files exist**

```bash
ls lib/src/bridge/
```

Expected: at least `bridge_generated.dart`.

- [ ] **Step 5.6 — Run SDK tests (Dart only)**

```bash
/home/wjx/flutter/bin/flutter test 2>&1 | tail -10
```

Expected: all 62 existing tests pass (they don't use the bridge yet).

- [ ] **Step 5.7 — Commit generated files**

```bash
git add mobileclaw-flutter/packages/mobileclaw_sdk/pubspec.yaml \
        mobileclaw-flutter/packages/mobileclaw_sdk/pubspec.lock \
        mobileclaw-flutter/packages/mobileclaw_sdk/flutter_rust_bridge.yaml \
        mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/bridge/
git commit -m "feat(sdk): add frb config and generated Dart bindings for AgentSession"
```

---

## Task 6: Build Linux native library

**Files:**
- Built: `mobileclaw-flutter/packages/mobileclaw_sdk/linux/libmobileclaw_core.so`
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/linux/CMakeLists.txt`

- [ ] **Step 6.1 — Build release `.so`**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw
~/.cargo/bin/cargo build --release -p mobileclaw-core 2>&1 | tail -10
```

Expected: `Finished release [optimized]`.

- [ ] **Step 6.2 — Copy to plugin linux/ directory**

```bash
cp target/release/libmobileclaw_core.so \
   .worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk/linux/
```

- [ ] **Step 6.3 — Update linux/CMakeLists.txt to bundle the .so**

In `mobileclaw-flutter/packages/mobileclaw_sdk/linux/CMakeLists.txt`, append:

```cmake
# Bundle the Rust native library
set(MOBILECLAW_SO "${CMAKE_CURRENT_SOURCE_DIR}/libmobileclaw_core.so")
install(FILES ${MOBILECLAW_SO} DESTINATION ${CMAKE_INSTALL_PREFIX}/lib)
target_link_libraries(${PLUGIN_NAME} PRIVATE ${MOBILECLAW_SO})
```

- [ ] **Step 6.4 — Verify .so exports the expected symbol**

```bash
nm -D .worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk/linux/libmobileclaw_core.so \
   | grep -i agent_session | head -10
```

Expected: lines containing `agent_session` (frb-generated extern "C" functions).

- [ ] **Step 6.5 — Commit**

```bash
cd .worktrees/flutter-dev
git add mobileclaw-flutter/packages/mobileclaw_sdk/linux/libmobileclaw_core.so \
        mobileclaw-flutter/packages/mobileclaw_sdk/linux/CMakeLists.txt
git commit -m "build(sdk): add Linux native library and link in CMakeLists"
```

---

## Task 7: Implement real `MobileclawAgentImpl` in Dart

**Files:**
- Create: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart`
- Modify: `mobileclaw-flutter/packages/mobileclaw_sdk/lib/mobileclaw_sdk.dart`

- [ ] **Step 7.1 — Write a failing test first**

Add to `mobileclaw-flutter/packages/mobileclaw_sdk/test/mobileclaw_sdk_test.dart`, in a new group at the bottom:

```dart
// ── These tests require the native library to be present ──────────────────
// Run only on Linux with: flutter test --dart-define=INTEGRATION=1
group('MobileclawAgentImpl (Linux integration)', () {
  const _run = bool.fromEnvironment('INTEGRATION');

  test('create() succeeds and memory starts empty', () async {
    if (!_run) return;

    final dir = Directory.systemTemp.createTempSync('claw_test_');
    try {
      final agent = await MobileclawAgentImpl.create(
        apiKey: 'test-key',          // won't call LLM in this test
        dbPath: '${dir.path}/m.db',
        sandboxDir: dir.path,
        httpAllowlist: [],
      );
      expect(await agent.memory.count(), 0);
      agent.dispose();
    } finally {
      dir.deleteSync(recursive: true);
    }
  }, timeout: const Timeout(Duration(seconds: 10)));

  test('memory store / recall round-trip via real SQLite', () async {
    if (!_run) return;

    final dir = Directory.systemTemp.createTempSync('claw_test2_');
    try {
      final agent = await MobileclawAgentImpl.create(
        apiKey: 'test-key',
        dbPath: '${dir.path}/m.db',
        sandboxDir: dir.path,
        httpAllowlist: [],
      );
      final doc = await agent.memory.store(
        'notes/test.md', 'hello world', MemoryCategory.core,
      );
      expect(doc.path, 'notes/test.md');
      final results = await agent.memory.recall('hello');
      expect(results, isNotEmpty);
      expect(results.first.score, greaterThan(0));
      agent.dispose();
    } finally {
      dir.deleteSync(recursive: true);
    }
  }, timeout: const Timeout(Duration(seconds: 10)));
}, skip: !bool.fromEnvironment('INTEGRATION') ? 'pass --dart-define=INTEGRATION=1' : null);
```

Also add `import 'dart:io';` at the top of the test file.

- [ ] **Step 7.2 — Create `agent_impl.dart`**

Create `mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart`:

```dart
import 'dart:async';

import 'engine.dart';
import 'events.dart';
import 'memory.dart';
import 'models.dart';
import 'exceptions.dart';
import 'bridge/bridge_generated.dart' as bridge;

/// Real [MobileclawAgent] backed by the Rust FFI bridge.
/// Phase 2 implementation — replaces [MockMobileclawAgent] for production.
class MobileclawAgentImpl implements MobileclawAgent {
  MobileclawAgentImpl._(this._session, this._memory);

  final bridge.AgentSession _session;
  final _RealMemory _memory;
  bool _disposed = false;

  static Future<MobileclawAgent> create({
    required String apiKey,
    required String dbPath,
    required String sandboxDir,
    required List<String> httpAllowlist,
    String model = 'claude-opus-4-6',
    String? skillsDir,
  }) async {
    final session = await bridge.AgentSession.create(
      config: bridge.AgentConfig(
        apiKey: apiKey,
        dbPath: dbPath,
        sandboxDir: sandboxDir,
        httpAllowlist: httpAllowlist,
        model: model,
        skillsDir: skillsDir,
      ),
    );
    return MobileclawAgentImpl._(session, _RealMemory(session));
  }

  @override
  void dispose() {
    _disposed = true;
    // Arc in Rust is dropped when the Dart handle is GC'd.
    // flutter_rust_bridge handles cleanup via the opaque handle finalizer.
  }

  @override
  Stream<AgentEvent> chat(String userInput, {String system = ''}) async* {
    _checkAlive();
    final dtos = await _session.chat(input: userInput, system: system);
    for (final dto in dtos) {
      yield switch (dto) {
        bridge.AgentEventDto_TextDelta(:final text) =>
          TextDeltaEvent(text: text),
        bridge.AgentEventDto_ToolCall(:final name) =>
          ToolCallEvent(toolName: name),
        bridge.AgentEventDto_ToolResult(:final name, :final success) =>
          ToolResultEvent(toolName: name, success: success),
        bridge.AgentEventDto_Done() =>
          const DoneEvent(),
      };
    }
  }

  @override
  Future<String> chatText(String userInput, {String system = ''}) async {
    final buf = StringBuffer();
    await for (final e in chat(userInput, system: system)) {
      if (e is TextDeltaEvent) buf.write(e.text);
    }
    return buf.toString();
  }

  @override
  List<ChatMessage> get history => _session.history()
      .map((m) => ChatMessage(role: m.role, content: m.content))
      .toList();

  @override
  MobileclawMemory get memory => _memory;

  @override
  Future<void> loadSkillsFromDir(String dirPath) =>
      _session.loadSkillsFromDir(dir: dirPath);

  @override
  List<SkillManifest> get skills => _session.skills()
      .map((s) => SkillManifest(
            name: s.name,
            description: s.description,
            trust: s.trust == 'bundled' ? SkillTrust.bundled : SkillTrust.installed,
            keywords: s.keywords,
            allowedTools: s.allowedTools,
          ))
      .toList();

  void _checkAlive() {
    if (_disposed) throw StateError('AgentSession has been disposed');
  }
}

/// Real [MobileclawMemory] backed by the Rust SQLite store.
class _RealMemory implements MobileclawMemory {
  _RealMemory(this._session);

  final bridge.AgentSession _session;

  @override
  Future<MemoryDoc> store(
      String path, String content, MemoryCategory category) async {
    final dto = await _session.memoryStore(
      path: path,
      content: content,
      category: category.toString(),
    );
    return _dtoToDoc(dto);
  }

  @override
  Future<List<SearchResult>> recall(
    String query, {
    int limit = 10,
    MemoryCategory? category,
    int? since,
    int? until,
  }) async {
    final dtos = await _session.memoryRecall(
      query: query,
      limit: limit,
      category: category?.toString(),
      since: since,
      until: until,
    );
    return dtos.map((r) => SearchResult(doc: _dtoToDoc(r.doc), score: r.score)).toList();
  }

  @override
  Future<MemoryDoc?> get(String path) async {
    final dto = await _session.memoryGet(path: path);
    return dto == null ? null : _dtoToDoc(dto);
  }

  @override
  Future<bool> forget(String path) => _session.memoryForget(path: path);

  @override
  Future<int> count() => _session.memoryCount();

  MemoryDoc _dtoToDoc(bridge.MemoryDocDto dto) {
    final cat = switch (dto.category) {
      'core'         => MemoryCategory.core,
      'daily'        => MemoryCategory.daily,
      'conversation' => MemoryCategory.conversation,
      final s when s.startsWith('custom:') =>
        MemoryCategory.custom(s.substring(7)),
      final s => MemoryCategory.custom(s),
    };
    return MemoryDoc(
      id: dto.id,
      path: dto.path,
      content: dto.content,
      category: cat,
      createdAt: dto.createdAt,
      updatedAt: dto.updatedAt,
    );
  }
}
```

> **Note:** The exact generated method/type names from `bridge_generated.dart` may differ slightly from the above. Adjust the `bridge.AgentEventDto_*` names to match what codegen produced. Check `lib/src/bridge/bridge_generated.dart` for the actual class names.

- [ ] **Step 7.3 — Export from mobileclaw_sdk.dart**

In `mobileclaw-flutter/packages/mobileclaw_sdk/lib/mobileclaw_sdk.dart`, add:

```dart
export 'src/agent_impl.dart';
```

- [ ] **Step 7.4 — Run existing unit tests — must still pass**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk
/home/wjx/flutter/bin/flutter test 2>&1 | tail -10
```

Expected: 62 tests pass (mock tests unaffected).

- [ ] **Step 7.5 — Run integration tests (requires native library)**

```bash
/home/wjx/flutter/bin/flutter test \
  --dart-define=INTEGRATION=1 \
  test/mobileclaw_sdk_test.dart 2>&1 | tail -20
```

Expected: 62 + 2 integration tests pass.

- [ ] **Step 7.6 — Commit**

```bash
git add mobileclaw-flutter/packages/mobileclaw_sdk/lib/src/agent_impl.dart \
        mobileclaw-flutter/packages/mobileclaw_sdk/lib/mobileclaw_sdk.dart \
        mobileclaw-flutter/packages/mobileclaw_sdk/test/mobileclaw_sdk_test.dart
git commit -m "feat(sdk): implement MobileclawAgentImpl backed by Rust FFI"
```

---

## Task 8: Update demo app to use real agent

**Files:**
- Modify: `mobileclaw-flutter/apps/mobileclaw_app/lib/core/engine_provider.dart`

- [ ] **Step 8.1 — Update engine_provider.dart**

Replace the body of `engine_provider.dart`:

```dart
import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:mobileclaw_sdk/mobileclaw_sdk.dart';
import 'package:path_provider/path_provider.dart';

/// `true` when the native library is present (Linux desktop / device builds).
bool get _nativeAvailable {
  if (Platform.isLinux) {
    // The .so is bundled alongside the Flutter app binary.
    return true;
  }
  // iOS / Android native support lands in Phase 3.
  return false;
}

/// Singleton [MobileclawAgent] for the app.
///
/// Uses [MobileclawAgentImpl] when the native library is available,
/// otherwise falls back to [MockMobileclawAgent] (Phase 1 / unsupported platforms).
final agentProvider = FutureProvider<MobileclawAgent>((ref) async {
  final dir = await getApplicationSupportDirectory();

  if (_nativeAvailable) {
    return MobileclawAgentImpl.create(
      apiKey: const String.fromEnvironment('ANTHROPIC_API_KEY', defaultValue: ''),
      dbPath: '${dir.path}/claw.db',
      sandboxDir: '${dir.path}/workspace',
      httpAllowlist: ['https://api.anthropic.com/'],
    );
  }

  // Fallback for development / unsupported platforms.
  return MockMobileclawAgent.create(
    apiKey: '',
    dbPath: '${dir.path}/claw.db',
    sandboxDir: '${dir.path}/workspace',
    httpAllowlist: [],
  );
});
```

- [ ] **Step 8.2 — Verify app analyzes cleanly**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw/.worktrees/flutter-dev/mobileclaw-flutter/apps/mobileclaw_app
/home/wjx/flutter/bin/flutter analyze 2>&1 | tail -10
```

Expected: `No issues found!` or only style warnings.

- [ ] **Step 8.3 — Commit**

```bash
git add mobileclaw-flutter/apps/mobileclaw_app/lib/core/engine_provider.dart
git commit -m "feat(app): use real MobileclawAgentImpl on Linux, mock fallback elsewhere"
```

---

## Task 9: Final verification

- [ ] **Step 9.1 — Full Rust test suite**

```bash
cd /home/wjx/agent_eyes/bot/mobileclaw
~/.cargo/bin/cargo test -p mobileclaw-core --features test-utils 2>&1 | tail -10
```

Expected: all pass, 0 failures.

- [ ] **Step 9.2 — Full SDK test suite (mock tests)**

```bash
cd .worktrees/flutter-dev/mobileclaw-flutter/packages/mobileclaw_sdk
/home/wjx/flutter/bin/flutter test 2>&1 | tail -5
```

Expected: ≥62 tests pass.

- [ ] **Step 9.3 — SDK integration tests (native bridge)**

```bash
/home/wjx/flutter/bin/flutter test \
  --dart-define=INTEGRATION=1 2>&1 | tail -10
```

Expected: all pass including 2 new integration tests.

- [ ] **Step 9.4 — Final commit**

```bash
git add -u
git commit -m "chore: Phase 2 complete — FFI binding wired, all tests passing"
```

---

## Troubleshooting

### codegen produces wrong method names

Check `lib/src/bridge/bridge_generated.dart` for the exact Dart method/class names and update `agent_impl.dart` accordingly. The frb codegen converts Rust `snake_case` to Dart `camelCase`.

### Missing `flutter_rust_bridge::frb` macro

Add to `mobileclaw-core/src/ffi.rs`:
```rust
use flutter_rust_bridge::frb;
```

### Linker error: `undefined symbol` in `.so`

Ensure `crate-type = ["cdylib", "lib"]` is in `mobileclaw-core/Cargo.toml`.

### `AgentSession::create` fails with SQLite error

The `db_path` parent directory must exist. The Dart layer must create it before calling `create()`.

### frb version mismatch between Dart and Rust

Both must use the same major version. Check `Cargo.lock` and `pubspec.lock` — the `flutter_rust_bridge` versions must match exactly.
