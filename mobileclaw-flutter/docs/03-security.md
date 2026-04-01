# Security Constraints

**Security is a first-class requirement. Any feature that violates these  
constraints must be blocked at review, not patched post-merge.**

---

## 1. The Five Security Boundaries

These map to `B1`–`B5` in the audit log (`category = 'security'`).  
Every violation is logged with `blocked = 1` and surfaced as a  
`FfiEvent::Error` with the corresponding error code.

### B1 — Dart → Rust (claw_ffi)

All parameters crossing the FFI boundary are validated **on the Rust side**  
before any processing:

| Check | Limit | Error code |
|-------|-------|-----------|
| String length (path) | ≤ 4 096 bytes | `B1_PATH_TOO_LONG` |
| String length (message) | ≤ 1 MB | `B1_MSG_TOO_LONG` |
| Null bytes in strings | reject | `B1_NULL_BYTE` |
| Path contains `..` | reject raw input | `B1_PATH_DOTDOT` |
| Enum variant unknown | reject | `B1_INVALID_ENUM` |

**Flutter rule**: never construct raw FFI types from untrusted user input.  
Always pass through the typed `MobileclawAgent` API.

### B2 — Rust → LLM API

- API Key lives in the **platform Keystore / Keychain only**.  
  Never in SQLite, Dart heap, or `FfiConfig` as a plaintext field.  
  `FfiConfig.keystore_alias` is an opaque string; Rust reads the secret  
  directly via JNI (Android) / `Security.framework` (iOS).
- **SPKI Hash Pinning** on Anthropic's leaf certificate.  
  Three pins stored as `ANTHROPIC_SPKI_PINS` constants in `claw_llm`.
- `FfiEvent::CertPinUpdate` carries a server-side Ed25519 signature;  
  Rust verifies before updating the in-memory pin list.
- Outbound request body is scanned for absolute paths (`/home/…`,  
  `/data/…`) — match → `B2_PATH_LEAK`, request blocked.
- TLS 1.2+ only; SSLv3 / TLS 1.0 / 1.1 rejected.

**Flutter rule**: do NOT add `disable_cert_pinning: true` to `FfiConfig`  
except in an enterprise MDM deployment with explicit customer sign-off.

### B3 — Rust → File System (path jail)

- All file operations use `cap_std::fs::Dir` — capability-based, no  
  `PathBuf` escapes the jail root.
- `O_NOFOLLOW` on all opens; symlinks cannot escape the sandbox.
- **Never** use `std::fs::canonicalize()` followed by `open()` (TOCTOU).
- Write operations: single-file size ≤ 50 MB (`B3_FILE_TOO_LARGE`).
- `.db` / `.jsonl` extensions are write-protected (`B3_PROTECTED_EXT`).

**Flutter rule**: file paths passed to the SDK must be relative to the  
app's sandbox directory. Never construct absolute `/data/…` paths in Dart  
and pass them as arguments.

### B4 — Rust → WebDAV

- WebDAV URL must use `https://`; reject `http://` (`B4_HTTP_ONLY`).
- Credentials stored in Keystore, never in SQLite plain text.
- Download size ≤ 100 MB per file.
- Downloaded Skill packages require independent Ed25519 signature  
  verification (see B5) before installation.

### B5 — Rust → WASM Tool (most critical)

Third-party Skill WASM modules run in a fully isolated sandbox:

- **No WASI** — no direct file system access.
- **No raw networking** — only through the host whitelist functions  
  (`wasm_http_send` etc.), which enforce the domain whitelist.
- Each execution creates a **fresh WASM instance** — no persistent state.
- **Fuel metering**: CPU cycle budget prevents infinite loops.
- **Memory cap**: 8 MB per instance.
- Import whitelist enforced at load time; any unknown import → `B5_IMPORT_DENIED`.
- Output scanned for API Key patterns and absolute paths before returning  
  to the Agent (`B5_LEAK_DETECTED`).

**Flutter rule**: do not expose raw WASM execution to UI code. All Skill  
invocations go through `SkillManager` which enforces trust levels.

---

## 2. Trust Level Cascade

When any `Community` or `Local` trust skill is active in a session,  
the **entire session** automatically downgrades to read-only mode:
- `file_write` disabled
- `memory_write` disabled
- `db_execute` disabled

This is enforced in `ToolRegistry.get()` — not a UI hint.

---

## 3. Audit Log

All `category = 'security'` events must be surfaced to the user  
(e.g., a dismissible error banner in `error_banner.dart`). They must  
never be silently swallowed.

```sql
-- Security violations only
SELECT boundary, event, detail, ts
FROM audit_log
WHERE category = 'security'
ORDER BY ts DESC
LIMIT 50;
```

---

## 4. API Key Handling in Flutter

```dart
// CORRECT — store key in platform secure storage BEFORE init
await FlutterSecureStorage().write(
  key: 'anthropic_key',
  value: rawApiKey,
);
final agent = await MobileclawAgent.create(
  apiKey: 'anthropic_key',   // alias only — NOT the raw key
  ...
);

// WRONG — never pass the raw key
final agent = await MobileclawAgent.create(
  apiKey: 'sk-ant-api03-...',  // raw key visible in Dart heap / crash dumps
  ...
);
```

---

## 5. What the Flutter Layer Must Not Do

| Prohibited action | Reason |
|-------------------|--------|
| Pass raw API key in `apiKey` field | Key lands in Dart heap + crash dumps |
| Construct file paths with `..` | B3 path traversal |
| Fetch from `http://` URLs | Plaintext traffic interceptable |
| Store API key in SharedPreferences | Not encrypted on all platforms |
| Call `agent.chat()` from multiple isolates | Not thread-safe |
| Ignore `ClawException` with type `B2_CERT_PIN_FAIL` | Certificate pinning failure may indicate MITM |
