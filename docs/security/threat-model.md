# Rust-SQL threat model

This document describes what Rust-SQL protects, from whom, how, and where the protection stops.
It covers the shareable web server, the OS keychain dependency, SSH host-key handling, the audit
log, and the emulation opt-in. It describes the code as of the 1.42 development cycle; when the
code and this document disagree, the code wins and this document is out of date.

## Assets

| Asset | Where it lives |
| --- | --- |
| Database and SSH credentials (passwords, key passphrases) | `rust-sql.json`, encrypted (`v2:` XChaCha20-Poly1305) |
| The master key that encrypts them | OS keychain, service `rust-sql`, account `master-key:<data dir>` |
| Web accounts, session hashes, connection grants, audit log | `rust-sql.db` (SQLite) next to `rust-sql.json` |
| SSH host-key pins | `sshHostKeys` in `rust-sql.json`, the whole list encrypted as one `v2:` value with its own subkey |
| Data inside the user's databases | The databases themselves; Rust-SQL holds result pages in memory only |
| Query text | Query history in the desktop GUI and terminal UI, in memory for the life of the process |

## Actors and trust boundaries

| Actor | Trusted for | Not trusted for |
| --- | --- | --- |
| Operating-system user running Rust-SQL | Everything on that machine: the config file, the app database, the keychain session | n/a. Rust-SQL does not defend against its own OS user. |
| Web administrator (`is_admin`) | Accounts, grants, saved connections, settings, host-key pin deletion, the audit log | Reading saved passwords: no IPC response returns them, admins included |
| Web user | Running statements on the connections they were granted | Any connection without a grant; any admin event |
| Anyone who can reach the web port | Nothing | Everything; they get 401 without a session |
| The network between browser and server | Nothing | Confidentiality and integrity; TLS is required off loopback |
| The network between Rust-SQL and an SSH bastion | Nothing | Bastion identity; checked against the pinned host key |
| A database server | The results it returns | Rust-SQL never treats server output as code |

The main trust boundary is the web server's HTTP interface. The desktop GUI, terminal UI, and
CLI run in-process with the privileges of the OS user and have no authentication of their own.

## Web server

Configuration and behavior are documented in [web-server.md](../app/web-server.md). The threats
it is designed against, and the control for each:

| Threat | Control |
| --- | --- |
| Password guessing | Argon2id hashes (12-character minimum); 5 failed logins per username and per source IP in 15 minutes, then 429. Unknown user and wrong password give the same answer, and the comparison is constant-time. |
| Session theft from storage | Only the SHA-256 of the 256-bit session ID is stored, so reading `rust-sql.db` does not yield a usable cookie. |
| Session theft in transit or by script | Cookie is `Secure; HttpOnly; SameSite=Strict`; the server refuses to start off loopback without TLS. |
| Long-lived sessions | 12-hour expiry (configurable); sign-out, password change, account deletion, and `USERS_REVOKE_SESSIONS` revoke them. |
| Cross-site request forgery | Per-session `X-CSRF-Token` header on every state-changing request, in addition to `SameSite=Strict`. |
| DNS rebinding | `Host` header allowlist; anything else gets 421. |
| One user reaching another's connection | `connection_grants`, re-checked on every event that touches a connection; each session has its own database connection. Refusals are audited. |
| Credential disclosure through the API | No IPC event accepts a key or returns a saved password; responses carry `hasPassword`-style flags. A test scans every response type. |
| Resource exhaustion by an authenticated user | 1 MiB body limit (413); per-session cap on concurrent requests (429); result paging caps pages at 10,000 rows. |

What it does **not** protect against:

- **A malicious administrator.** Admins can create connections pointing anywhere, grant
  themselves anything, delete host-key pins, and read the whole audit log.
- **A malicious user with a grant.** A grant gives the database privileges of the saved
  connection's account. Read-only mode is a guardrail in the GUI and CLI; the web UI has no
  read-only mode, so restrict users with database-side permissions.
- **Rate-limit persistence.** Login counters are in memory and reset on restart. There is no
  global (cross-username, cross-IP) limit, so a distributed attack is slowed only by Argon2id.
- **Compromise of the host.** Anyone who can read the process memory, the TLS private key, or
  write the binary or `rust-sql.db` is outside this model.
- **Denial of service at the network level.** There is no connection limit before
  authentication beyond what the OS and `axum` provide.

## OS keychain dependency

The master key is a random 256-bit value stored in the macOS Keychain, Windows Credential
Manager, or the Linux Secret Service. Saved credentials are encrypted with a subkey derived from
it by HKDF-SHA256 and bound to their connection ID and field name as associated data.

- **No fallback, by design.** If the keychain is unavailable, locked, or denies access, Rust-SQL
  refuses to store or decrypt credentials and the web server refuses to start. There is no
  default key, no file-based key, and no plaintext mode. Linux without a running Secret Service
  (typical for headless servers and containers) is unsupported.
- **Protects:** a copied `rust-sql.json` (backup, sync folder, support bundle) without the
  keychain; swapping a ciphertext between connections or fields; silent tampering (AEAD).
- **Separate subkeys.** Saved credentials and the SSH host-key pin list use different HKDF info
  strings and associated-data domains, so a ciphertext from one cannot be passed off as the
  other.
- **Does not protect:** against malware running as the same OS user, which can usually ask the
  keychain for the key; on Linux, any process in the unlocked Secret Service session can read it.
- **Loss of the key.** Deleting the keychain entry makes saved passwords unrecoverable. Rust-SQL
  never overwrites an unusable entry; the user must re-enter the passwords.

## SSH host-key handling

SSH tunnels authenticate to the bastion with `russh` and verify its host key by trust on first
use with a 180-day re-confirmation timer.

| State | Behavior |
| --- | --- |
| Unknown host:port | Refuse with `HostKeyUnknown`; the UI shows the SHA-256 fingerprint and asks for approval. On approval the pin is stored and the connection retried. |
| Pin matches, approved ≤ 180 days ago | Connect. |
| Pin matches, approved > 180 days ago | Refuse with `HostKeyReconfirmationRequired`; on confirmation `approved_at` resets. |
| Fingerprint differs from the pin | Refuse with `HostKeyChanged`. No in-app override: the pin must be deleted in Settings, and the deletion is audited. |

- **Protects:** a bastion impersonated after the first approval (DNS hijack, ARP spoofing, a
  rerouted IP) is refused, so database credentials are not sent through it.
- **First-use risk.** The first connection trusts whatever key is presented if the user approves
  it. Users should compare the fingerprint with one obtained out of band (`ssh-keyscan` run on
  the bastion, or its administrator). `--trust-host-key SHA256:...` lets batch jobs pin a known
  fingerprint instead of trusting the first one seen.
- **Why the timer.** Re-confirmation makes stale approvals visible and gives a periodic prompt to
  re-check the fingerprint. It does not detect anything by itself: a re-confirmation is only as
  good as the user's comparison.
- **Pin storage.** The pin list (host, port, key algorithm, fingerprint, approval time) is
  serialized and encrypted as one XChaCha20-Poly1305 value under a subkey of the keychain master
  key, bound to the record `sshHostKeys` / field `pins`. Someone who can write `rust-sql.json`
  but not read the keychain cannot add, edit, or swap a pin or reset its approval time, and a
  copy of the file does not reveal which bastions are trusted. If the value fails to decrypt
  (tampering, another master key) or the keychain is unavailable, tunnels are refused with an
  error; they never fall back to unverified connections.
- **What encryption does not stop.** Deleting `sshHostKeys` returns every bastion to the
  first-use prompt, so the user must again compare fingerprints. Restoring an older copy of the
  value (rollback) brings back the pins and approval times it held, including a pin deleted
  since; there is no version counter. Malware running as the OS user can read the keychain and
  is out of scope, as for saved passwords.
- **No migration.** Plaintext pin lists written by earlier builds are discarded at startup, and
  the user approves each bastion again.
- **Legitimate key rotation** looks exactly like an attack (`HostKeyChanged`); the user deletes
  the pin, which is audited, and approves the new key.
- Passwords, passphrases, private key material, and the agent socket path are redacted from
  errors and logs. Only the configured authentication method is tried.

## Audit log

The `audit_events` table in `rust-sql.db` records logins, failed and rate-limited logins,
sign-outs, connection creation, changes and deletion, grant and account changes, session
revocations, refused requests, every statement execution, and SSH host-key approvals and
deletions. Rows carry the time, event type, outcome, user ID and name, source IP, connection ID,
and for statements the statement type and row count. The desktop GUI, terminal UI, and CLI are
recorded as the `local` actor.

What it deliberately omits:

- **Query text and parameter values**, which can contain personal data. The statement type and
  row count are kept instead.
- **Passwords, session IDs, CSRF tokens, and key material**, in any event.

What it does not provide:

- **Tamper evidence.** It is an ordinary SQLite table. Anyone who can write `rust-sql.db`
  (the OS user, or root) can alter or delete rows. Ship it to an external log store if you need
  an immutable record.
- **Retention or rotation.** Rows accumulate until removed by hand.
- **Settings changes** (`CONFIG_SAVE*`) other than connections, grants, accounts, and host-key
  pins are not recorded.
- **Why a statement failed.** A failed execution is recorded as a `failure` row with the
  connection and user, but not the database's error message.
- **Local query history.** The desktop GUI and terminal UI keep a query history in memory (see
  `historyStoreBoundValues` and `excludeFromHistory`); it is separate from the audit log and is
  not written to disk.

## Emulation opt-in

MySQL, MariaDB, SQL Server, and Cassandra have no live driver. With `--emulate` or
`"emulateUnsupportedDialects": true`, connections to them run against an in-memory SQLite
database seeded with sample objects instead of failing.

- **Threat:** a user mistakes emulated output for real data and acts on it (for example, decides
  a table is empty, or that a migration ran).
- **Controls:** emulation is off by default and never used for PostgreSQL, Redshift, or SQLite;
  a PostgreSQL or Redshift connection failure is always an error, never a fallback. While a
  connection is emulated, every front end shows "EMULATED — results are not from your server.",
  `ConnectionMode::Emulated` is exposed through the adapter, facade, and `DB_CONNECT`, and "Test
  connection" reports that nothing was tested. The CI live-database job fails if an emulated
  result is returned when a live connection was requested.
- **Residual risk:** the label can be ignored, and output copied out of the app (exports,
  clipboard) does not carry it.

## SQLite files opened by the user

Rust-SQL opens SQLite database files the user picks, which may come from someone else. It builds
SQLite from the source bundled with `libsqlite3-sys` (3.54 or later, which fixes
CVE-2026-119375 / RUSTSEC-2026-0459) instead of linking the system library, so the version in
use does not depend on the operating system. A test fails if the linked SQLite is older than
3.54.0.

## Known gaps

- **SSH host-key pins can be rolled back or deleted** by anyone who can write `rust-sql.json`
  (see [SSH host-key handling](#ssh-host-key-handling)). They are encrypted and authenticated, so
  they cannot be forged or edited.
- **PostgreSQL TLS is not implemented.** Connections with SSL enabled are refused rather than
  sent unencrypted; use an SSH tunnel to reach a server over an untrusted network.
- **Read-only mode exists only in the desktop GUI and CLI**, not in the terminal UI or the web UI.
- **Unmaintained transitive dependencies** reported by `cargo audit` (informational, no fix
  available): `zvariant`, `zvariant_derive`, and `zbus_names` (through `keyring`'s Secret Service
  backend, until `secret-service` ships on `zbus` 6), and `chrono` (pulled in by `russh-util`
  only for `wasm32` targets, so it is not compiled into Rust-SQL).
