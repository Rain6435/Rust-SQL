# Contributing to Rust-SQL

First off, thank you for considering contributing to **Rust-SQL**! It's people like you that make open-source software a great community to be part of.

---

## Code of Conduct

This project and everyone participating in it is governed by our [Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code.

---

## Architecture Overview

`Rust-SQL` is organized into clean, modular layers:

- **`src/common/`**: Shared data types (`Server`, `Config`, `QueryRowResult`, `AdapterInfo`), IPC event constants (`event.rs`), and string/version utilities (`utils.rs`).
- **`src/core/`**: Core business logic:
  - `crypto.rs`: AES-256-CBC password encryption/decryption for `rust-sql.json`.
  - `config.rs` & `servers.rs` & `validators.rs`: Configuration file lifecycle, server CRUD, and validation.
  - `sql_identifier.rs` & `sql_formatter.rs`: Multi-statement SQL parser/classifier and dialect-aware query formatter.
  - `safety.rs`: Read-only guardrail and destructive query analyzer (`DELETE`/`UPDATE` without `WHERE`, `DROP`, `TRUNCATE`).
  - `params.rs`: Named (`:param`, `@param`) and positional (`$1`) query parameter extraction and safe literal binding.
  - `bookmarks.rs`: SQL bookmark library and searchable execution history log.
  - `result_ops.rs`: Client-side result row filtering, multi-column sorting, column summary statistics profiler, and Mermaid ER diagram generator.
  - `export.rs`: Multi-format result exporter (`CSV`, `JSON`, `JSONL`, `MARKDOWN`, `SQL INSERT`).
  - `tunnel.rs`: SSH local port-forwarding tunnel.
- **`src/adapters/`**: Database adapters (`sqlite.rs`, `dialect_adapters.rs`, `facade.rs`) supporting PostgreSQL, Redshift, MySQL, MariaDB, Microsoft SQL Server, SQLite, and Apache Cassandra.
- **`src/state/`**: State machine (`AppStore`) managing all 14 application slices.
- **`src/gui/`**: Native Desktop GUI (`eframe` / `egui`).
- **`src/tui/`**: Terminal UI (`ratatui` + `crossterm`).
- **`src/web/`**: Web UI and IPC server (`axum`).

---

## Prerequisites

- **Rust**: Stable toolchain (`1.80+`, recommended via [rustup](https://rustup.rs/)).
- **System Libraries (Linux)**:
  ```bash
  sudo apt-get update && sudo apt-get install -y \
    pkg-config libssl-dev libsqlite3-dev \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libgtk-3-dev \
    libgl1-mesa-dev libegl1-mesa-dev libfontconfig1-dev libfreetype-dev
  ```

---

## Development Workflow

1. **Fork and Clone the Repository**:
   ```bash
   git clone https://github.com/<your-username>/Rust-SQL.git
   cd Rust-SQL
   ```

2. **Run the Test Suite**:
   ```bash
   cargo test
   ```

3. **Run in Different Modes**:
   ```bash
   # Native Desktop GUI
   cargo run -- --gui

   # Terminal UI
   cargo run -- --tui

   # Web UI & IPC Server
   cargo run -- --web --port 8080

   # Headless CLI Batch Execution
   cargo run -- -e "SELECT sqlite_version();" --format markdown
   ```

4. **Formatting, Linting & Git Hooks**:
   Install the repository's commit and pre-push hooks so formatting, modularization (`<= 275` lines/file), Conventional Commits, Clippy, tests, and Rustdoc checks run automatically:
   ```bash
   make install-hooks
   # or: git config core.hooksPath .githooks
   ```
   You can also run the checks manually before opening a Pull Request:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```

---

## Submitting a Pull Request

1. Create a descriptive feature branch (`git checkout -b feat/my-feature` or `fix/issue-description`).
2. Write unit or integration tests in `src/` or `tests/integration_suite.rs` covering your changes.
3. Update `CHANGELOG.md` under the `[Unreleased]` section if the change is user-facing.
4. Open a Pull Request and complete the PR checklist.
