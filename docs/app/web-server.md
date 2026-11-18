## Multi-user Web UI (`--web`)

The web UI is a shareable, multi-user server: several people sign in to one running instance
from different machines. Each person sees only the saved connections an administrator has
granted them.

### First start

1. Create the first administrator. The password (at least 12 characters) is read from the
   terminal without echo, or from one line of standard input with `--password-stdin`:

   ```bash
   Rust-SQL users create --admin --username alice
   Rust-SQL users list
   ```

2. Configure TLS in `rust-sql.json` (see [configuration-file.md](configuration-file.md)):

   ```json
   "web": {
     "bindAddress": "0.0.0.0",
     "port": 8443,
     "tlsCertPath": "/etc/rust-sql/fullchain.pem",
     "tlsKeyPath": "/etc/rust-sql/privkey.pem",
     "allowedHosts": ["sql.example.com"]
   }
   ```

3. Start it with `Rust-SQL --web`. The server refuses to start, and says why, when:
   - the bind address is not loopback and TLS is not configured;
   - TLS is not configured and plain HTTP was not explicitly enabled for development
     (`--insecure-http` or `"allowInsecureHttp": true`, loopback only);
   - no account exists yet.

For local development only: `Rust-SQL --web --insecure-http` serves plain HTTP on
`127.0.0.1:8080`.

### Security model

| Area | Behavior |
| --- | --- |
| Accounts | Stored in the application database (`rust-sql.db`, next to `rust-sql.json`). Passwords are hashed with Argon2id; the PHC hash string carries its salt and parameters. Unknown users and wrong passwords get the same answer. |
| Sessions | A random 256-bit ID in a cookie marked `HttpOnly; Secure; SameSite=Strict; Path=/`. Only its SHA-256 hash is stored. Sessions expire after 12 hours (`web.sessionTtlHours`) and end on sign-out, password change, account deletion, or `USERS_REVOKE_SESSIONS`. |
| CSRF | Every `POST` except `/api/login` must send the session's token in `X-CSRF-Token`; otherwise 403. |
| Login rate limit | 5 failed attempts per username and per source IP in 15 minutes, then 429 with `Retry-After`. Kept in memory, so a restart resets it. |
| Host header | Must match `web.allowedHosts` (default: the bind address and port, plus `localhost:<port>` on loopback); otherwise 421. A missing port means 443 with TLS and 80 without. |
| Query parameters | `DB_EXECUTE_QUERY` (args: database, SQL, parameters) and `DB_CREATE_CANCELLABLE_QUERY` (database, query ID, SQL, parameters) take an optional JSON object such as `{"id": 5, "email": "a@example.com"}`. JSON types are kept and the values are bound through prepared statements. |
| Paging | `DB_EXECUTE_PAGED` (args: database, SQL, parameters, cursor) returns `{ results, page }`. For a single `SELECT`, `results` holds one page and `page.next` is the cursor (`{ "offset": 1000, "pageSize": 1000 }`) to send for the next page, or `null` at the end. Other scripts run in full with `page: null`. Page size is capped at 10,000. Each call is grant-checked and audited like `DB_EXECUTE_QUERY`. |
| Limits | Request bodies over 1 MiB get 413. Each session may run 8 IPC requests at once (`web.maxConcurrentRequestsPerSession`); more get 429. |
| Authorization | `connection_grants` maps users to saved connections. Every event that touches a connection re-checks the grant; without one the answer is 403 and the attempt is audited. Administrators can use every connection. |
| Admin-only | Accounts and grants (`USERS_*`, `GRANTS_*`), the audit log (`AUDIT_LIST`), creating, changing and deleting saved connections (`SERVERS_ADD`/`UPDATE`/`ADD_OR_UPDATE`/`REMOVE_BY_ID`), settings (`CONFIG_SAVE*`, `CONFIG_PREPARE`), deleting SSH host-key pins, and events that read or write the server's file system or change process-wide limits (`DB_QUERY_OPEN`, `DB_QUERY_SAVE`, `DB_EXPORT_QUERY_RESULT_TO_FILE`, `DB_SET_SELECT_LIMIT`). |
| Credentials | Clients never send or receive encryption keys. `DB_CONNECT` takes a saved connection ID and the server decrypts the credentials itself. Plaintext passwords are sent only when creating or updating a connection; responses report `hasPassword`, `sshHasPassword`, `sshHasPrivateKey`, `sshHasPassphrase`, and `sslHasKey` instead of values. On update, an omitted password keeps the saved one and an empty string clears it. |
| Isolation | Each session has its own database connection; one user's open connection is never visible to another session. |

### Audit log

The `audit_events` table records logins, failed and rate-limited logins, sign-outs, connection
creation, changes and deletion, grant changes, account changes, refused requests, every
statement execution, and SSH host-key approvals and deletions (from every front end, with the
desktop GUI, TUI and CLI recorded as `local`). Statement rows hold the connection ID, user ID,
time, statement type, and row count. The query text is not recorded, and no row contains a
password, session ID, or CSRF token. Administrators read it with `AUDIT_LIST`.

### Encryption key

The server reads the master key for saved passwords from the OS keychain at startup (creating it
on first run) and never accepts one from a client. If the keychain is unavailable or access is
denied, the server refuses to start and prints why. On Linux this requires a running Secret
Service in the session that starts the server; a headless Linux host without one is not
supported. See [Saved passwords and the OS keychain](../../README.md#saved-passwords-and-the-os-keychain).
