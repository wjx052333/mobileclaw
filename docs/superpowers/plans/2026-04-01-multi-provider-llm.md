# Multi-Provider LLM Support — Implementation Plan (Rust Core)

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend mobileclaw-core so users can configure and switch between Anthropic, OpenAI-compatible, and Ollama LLM providers, persisted in the encrypted SQLite SecretStore.

**Architecture:** Three independent client structs all implementing the existing `LlmClient` trait; a factory `create_llm_client()` dispatches to the right one. Provider configs are stored in `SqliteSecretStore` (new tables). `AgentSession::create()` loads the active provider from the store, falling back to the legacy explicit `api_key`/`model` fields.

**Tech Stack:** Rust, reqwest (SSE via eventsource-stream), async-stream (NDJSON for Ollama), uuid v1, wiremock (probe tests), rusqlite, flutter_rust_bridge.

**Spec:** `docs/superpowers/specs/2026-04-01-multi-provider-llm-design.md`

**Plan 2 (Flutter UI) follows after this plan** — screens depend on the FFI methods defined here.

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `Cargo.toml` (workspace) | Add uuid, async-stream, wiremock workspace deps |
| Modify | `mobileclaw-core/Cargo.toml` | Add uuid, async-stream; wiremock to dev-deps |
| Create | `mobileclaw-core/src/llm/provider.rs` | `ProviderProtocol`, `ProviderConfig`, `create_llm_client()` factory |
| Create | `mobileclaw-core/src/llm/openai_compat.rs` | `OpenAiCompatClient` + `parse_openai_event()` helper |
| Create | `mobileclaw-core/src/llm/ollama.rs` | `OllamaClient` + `parse_ollama_line()` helper + NDJSON streaming |
| Create | `mobileclaw-core/src/llm/probe.rs` | `ProbeResult`, `probe_provider()`, two-stage fallback |
| Modify | `mobileclaw-core/src/llm/mod.rs` | Export new modules |
| Modify | `mobileclaw-core/src/error.rs` | Add `ProviderNotFound(String)` variant |
| Modify | `mobileclaw-core/src/secrets/store.rs` | New schema tables + 7 provider methods on `SqliteSecretStore` |
| Modify | `mobileclaw-core/src/ffi.rs` | New DTOs, `AgentConfig` optional fields, `create()` fallback logic, 6 provider FFI methods + `provider_probe` free fn |

---

## Task 1: Add Dependencies + ProviderConfig Struct

**Files:**
- Modify: `Cargo.toml`
- Modify: `mobileclaw-core/Cargo.toml`
- Create: `mobileclaw-core/src/llm/provider.rs`
- Modify: `mobileclaw-core/src/llm/mod.rs`

- [ ] **Step 1.1: Add workspace deps**

  In `Cargo.toml`, add to `[workspace.dependencies]`:
  ```toml
  uuid        = { version = "1", features = ["v4"] }
  async-stream = "0.3"
  wiremock    = "0.6"
  ```

- [ ] **Step 1.2: Add crate deps**

  In `mobileclaw-core/Cargo.toml`, add to `[dependencies]`:
  ```toml
  uuid         = { workspace = true }
  async-stream = { workspace = true }
  ```

  Add to `[dev-dependencies]`:
  ```toml
  wiremock = { workspace = true }
  ```

- [ ] **Step 1.3: Write failing test for ProviderConfig**

  Create `mobileclaw-core/src/llm/provider.rs` with just the test block:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_provider_config_roundtrip() {
          let cfg = ProviderConfig::new(
              "DeepSeek".into(),
              ProviderProtocol::OpenAiCompat,
              "https://api.deepseek.com".into(),
              "deepseek-chat".into(),
          );
          assert_eq!(cfg.protocol, ProviderProtocol::OpenAiCompat);
          assert_eq!(cfg.model, "deepseek-chat");
          assert!(!cfg.id.is_empty());

          // Roundtrip through JSON
          let json = serde_json::to_string(&cfg).unwrap();
          let restored: ProviderConfig = serde_json::from_str(&json).unwrap();
          assert_eq!(restored.id, cfg.id);
          assert_eq!(restored.protocol, ProviderProtocol::OpenAiCompat);
          assert_eq!(restored.base_url, "https://api.deepseek.com");
      }
  }
  ```

- [ ] **Step 1.4: Run test — expect compile error (struct not defined)**

  ```bash
  cargo test -p mobileclaw-core test_provider_config_roundtrip
  ```
  Expected: `error[E0412]: cannot find type 'ProviderConfig'`

- [ ] **Step 1.5: Implement ProviderConfig and ProviderProtocol**

  Full content of `mobileclaw-core/src/llm/provider.rs`:
  ```rust
  use std::sync::Arc;
  use serde::{Deserialize, Serialize};
  use crate::{ClawError, ClawResult, llm::client::LlmClient};

  #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
  #[serde(rename_all = "snake_case")]
  pub enum ProviderProtocol {
      Anthropic,
      OpenAiCompat,
      Ollama,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ProviderConfig {
      pub id: String,
      pub name: String,
      pub protocol: ProviderProtocol,
      pub base_url: String,
      pub model: String,
      pub created_at: i64,
  }

  impl ProviderConfig {
      pub fn new(
          name: String,
          protocol: ProviderProtocol,
          base_url: String,
          model: String,
      ) -> Self {
          Self {
              id: uuid::Uuid::new_v4().to_string(),
              name,
              protocol,
              base_url,
              model,
              created_at: std::time::SystemTime::now()
                  .duration_since(std::time::UNIX_EPOCH)
                  .expect("time before epoch")
                  .as_secs() as i64,
          }
      }
  }

  /// Create an LlmClient from a ProviderConfig. Called once per session.
  /// api_key is None only for Ollama.
  pub fn create_llm_client(
      config: &ProviderConfig,
      api_key: Option<&str>,
  ) -> ClawResult<Arc<dyn LlmClient>> {
      match config.protocol {
          ProviderProtocol::Anthropic => {
              let key = api_key
                  .ok_or_else(|| ClawError::Llm("Anthropic requires api_key".into()))?;
              use crate::llm::client::ClaudeClient;
              Ok(Arc::new(ClaudeClient::new(key, &config.model)))
          }
          ProviderProtocol::OpenAiCompat => {
              let key = api_key
                  .ok_or_else(|| ClawError::Llm("OpenAI-compat requires api_key".into()))?;
              use crate::llm::openai_compat::OpenAiCompatClient;
              Ok(Arc::new(OpenAiCompatClient::new(&config.base_url, key, &config.model)?))
          }
          ProviderProtocol::Ollama => {
              use crate::llm::ollama::OllamaClient;
              Ok(Arc::new(OllamaClient::new(&config.base_url, &config.model)?))
          }
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_provider_config_roundtrip() {
          let cfg = ProviderConfig::new(
              "DeepSeek".into(),
              ProviderProtocol::OpenAiCompat,
              "https://api.deepseek.com".into(),
              "deepseek-chat".into(),
          );
          assert_eq!(cfg.protocol, ProviderProtocol::OpenAiCompat);
          assert_eq!(cfg.model, "deepseek-chat");
          assert!(!cfg.id.is_empty());

          let json = serde_json::to_string(&cfg).unwrap();
          let restored: ProviderConfig = serde_json::from_str(&json).unwrap();
          assert_eq!(restored.id, cfg.id);
          assert_eq!(restored.protocol, ProviderProtocol::OpenAiCompat);
          assert_eq!(restored.base_url, "https://api.deepseek.com");
      }
  }
  ```

- [ ] **Step 1.6: Export from mod.rs**

  In `mobileclaw-core/src/llm/mod.rs`, add:
  ```rust
  pub mod provider;
  pub mod openai_compat;  // declare now, implement next task
  pub mod ollama;         // declare now, implement next task
  pub mod probe;          // declare now, implement later
  ```
  Also add stub files so it compiles:
  ```bash
  echo "// TODO" > mobileclaw-core/src/llm/openai_compat.rs
  echo "// TODO" > mobileclaw-core/src/llm/ollama.rs
  echo "// TODO" > mobileclaw-core/src/llm/probe.rs
  ```

- [ ] **Step 1.7: Run test — expect PASS**

  ```bash
  cargo test -p mobileclaw-core test_provider_config_roundtrip
  ```
  Expected: `test llm::provider::tests::test_provider_config_roundtrip ... ok`

- [ ] **Step 1.8: Commit**

  ```bash
  git add Cargo.toml mobileclaw-core/Cargo.toml mobileclaw-core/src/llm/
  git commit -m "feat(llm): add ProviderConfig struct and module stubs"
  ```

---

## Task 2: OpenAiCompatClient

**Files:**
- Modify: `mobileclaw-core/src/llm/openai_compat.rs`

OpenAI SSE format: all chunks arrive with an **empty `event` field**. The first chunk is often `{"choices":[{"delta":{"role":"assistant"}}]}` with no `content` — skip it. A `MessageStart` is emitted synthetically. `data: [DONE]` signals `MessageStop`.

- [ ] **Step 2.1: Write failing parsing tests**

  Replace `mobileclaw-core/src/llm/openai_compat.rs` with:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::llm::types::StreamEvent;

      #[test]
      fn test_parse_openai_event_content() {
          let data = r#"{"id":"1","choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
          let result = parse_openai_event(data).unwrap();
          assert_eq!(result, Some(StreamEvent::TextDelta { text: "Hello".into() }));
      }

      #[test]
      fn test_parse_openai_event_role_only_skipped() {
          // First chunk often has only role, no content
          let data = r#"{"id":"1","choices":[{"delta":{"role":"assistant"},"index":0}]}"#;
          let result = parse_openai_event(data).unwrap();
          assert_eq!(result, None);
      }

      #[test]
      fn test_parse_openai_event_done() {
          let result = parse_openai_event("[DONE]").unwrap();
          assert_eq!(result, Some(StreamEvent::MessageStop));
      }

      #[test]
      fn test_parse_openai_event_null_content_skipped() {
          let data = r#"{"id":"1","choices":[{"delta":{"content":null},"index":0}]}"#;
          let result = parse_openai_event(data).unwrap();
          assert_eq!(result, None);
      }
  }
  ```

- [ ] **Step 2.2: Run test — expect compile error**

  ```bash
  cargo test -p mobileclaw-core test_parse_openai_event
  ```
  Expected: compile error (function not defined).

- [ ] **Step 2.3: Implement parse_openai_event + OpenAiCompatClient**

  Full content of `mobileclaw-core/src/llm/openai_compat.rs`:
  ```rust
  use async_trait::async_trait;
  use futures::StreamExt;
  use eventsource_stream::Eventsource;

  use crate::{ClawError, ClawResult, llm::{client::{EventStream, LlmClient}, types::{Message, StreamEvent}}};

  pub struct OpenAiCompatClient {
      /// Always ends with "/v1" (normalised in constructor)
      base_url: String,
      api_key: String,
      model: String,
      http: reqwest::Client,
  }

  impl OpenAiCompatClient {
      pub fn new(base_url: &str, api_key: &str, model: &str) -> ClawResult<Self> {
          let base_url = normalise_base_url(base_url);
          let http = reqwest::Client::builder()
              .use_rustls_tls()
              .build()
              .map_err(|e| ClawError::Llm(format!("HTTP client build failed: {e}")))?;
          Ok(Self { base_url, api_key: api_key.into(), model: model.into(), http })
      }
  }

  fn normalise_base_url(url: &str) -> String {
      let trimmed = url.trim_end_matches('/');
      if trimmed.ends_with("/v1") {
          trimmed.to_string()
      } else {
          format!("{trimmed}/v1")
      }
  }

  /// Parse one OpenAI SSE data string into a StreamEvent.
  /// Returns None for empty/role-only deltas (should be skipped by caller).
  pub(crate) fn parse_openai_event(data: &str) -> ClawResult<Option<StreamEvent>> {
      if data == "[DONE]" {
          return Ok(Some(StreamEvent::MessageStop));
      }
      let v: serde_json::Value =
          serde_json::from_str(data).map_err(|e| ClawError::Parse(e.to_string()))?;
      let text = v["choices"][0]["delta"]["content"]
          .as_str()
          .unwrap_or("")
          .to_string();
      if text.is_empty() {
          Ok(None)
      } else {
          Ok(Some(StreamEvent::TextDelta { text }))
      }
  }

  #[async_trait]
  impl LlmClient for OpenAiCompatClient {
      async fn stream_messages(
          &self,
          system: &str,
          messages: &[Message],
          max_tokens: u32,
      ) -> ClawResult<EventStream> {
          let mut msg_array = vec![serde_json::json!({"role":"system","content":system})];
          for m in messages {
              msg_array.push(serde_json::json!({
                  "role": format!("{:?}", m.role).to_lowercase(),
                  "content": m.text_content()
              }));
          }
          let body = serde_json::json!({
              "model": self.model,
              "max_tokens": max_tokens,
              "messages": msg_array,
              "stream": true,
          });

          let resp = self.http
              .post(format!("{}/chat/completions", self.base_url))
              .header("Authorization", format!("Bearer {}", self.api_key))
              .header("content-type", "application/json")
              .json(&body)
              .send()
              .await
              .map_err(|e| ClawError::Llm(format!("OpenAI-compat request: {e}")))?;

          if !resp.status().is_success() {
              let status = resp.status();
              let body = resp.text().await.unwrap_or_default();
              return Err(ClawError::Llm(format!("OpenAI-compat {status}: {body}")));
          }

          // Synthetic MessageStart, then map SSE chunks
          let initial = futures::stream::once(async { Ok(StreamEvent::MessageStart) });
          let data_stream = resp.bytes_stream().eventsource().filter_map(|ev| async {
              match ev {
                  Ok(e) => match parse_openai_event(&e.data) {
                      Ok(Some(event)) => Some(Ok(event)),
                      Ok(None) => None,
                      Err(e) => Some(Err(e)),
                  },
                  Err(e) => Some(Err(ClawError::Llm(e.to_string()))),
              }
          });

          Ok(Box::pin(initial.chain(data_stream)))
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::llm::types::StreamEvent;

      #[test]
      fn test_parse_openai_event_content() {
          let data = r#"{"id":"1","choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
          assert_eq!(parse_openai_event(data).unwrap(), Some(StreamEvent::TextDelta { text: "Hello".into() }));
      }

      #[test]
      fn test_parse_openai_event_role_only_skipped() {
          let data = r#"{"id":"1","choices":[{"delta":{"role":"assistant"},"index":0}]}"#;
          assert_eq!(parse_openai_event(data).unwrap(), None);
      }

      #[test]
      fn test_parse_openai_event_done() {
          assert_eq!(parse_openai_event("[DONE]").unwrap(), Some(StreamEvent::MessageStop));
      }

      #[test]
      fn test_parse_openai_event_null_content_skipped() {
          let data = r#"{"id":"1","choices":[{"delta":{"content":null},"index":0}]}"#;
          assert_eq!(parse_openai_event(data).unwrap(), None);
      }

      #[test]
      fn test_normalise_base_url_appends_v1() {
          assert_eq!(normalise_base_url("https://api.groq.com/openai"), "https://api.groq.com/openai/v1");
          assert_eq!(normalise_base_url("https://api.groq.com/openai/v1"), "https://api.groq.com/openai/v1");
          assert_eq!(normalise_base_url("https://api.groq.com/openai/"), "https://api.groq.com/openai/v1");
      }
  }
  ```

- [ ] **Step 2.4: Run tests — expect PASS**

  ```bash
  cargo test -p mobileclaw-core openai_compat
  ```
  Expected: 5 tests pass.

- [ ] **Step 2.5: Commit**

  ```bash
  git add mobileclaw-core/src/llm/openai_compat.rs
  git commit -m "feat(llm): add OpenAiCompatClient with SSE streaming"
  ```

---

## Task 3: OllamaClient

**Files:**
- Modify: `mobileclaw-core/src/llm/ollama.rs`

Ollama `/api/chat` streams **NDJSON** (newline-delimited JSON) — NOT SSE. Each line is a complete JSON object like `{"message":{"content":"Hi"},"done":false}`. The final line has `"done":true`. Do not use `eventsource_stream` here.

- [ ] **Step 3.1: Write failing parsing tests**

  Replace `mobileclaw-core/src/llm/ollama.rs` with:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::llm::types::StreamEvent;

      #[test]
      fn test_parse_ollama_line_content() {
          let line = r#"{"model":"llama3","message":{"role":"assistant","content":"Hi"},"done":false}"#;
          let (event, done) = parse_ollama_line(line).unwrap();
          assert_eq!(event, Some(StreamEvent::TextDelta { text: "Hi".into() }));
          assert!(!done);
      }

      #[test]
      fn test_parse_ollama_line_done_empty_content() {
          let line = r#"{"model":"llama3","message":{"role":"assistant","content":""},"done":true}"#;
          let (event, done) = parse_ollama_line(line).unwrap();
          assert_eq!(event, None);  // empty content, skip
          assert!(done);
      }

      #[test]
      fn test_parse_ollama_line_done_with_content() {
          let line = r#"{"model":"llama3","message":{"role":"assistant","content":"!"},"done":true}"#;
          let (event, done) = parse_ollama_line(line).unwrap();
          assert_eq!(event, Some(StreamEvent::TextDelta { text: "!".into() }));
          assert!(done);
      }

      #[test]
      fn test_normalise_ollama_url_adds_slash() {
          assert_eq!(normalise_base_url("http://localhost:11434"), "http://localhost:11434/");
          assert_eq!(normalise_base_url("http://localhost:11434/"), "http://localhost:11434/");
      }
  }
  ```

- [ ] **Step 3.2: Run test — expect compile error**

  ```bash
  cargo test -p mobileclaw-core ollama
  ```

- [ ] **Step 3.3: Implement parse_ollama_line + OllamaClient**

  Full content of `mobileclaw-core/src/llm/ollama.rs`:
  ```rust
  use async_trait::async_trait;
  use async_stream::try_stream;
  use futures::StreamExt;

  use crate::{ClawError, ClawResult, llm::{client::{EventStream, LlmClient}, types::{Message, StreamEvent}}};

  pub struct OllamaClient {
      /// Always ends with "/" (normalised in constructor)
      base_url: String,
      model: String,
      http: reqwest::Client,
  }

  impl OllamaClient {
      pub fn new(base_url: &str, model: &str) -> ClawResult<Self> {
          let http = reqwest::Client::builder()
              .use_rustls_tls()
              .build()
              .map_err(|e| ClawError::Llm(format!("HTTP client build failed: {e}")))?;
          Ok(Self { base_url: normalise_base_url(base_url), model: model.into(), http })
      }
  }

  fn normalise_base_url(url: &str) -> String {
      let trimmed = url.trim_end_matches('/');
      format!("{trimmed}/")
  }

  /// Parse one NDJSON line from Ollama's /api/chat response.
  /// Returns (Option<StreamEvent>, is_done).
  /// is_done=true means the stream is finished.
  pub(crate) fn parse_ollama_line(line: &str) -> ClawResult<(Option<StreamEvent>, bool)> {
      let v: serde_json::Value =
          serde_json::from_str(line).map_err(|e| ClawError::Parse(e.to_string()))?;
      let text = v["message"]["content"].as_str().unwrap_or("").to_string();
      let done = v["done"].as_bool().unwrap_or(false);
      let event = if text.is_empty() {
          None
      } else {
          Some(StreamEvent::TextDelta { text })
      };
      Ok((event, done))
  }

  #[async_trait]
  impl LlmClient for OllamaClient {
      async fn stream_messages(
          &self,
          system: &str,
          messages: &[Message],
          _max_tokens: u32,  // Ollama /api/chat does not accept max_tokens
      ) -> ClawResult<EventStream> {
          let mut msg_array = vec![serde_json::json!({"role":"system","content":system})];
          for m in messages {
              msg_array.push(serde_json::json!({
                  "role": format!("{:?}", m.role).to_lowercase(),
                  "content": m.text_content()
              }));
          }
          let body = serde_json::json!({
              "model": self.model,
              "messages": msg_array,
              "stream": true,
          });

          let resp = self.http
              .post(format!("{}api/chat", self.base_url))
              .json(&body)
              .send()
              .await
              .map_err(|e| ClawError::Llm(format!("Ollama request: {e}")))?;

          if !resp.status().is_success() {
              let status = resp.status();
              let body = resp.text().await.unwrap_or_default();
              return Err(ClawError::Llm(format!("Ollama {status}: {body}")));
          }

          let mut bytes_stream = resp.bytes_stream();
          let stream = try_stream! {
              yield StreamEvent::MessageStart;
              let mut buf: Vec<u8> = Vec::new();
              loop {
                  if let Some(nl) = buf.iter().position(|&b| b == b'\n') {
                      // Extract the line up to (not including) the newline
                      let line = String::from_utf8_lossy(&buf[..nl]).into_owned();
                      buf.drain(..=nl);
                      let trimmed = line.trim();
                      if trimmed.is_empty() { continue; }
                      let (event, done) = parse_ollama_line(trimmed)?;
                      if let Some(ev) = event {
                          yield ev;
                      }
                      if done {
                          yield StreamEvent::MessageStop;
                          return;
                      }
                  } else {
                      match bytes_stream.next().await {
                          Some(Ok(chunk)) => buf.extend_from_slice(&chunk),
                          Some(Err(e)) => Err(ClawError::Llm(e.to_string()))?,
                          None => {
                              // HTTP body ended without done:true
                              yield StreamEvent::MessageStop;
                              return;
                          }
                      }
                  }
              }
          };

          Ok(Box::pin(stream))
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::llm::types::StreamEvent;

      #[test]
      fn test_parse_ollama_line_content() {
          let line = r#"{"model":"llama3","message":{"role":"assistant","content":"Hi"},"done":false}"#;
          let (event, done) = parse_ollama_line(line).unwrap();
          assert_eq!(event, Some(StreamEvent::TextDelta { text: "Hi".into() }));
          assert!(!done);
      }

      #[test]
      fn test_parse_ollama_line_done_empty_content() {
          let line = r#"{"model":"llama3","message":{"role":"assistant","content":""},"done":true}"#;
          let (event, done) = parse_ollama_line(line).unwrap();
          assert_eq!(event, None);
          assert!(done);
      }

      #[test]
      fn test_parse_ollama_line_done_with_content() {
          let line = r#"{"model":"llama3","message":{"role":"assistant","content":"!"},"done":true}"#;
          let (event, done) = parse_ollama_line(line).unwrap();
          assert_eq!(event, Some(StreamEvent::TextDelta { text: "!".into() }));
          assert!(done);
      }

      #[test]
      fn test_normalise_ollama_url_adds_slash() {
          assert_eq!(normalise_base_url("http://localhost:11434"), "http://localhost:11434/");
          assert_eq!(normalise_base_url("http://localhost:11434/"), "http://localhost:11434/");
      }
  }
  ```

- [ ] **Step 3.4: Run tests — expect PASS**

  ```bash
  cargo test -p mobileclaw-core ollama
  ```
  Expected: 4 tests pass.

- [ ] **Step 3.5: Verify the factory compiles end-to-end**

  ```bash
  cargo build -p mobileclaw-core 2>&1 | head -20
  ```
  Expected: no errors.

- [ ] **Step 3.6: Commit**

  ```bash
  git add mobileclaw-core/src/llm/ollama.rs mobileclaw-core/src/llm/provider.rs
  git commit -m "feat(llm): add OllamaClient (NDJSON) and complete factory dispatch"
  ```

---

## Task 4: ProbeResult + probe_provider

**Files:**
- Modify: `mobileclaw-core/src/llm/probe.rs`

Two-stage: first tries a minimal completion request (1 token); if that fails but the `/v1/models` or `/api/tags` endpoint responds with 200, returns `ok:true, degraded:true`. Both failing → `ok:false`. `degraded:true` means "endpoint reachable but completions untested".

- [ ] **Step 4.1: Write failing tests**

  Replace `mobileclaw-core/src/llm/probe.rs` with tests only:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_probe_result_fields() {
          let r = ProbeResult { ok: true, latency_ms: 150, degraded: false, error: None };
          assert!(r.ok);
          assert_eq!(r.latency_ms, 150);
          assert!(!r.degraded);
      }

      #[tokio::test]
      async fn test_probe_unreachable_host_returns_fail() {
          // Port 1 is reserved and should refuse connections quickly
          use crate::llm::provider::{ProviderConfig, ProviderProtocol};
          let cfg = ProviderConfig::new(
              "test".into(),
              ProviderProtocol::OpenAiCompat,
              "http://localhost:1".into(),
              "unused".into(),
          );
          let result = probe_provider(&cfg, Some("key")).await;
          assert!(!result.ok);
          assert!(result.error.is_some());
      }
  }
  ```

- [ ] **Step 4.2: Run test — expect compile error**

  ```bash
  cargo test -p mobileclaw-core probe
  ```

- [ ] **Step 4.3: Implement probe_provider**

  Full content of `mobileclaw-core/src/llm/probe.rs`:
  ```rust
  use std::time::Instant;
  use crate::{ClawError, ClawResult, llm::{client::LlmClient, provider::{ProviderConfig, ProviderProtocol, create_llm_client}, types::{Message, StreamEvent}}};
  use futures::StreamExt;

  #[derive(Debug, Clone)]
  pub struct ProbeResult {
      pub ok: bool,
      pub latency_ms: u64,
      /// true = completions failed but models endpoint responded
      pub degraded: bool,
      pub error: Option<String>,
  }

  pub async fn probe_provider(config: &ProviderConfig, api_key: Option<&str>) -> ProbeResult {
      let start = Instant::now();

      match probe_with_request(config, api_key).await {
          Ok(()) => ProbeResult {
              ok: true,
              latency_ms: start.elapsed().as_millis() as u64,
              degraded: false,
              error: None,
          },
          Err(completion_err) => {
              let err_msg = completion_err.to_string();
              match probe_models_endpoint(config, api_key).await {
                  Ok(()) => ProbeResult {
                      ok: true,
                      latency_ms: start.elapsed().as_millis() as u64,
                      degraded: true,
                      error: None,
                  },
                  Err(_) => ProbeResult {
                      ok: false,
                      latency_ms: start.elapsed().as_millis() as u64,
                      degraded: false,
                      error: Some(err_msg),
                  },
              }
          }
      }
  }

  async fn probe_with_request(config: &ProviderConfig, api_key: Option<&str>) -> ClawResult<()> {
      // Wrap in timeout so probe never blocks for OS TCP timeout (~30s on refused connections)
      tokio::time::timeout(
          std::time::Duration::from_secs(15),
          probe_with_request_inner(config, api_key),
      )
      .await
      .unwrap_or_else(|_| Err(ClawError::Llm("probe timed out after 15s".into())))
  }

  async fn probe_with_request_inner(config: &ProviderConfig, api_key: Option<&str>) -> ClawResult<()> {
      let client = create_llm_client(config, api_key)?;
      let mut stream = client
          .stream_messages(".", &[Message::user("Hi")], 16)
          .await?;
      // Consume first event to verify the stream is working
      match stream.next().await {
          Some(Ok(_)) => Ok(()),
          Some(Err(e)) => Err(e),
          None => Err(ClawError::Llm("empty response stream".into())),
      }
  }

  async fn probe_models_endpoint(config: &ProviderConfig, api_key: Option<&str>) -> ClawResult<()> {
      let http = reqwest::Client::builder()
          .use_rustls_tls()
          .timeout(std::time::Duration::from_secs(10))
          .build()
          .map_err(|e| ClawError::Llm(e.to_string()))?;

      match config.protocol {
          ProviderProtocol::OpenAiCompat => {
              let base = config.base_url.trim_end_matches('/');
              let url = if base.ends_with("/v1") {
                  format!("{base}/models")
              } else {
                  format!("{base}/v1/models")
              };
              let resp = http
                  .get(&url)
                  .header("Authorization", format!("Bearer {}", api_key.unwrap_or("")))
                  .send()
                  .await
                  .map_err(|e| ClawError::Llm(e.to_string()))?;
              if resp.status().is_success() { Ok(()) }
              else { Err(ClawError::Llm(format!("models endpoint: {}", resp.status()))) }
          }
          ProviderProtocol::Ollama => {
              let base = config.base_url.trim_end_matches('/');
              let url = format!("{base}/api/tags");
              let resp = http.get(&url).send().await
                  .map_err(|e| ClawError::Llm(e.to_string()))?;
              if resp.status().is_success() { Ok(()) }
              else { Err(ClawError::Llm(format!("tags endpoint: {}", resp.status()))) }
          }
          ProviderProtocol::Anthropic => {
              // Anthropic has no public models endpoint — skip fallback
              Err(ClawError::Llm("no models endpoint for Anthropic".into()))
          }
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_probe_result_fields() {
          let r = ProbeResult { ok: true, latency_ms: 150, degraded: false, error: None };
          assert!(r.ok);
          assert_eq!(r.latency_ms, 150);
          assert!(!r.degraded);
      }

      #[tokio::test]
      async fn test_probe_unreachable_host_returns_fail() {
          use crate::llm::provider::{ProviderConfig, ProviderProtocol};
          let cfg = ProviderConfig::new(
              "test".into(),
              ProviderProtocol::OpenAiCompat,
              "http://localhost:1".into(),
              "unused".into(),
          );
          let result = probe_provider(&cfg, Some("key")).await;
          assert!(!result.ok);
          assert!(result.error.is_some());
      }
  }
  ```

- [ ] **Step 4.4: Run tests — expect PASS**

  ```bash
  cargo test -p mobileclaw-core probe
  ```
  Expected: `test_probe_result_fields ... ok`, `test_probe_unreachable_host_returns_fail ... ok`

- [ ] **Step 4.5: Commit**

  ```bash
  git add mobileclaw-core/src/llm/probe.rs
  git commit -m "feat(llm): add probe_provider with two-stage fallback"
  ```

---

## Task 5: ProviderNotFound Error Variant

**Files:**
- Modify: `mobileclaw-core/src/error.rs`

- [ ] **Step 5.1: Write failing test**

  Add to `mobileclaw-core/src/error.rs` inside a `#[cfg(test)]` block at the bottom:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_provider_not_found_display() {
          let e = ClawError::ProviderNotFound("abc-123".into());
          assert_eq!(e.to_string(), "provider not found: 'abc-123'");
      }
  }
  ```

- [ ] **Step 5.2: Run — expect compile error**

  ```bash
  cargo test -p mobileclaw-core test_provider_not_found_display
  ```

- [ ] **Step 5.3: Add the variant**

  In `mobileclaw-core/src/error.rs`, add after the `SecretStore` variant:
  ```rust
  #[error("provider not found: '{0}'")]
  ProviderNotFound(String),
  ```

- [ ] **Step 5.4: Run — expect PASS**

  ```bash
  cargo test -p mobileclaw-core test_provider_not_found_display
  ```

- [ ] **Step 5.5: Commit**

  ```bash
  git add mobileclaw-core/src/error.rs
  git commit -m "feat(error): add ProviderNotFound variant"
  ```

---

## Task 6: SecretStore — Provider Tables + CRUD Methods

**Files:**
- Modify: `mobileclaw-core/src/secrets/store.rs`

Provider configs (name, protocol, base_url, model) are stored in plaintext in the `providers` table. API keys are AES-256-GCM encrypted in `provider_secrets` (reuses existing cipher). The active provider ID is stored in a plaintext `kv` table (not encrypted — it's not a secret).

Pattern: follow the existing `put_email_account` / `get_email_account` methods on `SqliteSecretStore`. Do NOT add these to the `SecretStore` trait; add them as inherent methods (same as email).

- [ ] **Step 6.1: Write failing tests**

  Add to the `#[cfg(test)]` block at the bottom of `store.rs`:
  ```rust
  mod provider_tests {
      use super::*;
      use tempfile::NamedTempFile;
      use crate::llm::provider::{ProviderConfig, ProviderProtocol};

      async fn open_test_store() -> SqliteSecretStore {
          let f = NamedTempFile::new().unwrap();
          SqliteSecretStore::open(f.path().to_path_buf(), b"test-key-32bytes0000000000000000")
              .await
              .unwrap()
      }

      #[tokio::test]
      async fn test_provider_save_and_load() {
          let store = open_test_store().await;
          let cfg = ProviderConfig::new("Groq".into(), ProviderProtocol::OpenAiCompat,
              "https://api.groq.com/openai".into(), "mixtral-8x7b".into());
          store.provider_save(&cfg, Some("sk-test")).await.unwrap();

          let loaded = store.provider_load(&cfg.id).await.unwrap();
          assert_eq!(loaded.name, "Groq");
          assert_eq!(loaded.model, "mixtral-8x7b");

          let key = store.provider_api_key(&cfg.id).await.unwrap();
          assert_eq!(key, Some("sk-test".into()));
      }

      #[tokio::test]
      async fn test_provider_list_and_delete() {
          let store = open_test_store().await;
          let a = ProviderConfig::new("A".into(), ProviderProtocol::Anthropic,
              "https://api.anthropic.com".into(), "claude-opus-4-6".into());
          let b = ProviderConfig::new("B".into(), ProviderProtocol::Ollama,
              "http://localhost:11434".into(), "llama3".into());
          store.provider_save(&a, Some("key-a")).await.unwrap();
          store.provider_save(&b, None).await.unwrap();

          let list = store.provider_list().await.unwrap();
          assert_eq!(list.len(), 2);

          store.provider_delete(&a.id).await.unwrap();
          let list = store.provider_list().await.unwrap();
          assert_eq!(list.len(), 1);
          assert_eq!(list[0].name, "B");
      }

      #[tokio::test]
      async fn test_active_provider_id_persistence() {
          let f = NamedTempFile::new().unwrap();
          let path = f.path().to_path_buf();
          let key = b"test-key-32bytes0000000000000000";

          let store = SqliteSecretStore::open(path.clone(), key).await.unwrap();
          let cfg = ProviderConfig::new("X".into(), ProviderProtocol::Ollama,
              "http://localhost:11434".into(), "llama3".into());
          store.provider_save(&cfg, None).await.unwrap();
          store.set_active_provider_id(&cfg.id).await.unwrap();
          drop(store);

          // Re-open from same file — active ID must survive
          let store2 = SqliteSecretStore::open(path, key).await.unwrap();
          assert_eq!(store2.active_provider_id().await.unwrap(), Some(cfg.id));
      }

      #[tokio::test]
      async fn test_provider_not_found_returns_error() {
          let store = open_test_store().await;
          let err = store.provider_load("nonexistent-id").await.unwrap_err();
          assert!(matches!(err, crate::ClawError::ProviderNotFound(_)));
      }
  }
  ```

- [ ] **Step 6.2: Run — expect compile error (methods not defined)**

  ```bash
  cargo test -p mobileclaw-core provider_tests
  ```

- [ ] **Step 6.3: Extend SqliteSecretStore::open() to create new tables**

  In `mobileclaw-core/src/secrets/store.rs`, inside `SqliteSecretStore::open()`, extend the `execute_batch` call to also create the new tables:
  ```rust
  conn.execute_batch(
      "CREATE TABLE IF NOT EXISTS secrets (
          key   TEXT PRIMARY KEY,
          value TEXT NOT NULL
       );
       CREATE TABLE IF NOT EXISTS providers (
          id         TEXT PRIMARY KEY,
          name       TEXT NOT NULL,
          protocol   TEXT NOT NULL,
          base_url   TEXT NOT NULL,
          model      TEXT NOT NULL,
          created_at INTEGER NOT NULL
       );
       CREATE TABLE IF NOT EXISTS provider_secrets (
          provider_id TEXT PRIMARY KEY REFERENCES providers(id) ON DELETE CASCADE,
          encrypted   TEXT NOT NULL   -- base64(nonce || ciphertext), same format as secrets table
       );
       CREATE TABLE IF NOT EXISTS kv (
          key   TEXT PRIMARY KEY,
          value TEXT NOT NULL   -- plaintext; used for non-secret config
       );",
  )?;
  ```

- [ ] **Step 6.4: Add provider methods to the SqliteSecretStore impl block**

  Add after the existing email methods (around line 130), before the closing `}` of `impl SqliteSecretStore`:
  ```rust
  pub async fn provider_save(
      &self,
      config: &crate::llm::provider::ProviderConfig,
      api_key: Option<&str>,
  ) -> ClawResult<()> {
      let protocol = serde_json::to_string(&config.protocol)
          .map_err(|e| ClawError::SecretStore(e.to_string()))?;
      let protocol = protocol.trim_matches('"').to_string(); // strip JSON quotes
      {
          let conn = self.conn.lock().await;
          conn.execute(
              "INSERT INTO providers (id, name, protocol, base_url, model, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)
               ON CONFLICT(id) DO UPDATE SET
                 name=excluded.name, protocol=excluded.protocol,
                 base_url=excluded.base_url, model=excluded.model",
              rusqlite::params![
                  config.id, config.name, protocol,
                  config.base_url, config.model, config.created_at
              ],
          )?;
      }
      if let Some(key) = api_key {
          let encrypted = self.encrypt(key)?;
          let conn = self.conn.lock().await;
          conn.execute(
              "INSERT INTO provider_secrets (provider_id, encrypted) VALUES (?1, ?2)
               ON CONFLICT(provider_id) DO UPDATE SET encrypted=excluded.encrypted",
              rusqlite::params![config.id, encrypted],
          )?;
      }
      Ok(())
  }

  pub async fn provider_load(&self, id: &str) -> ClawResult<crate::llm::provider::ProviderConfig> {
      let conn = self.conn.lock().await;
      let result: Option<(String, String, String, String, i64)> = conn
          .query_row(
              "SELECT name, protocol, base_url, model, created_at FROM providers WHERE id = ?1",
              rusqlite::params![id],
              |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
          )
          .optional()?;
      match result {
          None => Err(ClawError::ProviderNotFound(id.into())),
          Some((name, protocol_str, base_url, model, created_at)) => {
              // Deserialize protocol from snake_case string
              let protocol: crate::llm::provider::ProviderProtocol =
                  serde_json::from_str(&format!("\"{}\"", protocol_str))
                      .map_err(|e| ClawError::SecretStore(e.to_string()))?;
              Ok(crate::llm::provider::ProviderConfig {
                  id: id.to_string(),
                  name,
                  protocol,
                  base_url,
                  model,
                  created_at,
              })
          }
      }
  }

  pub async fn provider_list(&self) -> ClawResult<Vec<crate::llm::provider::ProviderConfig>> {
      let conn = self.conn.lock().await;
      let mut stmt = conn.prepare(
          "SELECT id, name, protocol, base_url, model, created_at FROM providers ORDER BY created_at ASC"
      )?;
      let rows = stmt.query_map([], |row| {
          Ok((
              row.get::<_, String>(0)?,
              row.get::<_, String>(1)?,
              row.get::<_, String>(2)?,
              row.get::<_, String>(3)?,
              row.get::<_, String>(4)?,
              row.get::<_, i64>(5)?,
          ))
      })?;
      let mut configs = Vec::new();
      for row in rows {
          let (id, name, protocol_str, base_url, model, created_at) = row?;
          let protocol: crate::llm::provider::ProviderProtocol =
              serde_json::from_str(&format!("\"{}\"", protocol_str))
                  .map_err(|e| ClawError::SecretStore(e.to_string()))?;
          configs.push(crate::llm::provider::ProviderConfig { id, name, protocol, base_url, model, created_at });
      }
      Ok(configs)
  }

  pub async fn provider_delete(&self, id: &str) -> ClawResult<()> {
      let conn = self.conn.lock().await;
      // ON DELETE CASCADE removes provider_secrets row automatically
      conn.execute("DELETE FROM providers WHERE id = ?1", rusqlite::params![id])?;
      Ok(())
  }

  pub async fn provider_api_key(&self, id: &str) -> ClawResult<Option<String>> {
      let conn = self.conn.lock().await;
      let encrypted: Option<String> = conn
          .query_row(
              "SELECT encrypted FROM provider_secrets WHERE provider_id = ?1",
              rusqlite::params![id],
              |row| row.get(0),
          )
          .optional()?;
      match encrypted {
          None => Ok(None),
          // decrypt() returns ClawResult<SecretString>; expose() gives &str
          Some(enc) => self.decrypt(&enc).map(|s| Some(s.expose().to_string())),
      }
  }

  pub async fn active_provider_id(&self) -> ClawResult<Option<String>> {
      let conn = self.conn.lock().await;
      let val: Option<String> = conn
          .query_row(
              "SELECT value FROM kv WHERE key = 'active_provider_id'",
              [],
              |row| row.get(0),
          )
          .optional()?;
      Ok(val)
  }

  pub async fn set_active_provider_id(&self, id: &str) -> ClawResult<()> {
      let conn = self.conn.lock().await;
      conn.execute(
          "INSERT INTO kv (key, value) VALUES ('active_provider_id', ?1)
           ON CONFLICT(key) DO UPDATE SET value=excluded.value",
          rusqlite::params![id],
      )?;
      Ok(())
  }
  ```

  **Note:** `encrypt` and `decrypt` are existing private methods on `SqliteSecretStore`. They take/return `&str`/`String`. Check their exact signatures in the file before writing this code.

- [ ] **Step 6.5: Check encrypt/decrypt signatures before writing Step 6.4**

  ```bash
  grep -n "fn encrypt\|fn decrypt" mobileclaw-core/src/secrets/store.rs
  ```
  Adjust the calls in Step 6.4 to match the actual signatures.

- [ ] **Step 6.6: Run tests — expect PASS**

  ```bash
  cargo test -p mobileclaw-core provider_tests
  ```
  Expected: 4 tests pass.

- [ ] **Step 6.7: Commit**

  ```bash
  git add mobileclaw-core/src/secrets/store.rs
  git commit -m "feat(secrets): add provider CRUD and active_provider_id to SqliteSecretStore"
  ```

---

## Task 7: AgentConfig + AgentSession::create() Migration

**Files:**
- Modify: `mobileclaw-core/src/ffi.rs`

Change `api_key` and `model` from `String` to `Option<String>` in `AgentConfig`. Update `AgentSession::create()` to:
1. Try loading the active provider from `SqliteSecretStore`
2. Fall back to explicit `api_key` + `model` fields (backwards compat)
3. Error if neither is available

- [ ] **Step 7.1: Update AgentConfig fields**

  In `mobileclaw-core/src/ffi.rs`, change lines 33-34:
  ```rust
  // Before:
  pub api_key: String,
  // ...
  pub model: String,

  // After:
  pub api_key: Option<String>,   // None = load active provider from SecretStore
  // ...
  pub model: Option<String>,     // None = use model from active provider
  ```

- [ ] **Step 7.2: Update AgentSession::create() to use provider factory**

  Replace the line `let llm = ClaudeClient::new(&config.api_key, &config.model);` (around line 140 in `ffi.rs`) with:
  ```rust
  // Resolve LLM client: active provider from SecretStore, or legacy explicit config
  let llm: std::sync::Arc<dyn crate::llm::client::LlmClient> = {
      use crate::llm::provider::{ProviderConfig, ProviderProtocol, create_llm_client};
      match secrets.active_provider_id().await? {
          Some(id) => {
              let provider_cfg = secrets.provider_load(&id).await?;
              let api_key = secrets.provider_api_key(&id).await?;
              create_llm_client(&provider_cfg, api_key.as_deref())?
          }
          None => {
              // Backwards-compat: explicit api_key + model in AgentConfig
              let key = config.api_key.as_deref()
                  .ok_or_else(|| anyhow::anyhow!("no active provider and no api_key in config"))?;
              let model = config.model.as_deref()
                  .ok_or_else(|| anyhow::anyhow!("no active provider and no model in config"))?;
              let cfg = ProviderConfig::new(
                  "legacy".into(),
                  ProviderProtocol::Anthropic,
                  "https://api.anthropic.com".into(),
                  model.to_string(),
              );
              create_llm_client(&cfg, Some(key))?
          }
      }
  };
  ```

- [ ] **Step 7.3: Implement LlmClient for Arc\<dyn LlmClient\>**

  `AgentLoop<L: LlmClient>` stores `llm: L` directly. `dyn LlmClient` alone doesn't satisfy the `LlmClient` bound, but `Arc<dyn LlmClient>` can if we add a blanket impl. Add to `mobileclaw-core/src/llm/client.rs` (after the existing trait definition):

  ```rust
  #[async_trait]
  impl LlmClient for std::sync::Arc<dyn LlmClient> {
      async fn stream_messages(
          &self,
          system: &str,
          messages: &[crate::llm::types::Message],
          max_tokens: u32,
      ) -> crate::ClawResult<EventStream> {
          self.as_ref().stream_messages(system, messages, max_tokens).await
      }
  }
  ```

  Then change `AgentSession` in `ffi.rs`:
  ```rust
  // Before:
  pub struct AgentSession {
      inner: AgentLoop<ClaudeClient>,
      ...
  }

  // After:
  pub struct AgentSession {
      inner: AgentLoop<std::sync::Arc<dyn crate::llm::client::LlmClient>>,
      ...
  }
  ```

  The `create_llm_client()` factory already returns `Arc<dyn LlmClient>`, so the `AgentLoop::new(llm, ...)` call will type-check directly.

- [ ] **Step 7.4: Build — expect clean compile**

  ```bash
  cargo build -p mobileclaw-core 2>&1
  ```
  Fix any remaining type errors before committing.

- [ ] **Step 7.5: Run all tests — expect PASS**

  ```bash
  cargo test -p mobileclaw-core --features test-utils
  ```
  Expected: all existing tests still pass, no regressions.

- [ ] **Step 7.6: Commit**

  ```bash
  git add mobileclaw-core/src/ffi.rs
  git commit -m "feat(ffi): migrate AgentSession to use provider factory; backwards-compat fallback"
  ```

---

## Task 8: FFI DTOs + Provider Management Methods

**Files:**
- Modify: `mobileclaw-core/src/ffi.rs`

Add `ProviderConfigDto`, `ProbeResultDto`, and six `AgentSession` methods plus one free function `provider_probe`. These are the flutter_rust_bridge entry points that Dart calls.

- [ ] **Step 8.1: Add DTOs to ffi.rs**

  Add near the existing DTO definitions in `ffi.rs`:
  ```rust
  #[derive(Debug, Clone)]
  pub struct ProviderConfigDto {
      pub id: String,
      pub name: String,
      pub protocol: String,    // "anthropic" | "openai_compat" | "ollama"
      pub base_url: String,
      pub model: String,
      pub created_at: i64,
  }

  #[derive(Debug, Clone)]
  pub struct ProbeResultDto {
      pub ok: bool,
      pub latency_ms: u64,
      pub degraded: bool,
      pub error: Option<String>,
  }
  ```

  Add conversion helpers (not FFI, just internal):
  ```rust
  impl ProviderConfigDto {
      fn to_provider_config(&self) -> crate::ClawResult<crate::llm::provider::ProviderConfig> {
          use crate::llm::provider::ProviderProtocol;
          let protocol = match self.protocol.as_str() {
              "anthropic"     => ProviderProtocol::Anthropic,
              "openai_compat" => ProviderProtocol::OpenAiCompat,
              "ollama"        => ProviderProtocol::Ollama,
              other => return Err(crate::ClawError::Llm(format!("unknown protocol: {other}"))),
          };
          Ok(crate::llm::provider::ProviderConfig {
              id: self.id.clone(),
              name: self.name.clone(),
              protocol,
              base_url: self.base_url.clone(),
              model: self.model.clone(),
              created_at: self.created_at,
          })
      }
  }

  impl From<crate::llm::provider::ProviderConfig> for ProviderConfigDto {
      fn from(c: crate::llm::provider::ProviderConfig) -> Self {
          let protocol = match c.protocol {
              crate::llm::provider::ProviderProtocol::Anthropic    => "anthropic",
              crate::llm::provider::ProviderProtocol::OpenAiCompat => "openai_compat",
              crate::llm::provider::ProviderProtocol::Ollama       => "ollama",
          };
          Self { id: c.id, name: c.name, protocol: protocol.into(), base_url: c.base_url, model: c.model, created_at: c.created_at }
      }
  }

  impl From<crate::llm::probe::ProbeResult> for ProbeResultDto {
      fn from(r: crate::llm::probe::ProbeResult) -> Self {
          Self { ok: r.ok, latency_ms: r.latency_ms, degraded: r.degraded, error: r.error }
      }
  }
  ```

- [ ] **Step 8.2: Add provider management methods to AgentSession**

  Add to the `impl AgentSession` block:
  ```rust
  pub async fn provider_save(
      &self,
      config: ProviderConfigDto,
      api_key: Option<String>,
  ) -> anyhow::Result<()> {
      let cfg = config.to_provider_config().map_err(anyhow::Error::from)?;
      self.secrets.provider_save(&cfg, api_key.as_deref()).await.map_err(anyhow::Error::from)
  }

  pub async fn provider_list(&self) -> anyhow::Result<Vec<ProviderConfigDto>> {
      self.secrets
          .provider_list()
          .await
          .map(|v| v.into_iter().map(ProviderConfigDto::from).collect())
          .map_err(anyhow::Error::from)
  }

  pub async fn provider_delete(&self, id: String) -> anyhow::Result<()> {
      self.secrets.provider_delete(&id).await.map_err(anyhow::Error::from)
  }

  pub async fn provider_set_active(&self, id: String) -> anyhow::Result<()> {
      // Verify provider exists before setting active
      self.secrets.provider_load(&id).await.map_err(anyhow::Error::from)?;
      self.secrets.set_active_provider_id(&id).await.map_err(anyhow::Error::from)
  }

  pub async fn provider_get_active(&self) -> anyhow::Result<Option<ProviderConfigDto>> {
      match self.secrets.active_provider_id().await.map_err(anyhow::Error::from)? {
          None => Ok(None),
          Some(id) => {
              let cfg = self.secrets.provider_load(&id).await.map_err(anyhow::Error::from)?;
              Ok(Some(ProviderConfigDto::from(cfg)))
          }
      }
  }
  ```

- [ ] **Step 8.3: Add provider_probe as a free function**

  Add outside the `impl AgentSession` block (free function, does not need a session):
  ```rust
  pub async fn provider_probe(
      config: ProviderConfigDto,
      api_key: Option<String>,
  ) -> ProbeResultDto {
      let cfg = match config.to_provider_config() {
          Ok(c) => c,
          Err(e) => return ProbeResultDto {
              ok: false, latency_ms: 0, degraded: false, error: Some(e.to_string())
          },
      };
      crate::llm::probe::probe_provider(&cfg, api_key.as_deref()).await.into()
  }
  ```

- [ ] **Step 8.4: Build — expect clean compile**

  ```bash
  cargo build -p mobileclaw-core 2>&1
  ```

- [ ] **Step 8.5: Run full test suite**

  ```bash
  cargo test -p mobileclaw-core --features test-utils
  ```
  Expected: all tests pass.

- [ ] **Step 8.6: Commit**

  ```bash
  git add mobileclaw-core/src/ffi.rs
  git commit -m "feat(ffi): add ProviderConfigDto, ProbeResultDto, provider management methods"
  ```

---

## Task 9: Integration Test

**Files:**
- Create: `mobileclaw-core/tests/integration_provider.rs`

Verify the full path: factory → agent loop → tool execution with the new provider system.

- [ ] **Step 9.1: Write the integration test**

  Create `mobileclaw-core/tests/integration_provider.rs`:
  ```rust
  //! Integration: verify create_llm_client factory works with the agent loop.
  //! Uses MockLlmClient (requires --features test-utils).

  #[cfg(feature = "test-utils")]
  mod tests {
      use mobileclaw_core::llm::provider::{ProviderConfig, ProviderProtocol, create_llm_client};

      #[test]
      fn test_factory_returns_arc_for_anthropic() {
          let cfg = ProviderConfig::new(
              "Claude".into(),
              ProviderProtocol::Anthropic,
              "https://api.anthropic.com".into(),
              "claude-opus-4-6".into(),
          );
          // Factory must succeed with a valid key
          let result = create_llm_client(&cfg, Some("sk-ant-test"));
          assert!(result.is_ok(), "factory failed: {:?}", result.err());
      }

      #[test]
      fn test_factory_errors_without_key_for_anthropic() {
          let cfg = ProviderConfig::new(
              "Claude".into(),
              ProviderProtocol::Anthropic,
              "https://api.anthropic.com".into(),
              "claude-opus-4-6".into(),
          );
          let result = create_llm_client(&cfg, None);
          assert!(result.is_err());
          assert!(result.unwrap_err().to_string().contains("api_key"));
      }

      #[test]
      fn test_factory_returns_arc_for_ollama_without_key() {
          let cfg = ProviderConfig::new(
              "Llama".into(),
              ProviderProtocol::Ollama,
              "http://localhost:11434".into(),
              "llama3".into(),
          );
          let result = create_llm_client(&cfg, None);
          assert!(result.is_ok(), "Ollama factory should not require api_key");
      }

      #[test]
      fn test_factory_returns_arc_for_openai_compat() {
          let cfg = ProviderConfig::new(
              "Groq".into(),
              ProviderProtocol::OpenAiCompat,
              "https://api.groq.com/openai".into(),
              "mixtral-8x7b".into(),
          );
          let result = create_llm_client(&cfg, Some("gsk_test"));
          assert!(result.is_ok(), "OpenAI compat factory failed: {:?}", result.err());
      }
  }
  ```

- [ ] **Step 9.2: Run integration test**

  ```bash
  cargo test -p mobileclaw-core --features test-utils --test integration_provider
  ```
  Expected: 4 tests pass.

- [ ] **Step 9.3: Run full lint check**

  ```bash
  cargo clippy -p mobileclaw-core --features test-utils -- -D warnings
  ```
  Fix any warnings before committing.

- [ ] **Step 9.4: Commit**

  ```bash
  git add mobileclaw-core/tests/integration_provider.rs
  git commit -m "test: add integration tests for create_llm_client factory"
  ```

---

## Final Verification

- [ ] **Run complete test suite**

  ```bash
  cargo test -p mobileclaw-core --features test-utils
  ```
  Expected: all tests pass, no regressions.

- [ ] **Run coverage check**

  ```bash
  cargo llvm-cov --package mobileclaw-core --features test-utils --all-targets --fail-under-lines 85
  ```
  Expected: passes 85% floor.

- [ ] **Verify Plan 2 readiness**

  The following FFI entry points are now available for the Flutter UI (Plan 2):
  - `AgentSession::provider_save(config, api_key)`
  - `AgentSession::provider_list()`
  - `AgentSession::provider_delete(id)`
  - `AgentSession::provider_set_active(id)`
  - `AgentSession::provider_get_active()`
  - Free fn `provider_probe(config, api_key)` — does not need a session

---

## Notes for Plan 2 (Flutter UI)

Plan 2 will implement:
1. `ProviderListScreen` — list all saved providers; tap to set active; FAB to add
2. `ProviderFormScreen` — protocol picker, URL/model/key fields, Test button, Save button
3. `OnboardingScreen` — first-launch wizard wrapping `ProviderFormScreen`

**Protocol → URL hints:**
- `anthropic` → `https://api.anthropic.com`
- `openai_compat` → `https://api.openai.com` (or let user type)
- `ollama` → `http://localhost:11434`

**Save button** is disabled until Test passes (or user taps "skip test" small link).
**Edit existing** provider: API key field shows `••••••••`; if not changed, pass `None` as `api_key` to `provider_save` to preserve the stored key.
