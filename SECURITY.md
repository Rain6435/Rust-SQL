# Security Policy

## Supported Versions

The following versions of **Rust-SQL** currently receive security updates:

| Version | Supported          |
| ------- | ------------------ |
| 1.41.x  | :white_check_mark: |
| < 1.41  | :x:                |

## Built-In Security Controls

1. **Credential Encryption with an OS Keychain Master Key (`src/core/crypto.rs`, `src/core/keys/`)**:
   - Saved database passwords, SSH passwords, and SSH key passphrases are encrypted at rest with XChaCha20-Poly1305 and a random 24-byte nonce per value, stored as `{"ciphertext": "v2:<base64>"}`. Values without a recognized version prefix are rejected.
   - The key is derived with HKDF-SHA256 from a random 256-bit master key kept in the OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service). There is no default key and no fallback store; if the keychain is unavailable, credentials are neither stored nor decrypted.
   - Each ciphertext is bound to its connection ID and field name as associated data, so a value moved to another connection or field fails to decrypt.
   - The master key and decrypted passwords are held in memory in buffers that are zeroed on drop.
2. **Read-Only Guardrail & Destructive Query Analyzer (`src/core/safety/`)**:
   - Statements are parsed into an AST with `sqlparser` in the connection's dialect. The analyzer fails closed: a statement it cannot parse is `Critical`, needs confirmation, and is blocked in Read-Only mode.
   - **Read-Only Mode** (`--read-only` or `🔒 Read-Only` in the desktop GUI) runs only statements positively recognized as read-only. Anything else, including statements the analyzer does not recognize, is treated as a modification and blocked.
   - Read-Only Mode is also enforced by the database session where supported: PostgreSQL/Redshift read-only transactions, SQLite `SQLITE_OPEN_READ_ONLY` (files) or `PRAGMA query_only` (in-memory and emulated). If the session cannot be made read-only, nothing runs.
   - `DELETE` and `UPDATE` without a top-level `WHERE` (a `WHERE` in a comment or a subquery does not count), `DROP TABLE`/`SCHEMA`/`DATABASE`, and `TRUNCATE` require confirmation.
3. **Prepared-Statement Parameter Binding (`src/core/params/`)**:
   - Named (`:param`, `@param`) and positional (`$1`) parameters are sent through the driver's prepared-statement API with their type (text, number, boolean, `NULL`), never pasted into the SQL text. The copy with values filled in, shown in history, is labeled "display only, not executed".
4. **Web Server Accounts, Sessions, and TLS (`src/web/`, `src/core/appdb/`)**: see [docs/app/web-server.md](docs/app/web-server.md).
5. **SSH Tunnels with Pinned Host Keys (`src/core/tunnel/`)**: trust on first use with a 180-day re-confirmation timer; a changed key is refused.

The [threat model](docs/security/threat-model.md) describes what these controls protect against, what they do not, and the known gaps.

## Reporting a Vulnerability

Please **do not** report security vulnerabilities through public GitHub issues.

Instead, please report them privately via [GitHub Security Advisories](https://github.com/Rain6435/Rust-SQL/security/advisories/new) or by emailing the maintainers directly.

When reporting a vulnerability, please include:
- A description of the issue and its potential impact.
- Step-by-step instructions or a minimal proof-of-concept to reproduce the issue.
- Affected versions and configurations.

We will acknowledge receipt within **48 hours** and work with you on a coordinated fix and disclosure timeline.
