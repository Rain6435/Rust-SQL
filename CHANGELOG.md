# Changelog

All notable changes to **Rust-SQL** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.41.0] - 2026-11-17

### Added
- **Built-in Self-Updater (`src/core/updater/`)**:
  - Added `--check-update` and `--update` CLI flags (`Rust-SQL --check-update`, `Rust-SQL --update`) and a `Help -> Check for Updates` modal in the Desktop GUI.
  - Downloads platform-matched GitHub Release archives (`.tar.gz` / `.zip`), verifies `.sha256` checksums before extraction, and atomically replaces the running executable via `self-replace`.

### Fixed
- Switched `x86_64-apple-darwin` release builds from retired `macos-13` to `macos-latest` cross-compilation in `.github/workflows/release.yml`.

## [1.40.0] - 2026-11-17

### Changed
- **Full Codebase Modularization & Rustdoc Documentation**:
  - Modularized all 84 `.rs` files across `src/` and `tests/` to a strict maximum of 275 lines per file with full module-level (`//!`) and item-level (`///`) Rustdoc documentation.
  - Added automated test enforcement (`test_codebase_modularization_and_documentation_invariant`) in `tests/store_ipc_tui_suite.rs`.
- **Developer Experience & CI/CD**:
  - Added version-controlled `.githooks/` (`pre-commit`, `commit-msg`, `pre-push`) with `make install-hooks`.
  - Fixed Windows `sqlite3.lib` linking in CI and Release workflows, and updated GitHub Actions & Cargo dependencies (`eframe`/`egui` 0.37, `ratatui` 0.30, `crossterm` 0.29, `axum` 0.8, `aes` 0.9, `cbc` 0.2, `rand` 0.10, `dirs` 7.0, `thiserror` 2.0).

## [1.39.0] - 2026-11-17

### Added
- **Cross-Database Core Engine**:
  - Full database adapter layer (`src/adapters/`) supporting PostgreSQL (`postgresql`), Amazon Redshift (`redshift`), MySQL (`mysql`), MariaDB (`mariadb`), Microsoft SQL Server (`sqlserver`), SQLite (`sqlite`), and Apache Cassandra (`cassandra`).
  - AES-256-CBC password encryption/decryption (`src/core/crypto.rs`) and `rust-sql.json` configuration management (`src/core/config.rs`, `src/core/servers.rs`, `src/core/validators.rs`).
  - Complete 14-slice state store (`src/state/mod.rs`: `config`, `servers`, `connections`, `databases`, `schemas`, `tables`, `columns`, `triggers`, `indexes`, `views`, `routines`, `queries`, `sqlscripts`, `status`).
  - **Three User Interfaces**:
    - Native Desktop GUI (`--gui`) powered by `eframe` / `egui` (`src/gui/mod.rs`).
    - Terminal UI (`--tui` / `--term`) powered by `ratatui` + `crossterm` (`src/tui/mod.rs`).
    - Web GUI & IPC server (`--web`) powered by `axum` (`src/web/mod.rs`).
- **Modern Developer Features**:
  - **Read-Only Mode & Destructive Query Safety Analyzer (`src/core/safety.rs`)**: Blocks mutations in Read-Only mode and flags `DELETE`/`UPDATE` without `WHERE`, `DROP`, and `TRUNCATE` statements.
  - **Parameterized Queries (`src/core/params.rs`)**: Automatic extraction and safe literal binding for `:named`, `@named`, and `$1` placeholders.
  - **SQL Bookmarks & Execution Audit History (`src/core/bookmarks.rs`)**: Reusable tagged SQL snippet library and searchable execution audit trail.
  - **Result Filtering, Multi-Column Sorting & Column Data Profiler (`src/core/result_ops.rs`)**: Instant row filtering, clickable column sorting, and per-column summary statistics (`total`, `nulls`, `distinct`, `min`, `max`, `sum`, `mean`).
  - **Schema Mermaid ER Diagram Generator (`src/core/result_ops.rs`)**: Automated `erDiagram` generation from database tables, columns, primary keys, and foreign keys (`--erd` and GUI modal).
  - **Extended Multi-Format Data Export (`src/core/export.rs`)**: Added `MARKDOWN` (GitHub-flavored tables), `SQL` (`INSERT INTO` statements), and `JSONL` (`NDJSON`) exports alongside `CSV` and `JSON`.
  - **Headless CLI Batch Execution (`src/main.rs`)**: Added `-e / --exec`, `--uri`, `--server`, `--format`, `--read-only`, and `--erd` flags.
