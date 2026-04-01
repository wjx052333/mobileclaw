# Flutter Email Feature — Test Case Design

**Date:** 2026-04-01  
**Scope:** `packages/mobileclaw_sdk` — email account management (save / load / delete)  
**Test file:** `test/mobileclaw_sdk_test.dart`  
**Testing approach:** TDD — all tests were written before implementation code

---

## 1. EmailAccountDto Model

Test group: `EmailAccountDto`

| # | Case | Input | Expected |
|---|------|-------|----------|
| M-1 | Fields are accessible | `EmailAccountDto(id:'work', smtpHost:'smtp.example.com', smtpPort:587, imapHost:'imap.example.com', imapPort:993, username:'alice@example.com')` | All fields return correct values |
| M-2 | No password field exposed | Call `.toString()` on a dto | Result does not contain 'password' |
| M-3 | Equal when all fields match | Two identical dtos | `dto == dto2` is true; `hashCode` matches |
| M-4 | Not equal when id differs | `id:'work'` vs `id:'personal'` | `dto != dto2` |
| M-5 | Not equal when smtpPort differs | `smtpPort:587` vs `smtpPort:465` | `dto != dto2` |

**Security invariant tested:** `EmailAccountDto` must carry zero sensitive data — `toString()` must never mention 'password'. This mirrors the Rust `EmailAccountDto` which has no password field by design.

---

## 2. MockMobileclawAgent — Email Account Management

Test group: `MockMobileclawAgent.email`

| # | Case | Setup | Action | Expected |
|---|------|-------|--------|----------|
| E-1 | Load returns null for unknown id | Fresh mock | `emailAccountLoad(id:'unknown')` | `null` |
| E-2 | Save completes without error | Fresh mock | `emailAccountSave(dto: dto, password: 's3cr3t')` | Completes normally |
| E-3 | Load returns dto after save | Save 'work' account | `emailAccountLoad(id:'work')` | Equals original dto |
| E-4 | Password not exposed after save | Save with 's3cr3t' password | `emailAccountLoad(id:'work')` | Loaded dto.toString() does not contain 's3cr3t' |
| E-5 | Save overwrites existing account | Save 'work', then save 'work' with different smtpHost | `emailAccountLoad(id:'work')` | Returns updated dto |
| E-6 | Delete removes account | Save then delete | `emailAccountLoad(id:'work')` | `null` |
| E-7 | Delete is idempotent for unknown id | Fresh mock | `emailAccountDelete(id:'nonexistent')` | Completes normally |
| E-8 | Multiple accounts stored independently | Save 'work' and 'personal' | Load both | Each returns correct dto |
| E-9 | Deleting one account does not affect others | Save 'work' and 'personal', delete 'work' | Load both | 'work' → null, 'personal' → correct dto |

**Password isolation invariant:** The mock deliberately discards the password on save. `emailAccountLoad` can never return it. This matches the Rust security contract where the password is AES-256-GCM encrypted and inaccessible after storage.

---

## 3. MobileclawAgentImpl — Email Account Integration (Linux)

Test group: `MobileclawAgentImpl email (Linux integration)`  
**Skipped by default.** Set `INTEGRATION=true` to run against the real Rust `.so` library.

| # | Case | Setup | Action | Expected |
|---|------|-------|--------|----------|
| I-1 | Create agent with secretsDbPath | Temp dir | `MobileclawAgentImpl.create(secretsDbPath:..., encryptionKey:...)` | No exception |
| I-2 | Email account save/load/delete round-trip | Create agent | Save → Load → Delete → Load | Load returns dto; after delete, Load returns null |

**Integration test parameters:**
- `secretsDbPath` — path to a temp SQLite file; created automatically by Rust if absent
- `encryptionKey` — 32-byte list; in tests uses `List.filled(32, 0x42)` (not secure, test-only)

**What these tests verify end-to-end:**
- `AgentConfig` correctly passes `secretsDbPath` and `encryptionKey` to Rust
- Rust `AgentSession.email_account_save/load/delete` FFI methods are callable
- AES-256-GCM encryption round-trip works: stored value is not plaintext; load retrieves the correct config (not the password — password stays in Rust)

---

## 4. Test Coverage Matrix

| Component | Unit tests | Integration tests |
|-----------|-----------|-------------------|
| `EmailAccountDto` (models.dart) | M-1 to M-5 | — |
| `MockMobileclawAgent` email methods | E-1 to E-9 | — |
| `MobileclawAgentImpl` email methods | — | I-1 to I-2 |
| FFI bridge `AgentSession.emailAccount*` | — | I-1 to I-2 |
| `AgentConfig` with `secretsDbPath` / `encryptionKey` | — (compile check) | I-1 |

---

## 5. What Is NOT Tested Here

The following items are tested in the **Rust test suite** (`cargo test -p mobileclaw-core`):

- AES-256-GCM encryption correctness (ciphertext not equal to plaintext)
- Wrong decryption key returns an error
- Encrypted password cannot be retrieved via any `SecretStore::get` call
- `SecretString` zeroes memory on drop
- `email_send` / `email_fetch` tool execution against real SMTP/IMAP servers

Flutter tests do not test Rust internals. The Flutter layer trusts the Rust security guarantees documented in `mobileclaw-core/docs/05-flutter-interface.md` § Security Contract.

---

## 6. Running the Tests

```bash
# Unit tests only (always runs):
cd mobileclaw-flutter/packages/mobileclaw_sdk
flutter test test/mobileclaw_sdk_test.dart

# Integration tests (requires Linux .so and real Rust binary):
INTEGRATION=true flutter test test/mobileclaw_sdk_test.dart
```

Expected baseline: **80 tests passing** (+ 4 integration tests skipped when INTEGRATION is not set).
