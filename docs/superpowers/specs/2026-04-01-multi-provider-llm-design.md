# Multi-Provider LLM Support Design

**Date:** 2026-04-01  
**Status:** Design Complete, Ready for Implementation  
**Priority:** High (Performance-critical feature)

## Overview

Extend mobileclaw-core to support multiple LLM providers beyond Anthropic:
- **Anthropic (native)** — existing Claude API (SSE streaming)
- **OpenAI-compatible** — any `/v1/chat/completions` endpoint (Groq, DeepSeek, NVIDIA NIM, etc.)
- **Ollama** — local models, no API key required

Users can save multiple provider configurations, test availability, and switch between them. Active provider is persisted and restored on app launch.

## Design Principles

1. **Extreme Performance** — zero-copy configuration loading, no JSON registry at runtime, factory function called once per session
2. **Minimal Code** — three independent client structs, trait-based dispatch, no over-engineering
3. **Backwards Compatible** — existing `AgentConfig` still works; new provider API is additive
4. **Fail-Safe** — missing active provider falls back to explicit config; probe gracefully degrades

## Architecture

### 1. Rust Core Layer

#### New Files

```
src/llm/
  provider.rs     — ProviderConfig struct, ProviderProtocol enum, factory function
  openai_compat.rs — OpenAiCompatClient struct implementing LlmClient trait
  ollama.rs       — OllamaClient struct implementing LlmClient trait
  probe.rs        — provider availability testing (ping + minimal request)
```

#### File: `src/llm/provider.rs`

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ProviderProtocol {
    Anthropic,       // Native Claude API
    OpenAiCompat,    // /v1/chat/completions
    Ollama,          // /api/chat (local, no auth)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,                      // UUID, used as PK in SecretStore
    pub name: String,                    // Display name (e.g., "DeepSeek R1")
    pub protocol: ProviderProtocol,
    pub base_url: String,                // e.g., "https://api.deepseek.com"
    pub model: String,                   // e.g., "claude-opus-4-6"
    pub created_at: i64,                 // Unix timestamp
    // api_key stored separately in SecretStore, never in this struct
}

impl ProviderConfig {
    pub fn new(name: String, protocol: ProviderProtocol, base_url: String, model: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            protocol,
            base_url,
            model,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        }
    }
}

/// Factory function: single allocation per session
pub fn create_llm_client(
    config: &ProviderConfig,
    api_key: Option<&str>,
) -> ClawResult<Arc<dyn LlmClient>> {
    match config.protocol {
        ProviderProtocol::Anthropic => {
            let key = api_key.ok_or_else(|| ClawError::Llm("Anthropic requires api_key".into()))?;
            Ok(Arc::new(ClaudeClient::new(key, &config.model)))
        }
        ProviderProtocol::OpenAiCompat => {
            let key = api_key.ok_or_else(|| ClawError::Llm("OpenAI-compatible requires api_key".into()))?;
            Ok(Arc::new(OpenAiCompatClient::new(
                &config.base_url,
                key,
                &config.model,
            )?))
        }
        ProviderProtocol::Ollama => {
            Ok(Arc::new(OllamaClient::new(&config.base_url, &config.model)?))
        }
    }
}
```

#### File: `src/llm/openai_compat.rs`

OpenAI SSE format: all chunks arrive with **empty `event` field**. Each chunk is:
```
data: {"id":"...","choices":[{"delta":{"content":"Hello"},"index":0}]}
```
First chunk may have `{"delta":{"role":"assistant"}}` with no content (empty string). Last line is `data: [DONE]`. There is no `message_start` event type — `MessageStart` is emitted synthetically when the response HTTP connection succeeds.

**`StreamEvent` mapping for OpenAI SSE:**
- Connection established → emit `MessageStart`
- Each SSE chunk → extract `choices[0].delta.content`; if non-empty, emit `TextDelta { text }`; if empty, skip
- `data: [DONE]` → emit `MessageStop`

```rust
pub struct OpenAiCompatClient {
    base_url: String,   // always ends with "/v1" (normalised in constructor)
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl OpenAiCompatClient {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> ClawResult<Self> {
        // Normalise base_url to end with "/v1"
        let base_url = if base_url.ends_with("/v1") {
            base_url.to_string()
        } else if base_url.ends_with('/') {
            format!("{}v1", base_url)
        } else {
            format!("{}/v1", base_url)
        };

        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .build()
            .map_err(|e| ClawError::Llm(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self { base_url, api_key: api_key.to_string(), model: model.to_string(), http })
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
        // ... build body with system as first message, stream:true ...

        // SSE: eventsource_stream crate; all events have empty ev.event field
        // Prepend a synthetic MessageStart, then map each chunk
        let initial = futures::stream::once(async { Ok(StreamEvent::MessageStart) });

        let data_stream = resp.bytes_stream().eventsource().filter_map(|event| async {
            match event {
                Ok(ev) if ev.data == "[DONE]" => Some(Ok(StreamEvent::MessageStop)),
                Ok(ev) => {
                    // All content chunks: parse choices[0].delta.content
                    let v: serde_json::Value = match serde_json::from_str(&ev.data) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(ClawError::Parse(e.to_string()))),
                    };
                    let text = v["choices"][0]["delta"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    if text.is_empty() {
                        None  // skip role-only first chunk and empty deltas
                    } else {
                        Some(Ok(StreamEvent::TextDelta { text }))
                    }
                }
                Err(e) => Some(Err(ClawError::Llm(e.to_string()))),
            }
        });

        Ok(Box::pin(initial.chain(data_stream)))
    }
}
```

#### File: `src/llm/ollama.rs`

Ollama `/api/chat` with `stream: true` returns **newline-delimited JSON (NDJSON)**, not SSE.
Each line is a complete JSON object:
```json
{"model":"llama3","message":{"role":"assistant","content":"Hello"},"done":false}
{"model":"llama3","message":{"role":"assistant","content":"!"},"done":false}
{"model":"llama3","message":{"role":"assistant","content":""},"done":true}
```
Do **not** use `eventsource()` — use a line-by-line reader over the raw bytes stream.

**`StreamEvent` mapping for Ollama NDJSON:**
- First line → emit `MessageStart` before yielding
- Each non-done line → extract `message.content`; if non-empty, emit `TextDelta { text }`
- Line with `done: true` → emit `MessageStop`

**Implementation approach:** Convert the `reqwest` bytes stream into a `tokio::io::AsyncRead`
using `tokio_util::io::StreamReader`, then wrap with `tokio::io::BufReader` and read lines
via `AsyncBufReadExt::lines()`. This avoids buffering the entire response and keeps memory
usage constant regardless of response length.

```rust
// Dependency needed: tokio-util = { version = "...", features = ["io"] }
use tokio_util::io::StreamReader;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct OllamaClient {
    base_url: String,   // always ends with "/" (normalised in constructor)
    model: String,
    http: reqwest::Client,
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn stream_messages(
        &self,
        system: &str,
        messages: &[Message],
        _max_tokens: u32,  // Ollama /api/chat does not accept max_tokens
    ) -> ClawResult<EventStream> {
        // ... build body: system as first message, stream:true ...

        // NDJSON: read lines from response body, not SSE
        let byte_stream = resp
            .bytes_stream()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
        let reader = BufReader::new(StreamReader::new(byte_stream));
        let mut lines = reader.lines();

        let stream = async_stream::try_stream! {
            yield StreamEvent::MessageStart;
            while let Some(line) = lines.next_line().await.map_err(|e| ClawError::Llm(e.to_string()))? {
                if line.trim().is_empty() { continue; }
                let v: serde_json::Value = serde_json::from_str(&line)
                    .map_err(|e| ClawError::Parse(e.to_string()))?;
                let text = v["message"]["content"].as_str().unwrap_or("").to_string();
                let done = v["done"].as_bool().unwrap_or(false);
                if !text.is_empty() {
                    yield StreamEvent::TextDelta { text };
                }
                if done {
                    yield StreamEvent::MessageStop;
                    break;
                }
            }
        };

        Ok(Box::pin(stream))
    }
}
```

**New dependency:** `async-stream = "0.3"` for the `try_stream!` macro; `tokio-util` already in workspace.

#### File: `src/llm/probe.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub degraded: bool,     // true = completions failed but models endpoint succeeded
    pub error: Option<String>,
}

pub async fn probe_provider(config: &ProviderConfig, api_key: Option<&str>) -> ProbeResult {
    let start = std::time::Instant::now();

    // Strategy 1: Send minimal completion request
    let result = probe_with_request(config, api_key).await;
    if result.is_ok() {
        let latency_ms = start.elapsed().as_millis() as u64;
        return ProbeResult {
            ok: true,
            latency_ms,
            error: None,
        };
    }

    let error_msg = result.unwrap_err().to_string();

    // Strategy 2: Try model list endpoint (fails gracefully)
    let list_result = probe_models_endpoint(config, api_key).await;
    let latency_ms = start.elapsed().as_millis() as u64;

    if list_result.is_ok() {
        // Models endpoint responds but completion probe failed — ok but degraded
        return ProbeResult {
            ok: true,
            latency_ms,
            degraded: true,
            error: None,
        };
    }

    ProbeResult {
        ok: false,
        latency_ms,
        error: Some(error_msg),
    }
}

async fn probe_with_request(config: &ProviderConfig, api_key: Option<&str>) -> ClawResult<()> {
    let client = create_llm_client(config, api_key)?;
    let mut stream = client
        .stream_messages("You are helpful assistant", &[Message::user("Hi")], 16)
        .await?;

    // Consume first event to verify stream works
    match futures::StreamExt::next(&mut stream).await {
        Some(Ok(_)) => Ok(()),
        Some(Err(e)) => Err(e),
        None => Err(ClawError::Llm("Empty response stream".into())),
    }
}

async fn probe_models_endpoint(config: &ProviderConfig, api_key: Option<&str>) -> ClawResult<()> {
    let http = reqwest::Client::builder()
        .use_rustls_tls()
        .build()?;

    match config.protocol {
        ProviderProtocol::OpenAiCompat => {
            let mut url = config.base_url.clone();
            if !url.ends_with("/v1") {
                if !url.ends_with('/') {
                    url.push('/');
                }
                url.push_str("v1");
            }
            url.push_str("/models");

            let req = http
                .get(&url)
                .header("Authorization", format!("Bearer {}", api_key.unwrap_or("")));
            let resp = req.send().await?;
            if resp.status().is_success() {
                Ok(())
            } else {
                Err(ClawError::Llm(format!("Models endpoint: {}", resp.status())))
            }
        }
        ProviderProtocol::Ollama => {
            let mut url = config.base_url.clone();
            if !url.ends_with('/') {
                url.push('/');
            }
            url.push_str("api/tags");

            let resp = http.get(&url).send().await?;
            if resp.status().is_success() {
                Ok(())
            } else {
                Err(ClawError::Llm(format!("Ollama tags endpoint: {}", resp.status())))
            }
        }
        ProviderProtocol::Anthropic => {
            // Anthropic doesn't expose /models; skip this check
            Ok(())
        }
    }
}
```

#### AgentConfig Changes

Modify `src/ffi.rs`:

```rust
pub struct AgentConfig {
    pub api_key: Option<String>,        // ← now optional (loaded from SecretStore)
    pub db_path: String,
    pub sandbox_dir: String,
    pub http_allowlist: Vec<String>,
    pub model: Option<String>,          // ← now optional
    pub skills_dir: Option<String>,
    pub secrets_db_path: String,
}

impl AgentSession {
    pub async fn create(config: AgentConfig) -> anyhow::Result<AgentSession> {
        // Load active provider from SecretStore
        let (provider_config, api_key) = match secrets.active_provider_id()? {
            Some(id) => {
                let cfg = secrets.provider_load(&id)?;
                let key = secrets.provider_api_key(&id)?;
                (cfg, key)
            }
            None => {
                // Fallback to explicit config (backwards compat)
                if let (Some(key), Some(model)) = (&config.api_key, &config.model) {
                    let cfg = ProviderConfig::new(
                        "Legacy".to_string(),
                        ProviderProtocol::Anthropic,
                        "https://api.anthropic.com".to_string(),
                        model.clone(),
                    );
                    (cfg, Some(key.clone()))
                } else {
                    return Err(anyhow::anyhow!("No active provider configured"));
                }
            }
        };

        let llm = create_llm_client(&provider_config, api_key.as_deref())?;
        // ... rest of initialization
    }
}
```

**Existing `ClaudeClient::new` signature** (for reference — factory dispatches to this):
```rust
// src/llm/client.rs (unchanged)
pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self
// Returns Self directly (infallible). Factory wraps in Arc<dyn LlmClient>.
```

### 2. SecretStore Extension

#### SQLite Schema

```sql
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
    encrypted   BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS kv (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL   -- plaintext; used for non-secret config (e.g., active_provider_id)
);
```

#### Updated SecretStore Trait

```rust
pub trait SecretStore: Send + Sync {
    // Existing email methods...
    
    // New provider methods
    fn provider_save(&self, config: &ProviderConfig, api_key: Option<&str>) -> ClawResult<()>;
    fn provider_load(&self, id: &str) -> ClawResult<ProviderConfig>;
    fn provider_list(&self) -> ClawResult<Vec<ProviderConfig>>;
    fn provider_delete(&self, id: &str) -> ClawResult<()>;
    fn provider_api_key(&self, id: &str) -> ClawResult<Option<String>>;
    fn active_provider_id(&self) -> ClawResult<Option<String>>;
    fn set_active_provider_id(&self, id: &str) -> ClawResult<()>;
}
```

### 3. FFI Layer Extensions

```rust
// In src/ffi.rs
impl AgentSession {
    pub fn provider_save(
        &self,
        config_dto: ProviderConfigDto,
        api_key: Option<String>,
    ) -> ClawResult<()> {
        let config = config_dto.to_provider_config();
        self.secrets.provider_save(&config, api_key.as_deref())
    }

    pub fn provider_list(&self) -> ClawResult<Vec<ProviderConfigDto>> {
        self.secrets
            .provider_list()
            .map(|cfgs| cfgs.into_iter().map(ProviderConfigDto::from).collect())
    }

    pub fn provider_delete(&self, id: String) -> ClawResult<()> {
        self.secrets.provider_delete(&id)
    }

    pub fn provider_set_active(&self, id: String) -> ClawResult<()> {
        // Validate provider exists
        self.secrets.provider_load(&id)?;
        self.secrets.set_active_provider_id(&id)
    }

    pub fn provider_get_active(&self) -> ClawResult<Option<ProviderConfigDto>> {
        match self.secrets.active_provider_id()? {
            Some(id) => self.secrets.provider_load(&id).map(|c| Some(ProviderConfigDto::from(c))),
            None => Ok(None),
        }
    }
}

// Free function (doesn't need AgentSession)
pub async fn provider_probe(
    config_dto: ProviderConfigDto,
    api_key: Option<String>,
) -> ProbeResultDto {
    let config = config_dto.to_provider_config();
    let result = probe_provider(&config, api_key.as_deref()).await;
    ProbeResultDto::from(result)
}

// DTOs for FFI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfigDto {
    pub id: String,
    pub name: String,
    pub protocol: String,  // "anthropic" | "openai_compat" | "ollama"
    pub base_url: String,
    pub model: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResultDto {
    pub ok: bool,
    pub latency_ms: u64,
    pub degraded: bool,     // true = completions probe failed but models endpoint succeeded
    pub error: Option<String>,
}
```

`degraded: true` means the provider is reachable but the completions endpoint is unverified.
UI should show a warning, not a green checkmark.

### 4. Testing

- **Unit tests**: each client struct has `#[cfg(test)]` tests with mock HTTP responses
- **Integration tests**: `tests/provider_probe.rs` with real endpoints (Ollama local, or skip if unavailable)
- **MockLlmClient**: extended to support all three protocols for agent loop tests

### 5. Error Handling

New `ClawError` variants:
- `ClawError::ProviderNotFound(String)` — provider ID not in SecretStore (e.g., set_active called with unknown ID)
- All other provider errors reuse `ClawError::Llm(String)` — no new variant needed

## Performance Impact

- **Session creation**: +1 SecretStore query (load active provider) — negligible (~1ms)
- **Stream latency**: unchanged (same `stream_messages` trait path)
- **Memory**: +~200 bytes per saved provider config (stored in SQLite)
- **Probe operation**: one-time network call (~100-500ms), not on hot path

## Backwards Compatibility

- Existing `AgentConfig(api_key, model)` still works as fallback
- `AgentSession::create()` checks SecretStore first, then explicit config
- Existing code paths unaffected

## Future: Multimodal Capability Detection (tracked separately, not in this plan)

Borrow ironclaw's `image_models.rs` / `vision_models.rs` patterns:
- `is_vision_model(model_name: &str)` — pattern matching on model string
- If model supports vision: accept image/video inputs directly
- If not: warn user that media inputs are unsupported
- Video-to-frame conversion is a separate task

## Testing Strategy

**`test_openai_client_streaming`**

Input (mock HTTP 200 response body — SSE):
```
data: {"id":"1","choices":[{"delta":{"role":"assistant"},"index":0}]}
data: {"id":"1","choices":[{"delta":{"content":"Hello"},"index":0}]}
data: {"id":"1","choices":[{"delta":{"content":" world"},"index":0}]}
data: [DONE]
```
Expected `StreamEvent` sequence: `[MessageStart, TextDelta("Hello"), TextDelta(" world"), MessageStop]`
First chunk has no `content` field → skipped. `[DONE]` → `MessageStop`.

---

**`test_ollama_client_streaming`**

Input (mock HTTP 200 response body — NDJSON, one object per line):
```
{"model":"llama3","message":{"role":"assistant","content":"Hi"},"done":false}
{"model":"llama3","message":{"role":"assistant","content":"!"},"done":false}
{"model":"llama3","message":{"role":"assistant","content":""},"done":true}
```
Expected `StreamEvent` sequence: `[MessageStart, TextDelta("Hi"), TextDelta("!"), MessageStop]`
Last line has empty content and `done:true` → emit `MessageStop` without `TextDelta`.

---

| Test | Input | Expected Output |
|------|-------|-----------------|
| `test_openai_client_streaming` | SSE with role-only first chunk + content chunks + `[DONE]` | `[MessageStart, TextDelta×N, MessageStop]` (role-only chunk skipped) |
| `test_ollama_client_streaming` | NDJSON lines with `done:false` + final `done:true` empty-content | `[MessageStart, TextDelta×N, MessageStop]` |
| `test_provider_config_roundtrip` | `ProviderConfig { protocol: Ollama, .. }` → serialize → deserialize | Round-trips losslessly, `ProviderProtocol::Ollama` preserved |
| `test_probe_completion_success` | Mock HTTP 200 with valid first SSE/NDJSON chunk | `ProbeResult { ok: true, degraded: false, error: None }` |
| `test_probe_models_fallback` | Completion mock 400, models mock 200 | `ProbeResult { ok: true, degraded: true, error: None }` |
| `test_probe_all_fail` | Both completion and models mock 401 | `ProbeResult { ok: false, degraded: false, error: Some("...") }` |
| `test_secretstore_provider_crud` | Save 2 providers, list, delete one, list again | List returns 1; deleted ID returns `provider_load` error |
| `test_active_provider_persistence` | Set active → drop SecretStore → new SqliteSecretStore same path → get active | Same ID returned |
| `integration_agent_with_provider` | `MockLlmClient` via `create_llm_client` factory | Agent loop completes, history updated |

## Migration Path (for existing users)

No migration needed:
1. First app launch after update: no active provider → show OnboardingScreen
2. User configures one → saved to SecretStore
3. `AgentSession::create()` loads it automatically
4. Legacy code passing explicit `api_key` + `model` still works as fallback

---

## Sign-Off

This design is:
- ✓ High performance (zero-copy, single factory call)
- ✓ Minimal code (3 client structs, 1 probe function)
- ✓ Backwards compatible (existing config still works)
- ✓ Extensible (adding providers is a new struct + factory dispatch)
- ✓ Test-friendly (trait-based, easy to mock)
