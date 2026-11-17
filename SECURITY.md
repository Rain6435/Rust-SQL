# Security Policy

## Supported Versions

The following versions of **Rust-SQL** currently receive security updates:

| Version | Supported          |
| ------- | ------------------ |
| 1.39.x  | :white_check_mark: |
| < 1.39  | :x:                |

## Built-In Security Controls

1. **AES-256-CBC Credential Encryption (`src/core/crypto.rs`)**:
   - Database and SSH passwords saved in `sqlectron.json` are encrypted at rest using AES-256-CBC with per-password random 16-byte IVs (`EncryptedPassword { ivText, encryptedText }`).
   - Legacy insecure cipher formats (`unsafeDecrypt`) are explicitly rejected.
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
