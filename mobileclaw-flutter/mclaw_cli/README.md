# mclaw_cli

Command-line tool for testing the mobileclaw SDK end-to-end. Provides a REPL for chatting with an LLM agent backed by the Rust `mobileclaw-core` engine via FFI.

## Prerequisites

### System dependencies

```bash
sudo apt install -y lld
```

`lld` is required for Flutter Linux builds.

### Rust native library

The `libmobileclaw_core.so` is built and bundled automatically during the Flutter build process. However, if you need to rebuild it manually:

```bash
cd /path/to/mobileclaw
cargo build --release -p mobileclaw-core
```

Output: `target/release/libmobileclaw_core.so`

### LLM provider management

The CLI stores LLM provider configurations (Anthropic, OpenAI, Ollama, etc.) in an encrypted database. Before chatting, you must add and activate a provider:

```bash
./mclaw_cli providers                 # List all providers and active one
./mclaw_cli providers add             # Add a new provider (interactive)
./mclaw_cli providers set <name>      # Activate a provider by name
./mclaw_cli providers delete <name>   # Delete a provider
```

#### Adding a provider

Run `providers add` for an interactive prompt with presets:

```
$ ./mclaw_cli providers add
Choose a preset or enter "custom":
  1) Anthropic (claude-opus-4-6)
  2) OpenAI (gpt-4o)
  3) Ollama (localhost:11434 / llama3)
  4) Custom

Select: 1
Display name [Anthropic]: 
Base URL [https://api.anthropic.com]: 
Model [claude-opus-4-6]: 
Now enter the API key (will be stored encrypted):
API key: sk-ant-...
Provider "Anthropic" saved.
Provider "Anthropic" set as active.
```

**Supported protocols:**
- `anthropic` — Anthropic Claude API
- `openai_compat` — OpenAI-compatible endpoints (OpenAI, Ollama, vLLM, etc.)
- `ollama` — Local Ollama inference server

API keys are stored encrypted with AES-256-GCM in the secrets database.

## Building

From the `mclaw_cli/` directory:

```bash
flutter pub get
flutter build linux
```

The compiled binary lands at:

```
build/linux/x64/release/bundle/mclaw_cli
```

## Running

The compiled bundle contains all dependencies (native library, Flutter runtime, assets). Add a provider first, then run the CLI.

### Interactive mode

```bash
cd build/linux/x64/release/bundle
./mclaw_cli
```

At the prompt, type messages to chat. Ctrl+D to exit.

Example:
```
[mclaw] Native library: ./lib/libmobileclaw_core.so
[mclaw] FFI bridge initialized
[mclaw] Data dir: ~/.local/share/com.example.mclaw_cli/mobileclaw_cli/
[mclaw] Provider: Anthropic (anthropic/claude-opus-4-6)
[mclaw] Ready.

Type a message, or Ctrl+D to exit.

You: Hello
Assistant: Hi there! How can I help you today?
Done (35 chars)
---
```

### Single message mode

```bash
cd build/linux/x64/release/bundle
./mclaw_cli "What time is it?"
```

### Custom native library path

If the `.so` is elsewhere, override with `--so-path`:

```bash
./mclaw_cli --so-path /path/to/libmobileclaw_core.so "Hello"
```

### Development mode (hot reload)

Run from source with `flutter run`:

```bash
cd mobileclaw-flutter/mclaw_cli
flutter run -d linux
```

To override the `.so` path in development:

```bash
flutter run -d linux -- --so-path /absolute/path/to/libmobileclaw_core.so
```

## Architecture

```
mclaw_cli (Dart/Flutter)
    ↓ flutter_rust_bridge (SSE codec)
libmobileclaw_core.so (Rust cdylib)
    ↓
AgentLoop → LlmClient → Anthropic/OpenAI API
           ToolRegistry (file, memory, http, time, email)
           SqliteMemory (FTS5)
           SqliteSecretStore (AES-256-GCM)
           SkillManager
```

The CLI loads the native library at runtime via `ExternalLibrary.open(soPath)` — it is **not** linked at compile time. This allows swapping the Rust library without recompiling the Dart layer.

## Data directories

| Item | Location |
|---|---|
| Data dir | `~/.local/share/com.example.mclaw_cli/mobileclaw_cli/` |
| Memory DB | `<data>/claw.db` |
| Secrets DB | `<data>/secrets.db` |
| Workspace | `<data>/workspace/` |
| Log file | `<data>/flutter.log` |

Clear all data:

```bash
rm -rf ~/.local/share/com.example.mclaw_cli/mobileclaw_cli/
```
