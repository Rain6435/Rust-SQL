# Changelog

All notable changes to **Rust-SQL** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.1.0] - 2026-11-25

### Changed
- **Desktop GUI visual identity & theme system (`src/gui/theme.rs`).** Introduced a dual-mode
  `AppTheme` palette (**Deep Slate Foundry** dark and **Drafting Paper** light) with 1px hairline
  outlines, differentiated corner radii, dialect-coded badges (PostgreSQL, MySQL/MariaDB, SQLite,
  SQL Server, Cassandra), and unified `primary_button` / `danger_button` / `subtle_button` /
  `card_frame` / `badge_pill` primitives across all screens and modals.
- **Header, footer, and server dashboard.** Added dialect & connection-mode (`LIVE` / `EMULATED`)
  status pills, a 1-click `Light`/`Dark` theme toggle in the footer, a `Grid`/`List` view toggle
  in the Connections toolbar, `SSH`/`SSL` badges on server cards, and `Enter`-to-connect when
  filtering narrows to a single server.
- **Editor, sidebar, results, and UX polish.** Wired the `Wrap` toggle to horizontal scrolling,
  added `A-`/`A+` editor font-size controls, a 1-click `Refresh` button in the database explorer,
  an actionable empty state in the results pane, `filtered/total` row counts with a `×` filter clear
  button, `Copy Cell` in the result context menu, and global `Ctrl+O`, Zoom (`Ctrl+=`/`-`/`0`),
  and `Esc`-to-close-modal shortcuts.

## [2.0.0] - 2026-11-18

This release replaces placeholder behavior with real implementations across connections,
credentials, the web server, and query safety. It is a major version because existing
configurations and integrations need changes:

- **Saved passwords must be entered again.** The old AES-256-CBC format is gone with no migration,
  and the OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service) is now
  required to store or read saved credentials.
- **SSH bastion fingerprints must be approved again.** Plaintext host-key pins are discarded.
- **The web server needs an account and HTTPS.** Create an administrator with
  `Rust-SQL users create --admin`; use `--insecure-http` for plain HTTP on loopback.
- **MySQL, MariaDB, SQL Server, and Cassandra fail to connect** unless emulation is turned on
  with `--emulate` or `"emulateUnsupportedDialects": true`; they have no live driver yet.
- **IPC changes:** `DB_CONNECT` takes a saved connection ID and `SERVERS_DECRYPT_SECRETS` was removed.

### Added
- **CSV and JSON import.** Import `.csv`/`.tsv`/`.json`/`.jsonl` rows into an existing table from
  the desktop GUI (`📥 Import`: preview, per-column mapping) or the CLI
  (`--import <FILE> --table <TABLE> [--schema S] [--yes]`). Rows go through prepared statements in
  one transaction; a failing row rolls back everything. 10,000+ rows need explicit confirmation.
- **Per-connection statement timeout** (`statementTimeoutSecs`, "Statement timeout (s)" in the
  connection form). PostgreSQL/Redshift set `statement_timeout`; SQLite interrupts the statement
  and sets `busy_timeout`. Shown in the GUI footer, terminal UI, and web footer.
- **Result paging.** A single `SELECT` loads `resultPageSize` rows (default 1000) at a time, with
  "Load more" in the GUI and web UI and <kbd>m</kbd> in the terminal UI. Web: `DB_EXECUTE_PAGED`.
  Column statistics say when they cover only the rows loaded so far.
- **Autocomplete in the desktop GUI editor**: tables, columns of referenced tables (aliases
  resolved), and keywords, live or on <kbd>Ctrl</kbd>-<kbd>Space</kbd>. The existing Auto Complete
  settings now do something; before, no completion engine existed.
- **Import and autocomplete in the terminal and web UIs**, sharing the desktop code. Terminal UI:
  <kbd>i</kbd> on a table opens an import dialog; suggestions appear under the editor (live or
  <kbd>Ctrl</kbd>-<kbd>Space</kbd>, <kbd>Tab</kbd> inserts). Web UI: `Import…` panel and an
  autocomplete list under the editor (a `<textarea>` now). New IPC events `DB_IMPORT_PREVIEW`,
  `DB_IMPORT_RUN`, `DB_COMPLETE` (grant required); `/api/ipc/import` takes import bodies up to
  16 MiB with the same session and CSRF checks. Imports are audited.
- **Application logo** in the desktop GUI (window icon and header), the web UI (favicon and
  header), and the README.
- **`--tui --read-only`** runs the terminal UI in Read-Only mode (queries and imports).
- **Live database CI job** that starts PostgreSQL, MySQL, and MariaDB and fails if any returns
  emulated results when a live connection was requested.
- **Threat model** in [`docs/security/threat-model.md`](docs/security/threat-model.md): the web
  server's accounts and sessions, the keychain dependency, SSH host-key handling and its 180-day
  timer, what the audit log records and omits, the emulation opt-in, and known gaps.

### Changed
- **SQLite is now built into Rust-SQL** (rusqlite 0.43 with the `bundled` feature, SQLite 3.54)
  instead of linked from the system. Building from source no longer needs `libsqlite3-dev`,
  Homebrew `sqlite3`, or vcpkg, and the Docker image no longer installs `libsqlite3-0`.
- **Query history no longer keeps parameter values by default.** It keeps the query with its
  placeholders; turn on "Keep parameter values in history" (`historyStoreBoundValues`) to also keep
  the display-only copy. A connection marked "Exclude from query history" records nothing.
- **Query parameters are bound through prepared statements instead of pasted into the SQL.**
  - SQLite, the emulator, PostgreSQL, and Redshift bind `:name`, `@name`, and `$1` values through
    the driver. A value containing `'`, `\`, or `--` can no longer change the statement.
  - Placeholders inside `--` and `/* */` comments are no longer substituted.
  - Parameters keep their type (Text, Number, Boolean, NULL) from the GUI, terminal UI, and web
    UI to the server. Before, every value was sent as a string and re-read as a number if it
    looked like one, so a ZIP code like `02134` became `2134`.
  - The terminal UI and web UI can now set parameter values. Web: `DB_EXECUTE_QUERY` and
    `DB_CREATE_CANCELLABLE_QUERY` take an optional JSON object of values.
  - A placeholder with no value is now an error naming it.
  - History stores the query with placeholders; the copy with values filled in is labeled
    "display only, not executed".
- **Read-Only mode and the destructive-query check can no longer be bypassed, and fail closed.**
  - Statements are parsed with `sqlparser` in the connection's dialect instead of scanned for a
    `WHERE` token. `DELETE FROM users -- WHERE id = 1` and
    `UPDATE users SET flag = (SELECT 1 WHERE 1=1)` are now `Critical`; both were `Safe` before.
  - A script that cannot be parsed is `Critical`, needs confirmation, and is blocked in Read-Only
    mode. Before, it was reported as safe and read-only.
  - Read-Only mode runs only statements recognized as read-only. `WITH ... DELETE`, `MERGE`,
    `REPLACE`, `CALL`, `COPY`, `GRANT`, `SET`, `SELECT ... INTO`, and `SELECT ... FOR UPDATE`
    were all allowed before and are now blocked.
  - Read-Only mode also makes the database session read-only (PostgreSQL/Redshift read-only
    transactions, SQLite `SQLITE_OPEN_READ_ONLY` or `query_only`). Switching it on or off reopens
    a SQLite file connection, which drops an open transaction.
- **Saved passwords work again, encrypted with a master key from the OS keychain.**
  - On first use Rust-SQL creates a random 256-bit master key in the macOS Keychain, Windows
    Credential Manager, or Linux Secret Service (service `rust-sql`, one entry per data
    directory). The keychain is required: without it, saved credentials are neither stored nor
    decrypted and the web server refuses to start. Linux without a running Secret Service is
    unsupported.
  - Values are encrypted with XChaCha20-Poly1305 (`{"ciphertext": "v2:..."}`) and bound to their
    connection and field, so a value copied to another connection does not decrypt. Duplicating a
    connection re-encrypts its credentials for the copy.
  - SSH key passphrases are now encrypted at rest like passwords, and a saved passphrase is used
    instead of prompting.
  - **Breaking:** the old AES-256-CBC format and the `encrypted` flag are gone, with no migration.
    A configuration that still holds `{ivText, encryptedText}` values fails to load with an error
    asking you to remove those passwords and enter them again.
- **The web UI is now multi-user, signed-in, and served over HTTPS.**
  - Accounts live in `rust-sql.db` next to the configuration file, with Argon2id password hashes
    (12 characters minimum). Create the first administrator with
    `Rust-SQL users create --admin`.
  - Sessions use a random 256-bit cookie (`HttpOnly; Secure; SameSite=Strict`) that expires after
    12 hours; state-changing requests require an `X-CSRF-Token` header; 5 failed logins per
    username or IP in 15 minutes return 429.
  - The server serves HTTPS with rustls. It refuses to start on a non-loopback address without
    TLS, and serves plain HTTP on loopback only with `--insecure-http`. The bind address
    (`--bind`), `Host` allowlist (421 otherwise), session lifetime, and per-session concurrency
    cap are configurable under `"web"`. Request bodies over 1 MiB get 413.
  - Each user sees only the saved connections an administrator granted them; a request without a
    grant gets 403 and is audited. Managing connections, accounts, grants and settings requires
    an administrator.
  - An audit log records logins, failed logins, sign-outs, connection and grant changes, query
    executions (statement type and row count, not the query text), and SSH host-key approvals
    and deletions from every front end.
  - `SERVERS_DECRYPT_SECRETS` was removed, no IPC event accepts an encryption secret, and no
    response contains a saved password (`hasPassword` and similar flags instead). `DB_CONNECT`
    now takes a saved connection ID.
  - The built-in default encryption secret was removed. Saved passwords are encrypted only with
    the master key from the OS keychain (see above).
- **SSH tunnels now actually use SSH.** Previously the "SSH tunnel" option opened a local port
  that connected straight to the database host, never contacting the bastion.
  - Connections authenticate to the bastion with `russh` (SSH agent, then private key with
    optional passphrase, then password; only the configured method is tried) and forward over a
    `direct-tcpip` channel, so databases reachable only from the bastion now work.
  - Bastion host keys are trusted on first use: the app shows the SHA-256 fingerprint and asks for
    approval, re-confirmation is required every 180 days, and a changed key is refused with no
    in-app override (delete the pin under Settings → "Trusted SSH host keys").
  - New CLI flag `--trust-host-key SHA256:...` approves a known fingerprint in batch mode.
  - SSH passwords, passphrases, key material, and the agent socket path are redacted from errors.
- **Emulated data is no longer presented as real results.**
  - PostgreSQL and Redshift connection failures (wrong password, unreachable host, missing
    database) now return an error naming the host, port, and database, never the password.
    Previously they silently fell back to an in-memory emulator with fake `roles`/`users` tables.
  - Live PostgreSQL and Redshift sessions now run queries and metadata (databases, schemas,
    tables, views, routines, columns, triggers, indexes, keys) against the real server.
    Previously only the database list came from the server.
  - MySQL, MariaDB, SQL Server, and Cassandra have no live driver. Connecting to them now fails
    with a "not implemented" error unless emulation is enabled with `--emulate` or
    `"emulateUnsupportedDialects": true`.
  - Added `ConnectionMode` (`Live` / `Emulated`), exposed through `DatabaseAdapter::connection_mode`,
    `DatabaseConnection::connection_mode`, `DatabaseFacade::connection_mode`, the `DB_CONNECT`
    IPC response, and the new `DB_GET_CONNECTION_MODE` IPC event.
  - The GUI, TUI, web UI, and CLI show "EMULATED — results are not from your server." whenever a
    connection is emulated. "Test connection" reports that nothing was tested for emulated dialects.
  - PostgreSQL connections with SSL enabled are refused instead of being sent unencrypted
    (TLS is not implemented yet).

### Removed
- The `invalid.` / `255.255.255.255` hostname test hook, the simulated-latency sleep in the
  emulator, the fabricated PostgreSQL `EXPLAIN` plan, and placeholder `let _ = (...)` blocks.
- `try_connect_live_postgres` (replaced by `connect_live_postgres`, which returns an error
  instead of `None`).

### Fixed
- A script containing a PostgreSQL dollar-quoted body with non-ASCII text, such as
  `SELECT $$é$$;`, crashed the statement splitter (and with it running, analyzing, and paging the
  script) instead of running.

### Security
- Upgraded `rusqlite` 0.32 to 0.43 and bundled SQLite 3.54.1 to fix RUSTSEC-2026-0459
  (CVE-2026-119375): SQLite before 3.54.0 could return uninitialized memory when opening a
  database file with a crafted `-wal` file. The advisory is no longer ignored in `deny.toml` or the
  audit workflow.
- **SSH host-key pins are encrypted.** The `sshHostKeys` list in `rust-sql.json` is now one
  XChaCha20-Poly1305 value under the OS keychain master key (its own HKDF subkey), so pins can no
  longer be read, edited, or swapped in without the keychain. Without the keychain, SSH tunnels
  are refused instead of connecting unverified. Plaintext pins from earlier builds are discarded
  at startup (no migration); approve each bastion's fingerprint again.

## [1.41.1] - 2026-11-17

### Fixed
- Upgraded `zip` to `9.6` to resolve upstream `zip 2.x` security advisories.
- Fixed Windows `set_executable_permissions` compilation in `src/core/updater/archive.rs`.

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
