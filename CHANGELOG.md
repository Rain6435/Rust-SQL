# Changelog

All notable changes to **Rust-SQL** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.39.0] - 2026-11-17

### Added
- **Complete Rust Port of Sqlectron (`v1.39.0`)**:
  - Full database adapter layer (`src/adapters/`) supporting PostgreSQL (`postgresql`), Amazon Redshift (`redshift`), MySQL (`mysql`), MariaDB (`mariadb`), Microsoft SQL Server (`sqlserver`), SQLite (`sqlite`), and Apache Cassandra (`cassandra`).
  - Bit-compatible AES-256-CBC password encryption/decryption (`src/core/crypto.rs`) and `sqlectron.json` configuration compatibility (`src/core/config.rs`, `src/core/servers.rs`, `src/core/validators.rs`).
  - Complete 14-slice Redux store equivalent (`src/state/mod.rs`: `config`, `servers`, `connections`, `databases`, `schemas`, `tables`, `columns`, `triggers`, `indexes`, `views`, `routines`, `queries`, `sqlscripts`, `status`).
  - **Three User Interfaces**:
    - Native Desktop GUI (`--gui`) powered by `eframe` / `egui` (`src/gui/mod.rs`).
    - Terminal UI (`--tui` / `--term`) powered by `ratatui` + `crossterm` (`src/tui/mod.rs`), cloning `sqlectron-term`.
    - Web GUI & Electron-compatible IPC server (`--web`) powered by `axum` (`src/web/mod.rs`).
- **Modern Developer Features**:
  - **Read-Only Mode & Destructive Query Safety Analyzer (`src/core/safety.rs`)**: Blocks mutations in Read-Only mode and flags `DELETE`/`UPDATE` without `WHERE`, `DROP`, and `TRUNCATE` statements.
  - **Parameterized Queries (`src/core/params.rs`)**: Automatic extraction and safe literal binding for `:named`, `@named`, and `$1` placeholders.
  - **SQL Bookmarks & Execution Audit History (`src/core/bookmarks.rs`)**: Reusable tagged SQL snippet library and searchable execution audit trail.
  - **Result Filtering, Multi-Column Sorting & Column Data Profiler (`src/core/result_ops.rs`)**: Instant row filtering, clickable column sorting, and per-column summary statistics (`total`, `nulls`, `distinct`, `min`, `max`, `sum`, `mean`).
  - **Schema Mermaid ER Diagram Generator (`src/core/result_ops.rs`)**: Automated `erDiagram` generation from database tables, columns, primary keys, and foreign keys (`--erd` and GUI modal).
  - **Extended Multi-Format Data Export (`src/core/export.rs`)**: Added `MARKDOWN` (GitHub-flavored tables), `SQL` (`INSERT INTO` statements), and `JSONL` (`NDJSON`) exports alongside `CSV` and `JSON`.
  - **Headless CLI Batch Execution (`src/main.rs`)**: Added `-e / --exec`, `--uri`, `--server`, `--format`, `--read-only`, and `--erd` flags.
