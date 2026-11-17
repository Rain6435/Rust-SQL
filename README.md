# Rust-SQL

[![CI](https://github.com/Rain6435/Rust-SQL/actions/workflows/ci.yml/badge.svg)](https://github.com/Rain6435/Rust-SQL/actions/workflows/ci.yml)
[![Release](https://github.com/Rain6435/Rust-SQL/actions/workflows/release.yml/badge.svg)](https://github.com/Rain6435/Rust-SQL/actions/workflows/release.yml)
[![Security Audit](https://github.com/Rain6435/Rust-SQL/actions/workflows/security-audit.yml/badge.svg)](https://github.com/Rain6435/Rust-SQL/actions/workflows/security-audit.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A fast, lightweight, cross-database, and cross-platform SQL client written in Rust with native Desktop GUI, Terminal UI, Web UI, and headless CLI batch execution.

---

## Supported Databases

Only three dialects connect to a real server today. The other four have no live driver yet.

| Dialect | Key | Status |
| --- | --- | --- |
| **PostgreSQL** | `postgresql` (default port `5432`) | **Live** via `tokio-postgres` |
| **Amazon Redshift** | `redshift` (default port `5432`) | **Live** via `tokio-postgres` (PostgreSQL wire protocol) |
| **SQLite** | `sqlite` (file or `:memory:`) | **Live** via `rusqlite` |
| **MySQL** | `mysql` (default port `3306`) | **Not implemented.** Emulation only, opt-in |
| **MariaDB** | `mariadb` (default port `3306`) | **Not implemented.** Emulation only, opt-in |
| **Microsoft SQL Server** | `sqlserver` (default port `1433`) | **Not implemented.** Emulation only, opt-in |
| **Apache Cassandra** | `cassandra` (default port `9042`) | **Not implemented.** Emulation only, opt-in |

Known limits of the live PostgreSQL/Redshift driver:

- TLS is not implemented yet. A connection with SSL enabled is refused rather than sent unencrypted.
- Query results are returned as text exactly as the server renders them (or `NULL`).
- `CREATE TABLE/VIEW/ROUTINE` script generation and "truncate all tables" return an explicit
  "not implemented for live connections" error.

A failed PostgreSQL or Redshift connection (wrong password, unreachable host, missing database)
is always reported as an error naming the host, port, and database. It never falls back to
emulated data, and the password is never included in the message.

### Emulation (opt-in only)

Connecting to MySQL, MariaDB, SQL Server, or Cassandra fails with a "live connections for this
dialect are not implemented" error unless you explicitly opt in to the in-memory emulator:

- start Rust-SQL with `--emulate`, or
- set `"emulateUnsupportedDialects": true` in the [configuration file](docs/app/configuration-file.md).

The emulator runs your SQL against a seeded in-memory SQLite database (`roles`, `users`,
`email_view`). **Its results are not from your server.** While a connection is emulated, every
front end shows **"EMULATED — results are not from your server."**: the desktop GUI above each
result set and in the footer, the TUI in the results title and status bar, the web UI in a red
banner and above each result set, and the CLI on stderr. The opt-in never affects PostgreSQL,
Redshift, or SQLite, which always connect live.

### SSH tunnels

A connection can go through an SSH bastion (PostgreSQL and Redshift; the SSH client is
[`russh`](https://crates.io/crates/russh)). Rust-SQL authenticates to the bastion and forwards
the database connection over an SSH `direct-tcpip` channel, so the database host is resolved by
the bastion and private hosts work. It never connects to the database directly.

- **Authentication**: exactly one method is used, in this order of preference: the SSH agent
  (when "Use ssh agent" is on, via `SSH_AUTH_SOCK`; the OpenSSH agent pipe on Windows), then the
  private key (with its passphrase, if any), then the password. If that method fails, the error
  names it and no other method is tried.
- **Host keys (trust on first use)**: the first connection to a bastion shows its SHA-256
  fingerprint and asks you to approve it. Approved keys are pinned per `host:port` and must be
  re-confirmed every **180 days**. If the bastion ever presents a different key, the connection is
  refused with no in-app override; delete the pin under Settings → "Trusted SSH host keys" once
  your administrator confirms the key was rotated. In batch mode, pass
  `--trust-host-key SHA256:...` to approve a key whose fingerprint you already know.
- Passwords, passphrases, key material, and the agent socket path are never included in errors
  or logs.

### Saved passwords and the OS keychain

Saved database passwords, SSH passwords, and SSH key passphrases are encrypted in
`rust-sql.json` with XChaCha20-Poly1305. Each value is bound to its connection and field, so it
cannot be copied onto another connection. The encryption key is a random 256-bit master key that
Rust-SQL creates on first use and keeps in the operating system keychain (service `rust-sql`,
one entry per data directory). **The keychain is required; there is no fallback.**

| Platform | Keychain | Status |
| --- | --- | --- |
| macOS | Keychain | Supported |
| Windows | Credential Manager | Supported |
| Linux | Secret Service (GNOME Keyring, KWallet, KeePassXC, ...) over D-Bus | Supported |
| Linux without a running Secret Service (headless servers, minimal containers) | None | **Unsupported** |

If the keychain cannot be reached, or access is denied, Rust-SQL refuses to store or decrypt
saved credentials and says why; connections without a saved password still work, and the web
server refuses to start. If the keychain entry is deleted, saved passwords cannot be recovered
and must be entered again.

---

## Core Features

1. **Read-Only Guardrail & Destructive Query Safety Analyzer ([src/core/safety/](src/core/safety/mod.rs))**:
   - **Statements are parsed, not pattern-matched.** Each statement is parsed into an AST with [`sqlparser`](https://crates.io/crates/sqlparser) using the connection's dialect (PostgreSQL, Redshift, MySQL/MariaDB, SQL Server, SQLite; Cassandra CQL uses the generic grammar). Comments never count, and only a top-level `WHERE` counts for `DELETE` and `UPDATE`.
   - **Read-Only Mode (`🔒 Read-Only` in the desktop GUI / `--read-only` in the CLI)**: only statements the analyzer positively recognizes as read-only run: `SELECT`/`VALUES`/`WITH ... SELECT` without `INTO` or `FOR UPDATE`, `EXPLAIN` (without `ANALYZE` of a write), `SHOW`/`DESCRIBE`, introspection `PRAGMA`s, and `BEGIN`/`COMMIT`/`ROLLBACK`. **Every other statement, including one the analyzer does not recognize or cannot parse, is treated as a modification and blocked** (for example `WITH ... DELETE`, `MERGE`, `REPLACE`, `CALL`, `COPY`, `GRANT`, `SET`, `SELECT ... INTO`).
   - **Enforced by the database too.** In Read-Only mode the session itself is read-only: PostgreSQL and Redshift run `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY`, SQLite files are opened with `SQLITE_OPEN_READ_ONLY`, and in-memory SQLite and the emulator use `PRAGMA query_only`. If the database refuses, the query is not run. The analyzer is a guardrail; the session setting is the backstop for things it cannot see, such as functions with side effects.
   - **Destructive Query Detection (`analyze_script`)**: flags `DELETE`/`UPDATE` without a top-level `WHERE`, `DROP TABLE`/`SCHEMA`/`DATABASE`, `TRUNCATE`, and any statement that cannot be parsed as `Critical`, and asks for confirmation before running them. It fails closed: an unparseable script is never reported as safe.
   - Read-Only mode is available in the desktop GUI and the headless CLI. The terminal UI and web UI do not have a Read-Only mode yet.
2. **Parameterized Queries ([src/core/params.rs](src/core/params.rs))**:
   - Automatically detects `:named`, `@named`, and `$1` positional bind variables in SQL queries and renders inline parameter inputs in the GUI (`extract_query_parameters`, `bind_query_parameters`) with safe SQL literal escaping.
3. **Saved SQL Bookmarks & Searchable Execution History ([src/core/bookmarks.rs](src/core/bookmarks.rs))**:
   - Save, tag, search, and insert reusable SQL snippets (`SqlBookmark`).
   - Full execution history audit trail (`QueryHistoryEntry`) recording database, SQL, duration (`ms`), row count, and status with one-click replay.
4. **Interactive Result Filtering, Sorting & Column Profiler ([src/core/result_ops.rs](src/core/result_ops.rs))**:
   - Client-side instant row filtering (`filter_result_rows`) and clickable column sorting (`sort_result_rows` ascending/descending).
   - **Column Summary Statistics (`📊 Stats` / `compute_column_stats`)**: Computes total rows, null count, distinct count, min, max, sum, and mean for every column in a result set.
5. **Automated Schema ER Diagram Generator ([src/core/result_ops.rs](src/core/result_ops.rs))**:
   - Generates a Mermaid `erDiagram` (`generate_mermaid_erd`) from database tables, columns, primary keys (`PK`), and foreign key references (`FK`), available both in the GUI (`🗺 Schema ERD`) and CLI (`--erd`).
6. **Multi-Format Data Export ([src/core/export.rs](src/core/export.rs))**:
   - Export query results to **CSV**, **JSON**, **JSON Lines (`JSONL` / `NDJSON`)**, **GitHub-Flavored Markdown Tables (`MARKDOWN`)**, and **SQL `INSERT INTO` Statements (`SQL`)** (to file or clipboard).
7. **Headless CLI Batch Execution ([src/main.rs](src/main.rs))**:
   - Run queries directly from CI/CD scripts or the terminal via `-e / --exec "<SQL>"`, `--uri <CONN_URI>`, `--server <NAME_OR_ID>`, `--format <json|jsonl|csv|markdown|sql>`, `--read-only`, `--erd`, and `--emulate` (see [Emulation](#emulation-opt-in-only)).

---

## Three Built-In User Interfaces

1. **Native Desktop GUI (`--gui`)**: Powered by `eframe` / `egui` ([src/gui/mod.rs](src/gui/mod.rs)).
2. **Terminal UI (`--tui` / `--term`)**: Powered by `ratatui` + `crossterm` ([src/tui/mod.rs](src/tui/mod.rs)).
3. **Multi-user Web UI (`--web`)**: Powered by `axum` ([src/web/mod.rs](src/web/mod.rs)). People sign in with accounts stored in the application database and see only the saved connections they have been granted. It serves HTTPS (rustls) and refuses to start on a non-loopback address without TLS; plain HTTP is allowed only on loopback with `--insecure-http`. Sessions, CSRF tokens, login rate limiting, `Host` validation, and an audit log are built in. See [docs/app/web-server.md](docs/app/web-server.md).

---

## Building & Running

```bash
# Run all unit and integration tests
cargo test

# Launch Native Desktop GUI
cargo run -- --gui

# Launch Terminal UI
cargo run -- --tui

# Create the first web administrator, then launch the Web UI (development: plain HTTP on loopback)
cargo run -- users create --admin --username admin
cargo run -- --web --insecure-http --port 8080

# Execute a headless batch query and format as Markdown
cargo run -- --uri "postgres://localhost:5432/rust_sql" -e "SELECT * FROM users" --format markdown

# Generate a Mermaid ER Diagram from the CLI
cargo run -- --uri "postgres://localhost:5432/rust_sql" --erd
```

### Docker

```bash
# The container binds 0.0.0.0, so it needs TLS configured in /data/rust-sql.json
# (web.tlsCertPath / web.tlsKeyPath) and an administrator account:
docker compose run --rm rust-sql-web users create --admin --username admin
docker compose up --build
```

---

## Documentation & Community

- [Configuration File Guide](docs/app/configuration-file.md)
- [Keyboard Shortcuts](docs/app/keyboard-shortcuts.md)
- [Logging Configuration](docs/app/logging.md)
- [Contributing Guide](CONTRIBUTING.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)
- [Security Policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [MIT License](LICENSE)
