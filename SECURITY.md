# Security Policy

## Supported Versions

The following versions of **Rust-SQL** currently receive security updates:

| Version | Supported          |
| ------- | ------------------ |
| 1.39.x  | :white_check_mark: |
| < 1.39  | :x:                |

## Built-In Security Controls

1. **Credential Encryption with an OS Keychain Master Key (`src/core/crypto.rs`, `src/core/keys/`)**:
   - Saved database passwords, SSH passwords, and SSH key passphrases are encrypted at rest with XChaCha20-Poly1305 and a random 24-byte nonce per value, stored as `{"ciphertext": "v2:<base64>"}`. Values without a recognized version prefix are rejected.
   - The key is derived with HKDF-SHA256 from a random 256-bit master key kept in the OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service). There is no default key and no fallback store; if the keychain is unavailable, credentials are neither stored nor decrypted.
   - Each ciphertext is bound to its connection ID and field name as associated data, so a value moved to another connection or field fails to decrypt.
   - The master key and decrypted passwords are held in memory in buffers that are zeroed on drop.
2. **Read-Only Guardrail & Destructive Query Analyzer (`src/core/safety.rs`)**:
   - Connections can be locked into **Read-Only Mode** (`--read-only` or `🔒 Read-Only` in GUI), blocking all data-modifying and schema-altering statements (`INSERT`, `UPDATE`, `DELETE`, `DROP`, `ALTER`, `TRUNCATE`, `CREATE`).
   - Unconditional `DELETE` and `UPDATE` statements without `WHERE` clauses are flagged before execution.
3. **Safe Parameter Binding (`src/core/params.rs`)**:
   - Named (`:param`, `@param`) and positional (`$1`) bind variables escape string literals to prevent accidental SQL injection during interactive query testing.

## Reporting a Vulnerability

Please **do not** report security vulnerabilities through public GitHub issues.

Instead, please report them privately via [GitHub Security Advisories](https://github.com/Rain6435/Rust-SQL/security/advisories/new) or by emailing the maintainers directly.

When reporting a vulnerability, please include:
- A description of the issue and its potential impact.
- Step-by-step instructions or a minimal proof-of-concept to reproduce the issue.
- Affected versions and configurations.

We will acknowledge receipt within **48 hours** and work with you on a coordinated fix and disclosure timeline.
