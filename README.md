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

---

## Core Features

1. **Read-Only Guardrail & Destructive Query Safety Analyzer ([src/core/safety.rs](src/core/safety.rs))**:
   - **Read-Only Mode (`🔒 Read-Only` / `--read-only`)**: Blocks any `INSERT`, `UPDATE`, `DELETE`, `CREATE`, `ALTER`, `DROP`, or `TRUNCATE` statement from running against protected connections.
   - **Destructive Query Detection (`analyze_query_safety`)**: Automatically flags `DELETE` or `UPDATE` statements missing a `WHERE` clause, as well as `DROP` and `TRUNCATE` statements, presenting a confirmation dialog before execution.
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
3. **Web GUI & IPC Server (`--web --port 8080`)**: Powered by `axum` ([src/web/mod.rs](src/web/mod.rs)), serving the Rust-SQL web interface and implementing all IPC events from [src/common/event.rs](src/common/event.rs).

---

## Building & Running

```bash
# Run all unit and integration tests
cargo test

# Launch Native Desktop GUI
cargo run -- --gui

# Launch Terminal UI
cargo run -- --tui

# Launch Web UI Server on port 8080
cargo run -- --web --port 8080

# Execute a headless batch query and format as Markdown
cargo run -- --uri "postgres://localhost:5432/rust_sql" -e "SELECT * FROM users" --format markdown

# Generate a Mermaid ER Diagram from the CLI
cargo run -- --uri "postgres://localhost:5432/rust_sql" --erd
```

### Docker

```bash
# Build and run the Web UI container + local test databases
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
