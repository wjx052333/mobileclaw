# mclaw_cli

Command-line tool for testing the mobileclaw SDK end-to-end. Provides a REPL for chatting with an LLM agent backed by the Rust `mobileclaw-core` engine via FFI.

## Prerequisites

### System dependencies

```bash
sudo apt install -y lld
```

`lld` is required for Flutter Linux builds.

### Rust native library

Build `libmobileclaw_core.so`:

```bash
cd /path/to/mobileclaw
cargo build --release -p mobileclaw-core
```

Output: `mobileclaw-core/target/release/libmobileclaw_core.so`

### LLM provider

The CLI requires a configured LLM provider (Anthropic API key or compatible provider). Configure one through the main `mobileclaw_app` first, or set it up via the SDK's `providerSave` / `providerSetActive` API.

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

The bundle directory contains everything needed:

```
build/linux/x64/release/bundle/
├── mclaw_cli
├── lib/
│   ├── libmobileclaw_core.so    ← Rust native library
│   ├── libflutter_linux_gtk.so
│   ├── libapp.so
│   └── libmobileclaw_sdk_plugin.so
└── data/
    └── flutter_assets/
```

### Interactive mode

```bash
cd build/linux/x64/release/bundle
LD_LIBRARY_PATH=./lib ./mclaw_cli
```

Type messages at the prompt. `quit` or `Ctrl+D` to exit.

### Single message mode

```bash
cd build/linux/x64/release/bundle
LD_LIBRARY_PATH=./lib ./mclaw_cli "What time is it?"
```

### Custom `.so` path

If the native library is elsewhere, override with `--so-path`:

```bash
cd build/linux/x64/release/bundle
LD_LIBRARY_PATH=./lib ./mclaw_cli --so-path /path/to/libmobileclaw_core.so "Hello"
```

### Verbose path

Run directly from the source directory (requires `.so` at the default relative path):

```bash
cd mobileclaw-flutter/mclaw_cli
flutter run -d linux
```

Or with a custom `.so` path:

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
