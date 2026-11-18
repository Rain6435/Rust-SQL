## Configuration File

Rust-SQL keeps a configuration file in the directory:

- **macOS:** `~/Library/Application Support/Rust-SQL/rust-sql.json`
- **Linux:** (`$XDG_CONFIG_HOME` or `~/.config`) + `/Rust-SQL/rust-sql.json`
- **Windows:** (`$APPDATA`) + `\Rust-SQL\rust-sql.json`

You can also override the configuration directory via the `$RUST_SQL_HOME` environment variable (`$RUST_SQL_HOME/rust-sql.json`) or use `~/.rust-sql.json`.

```json
{
  "zoomFactor": 1,
  "limitQueryDefaultSelectTop": 100,
  "enabledAutoComplete": true,
  "enabledLiveAutoComplete": true,
  "enabledDarkTheme": false,
  "disabledOpenAnimation": false,
  "csvDelimiter": ",",
  "connectionsAsList": false,
  "customFont": "Lato",
  "emulateUnsupportedDialects": false,
  "historyStoreBoundValues": false,
  "resultPageSize": 1000,
  "web": {
    "bindAddress": "127.0.0.1",
    "port": 8080,
    "tlsCertPath": "/etc/rust-sql/fullchain.pem",
    "tlsKeyPath": "/etc/rust-sql/privkey.pem",
    "allowInsecureHttp": false,
    "allowedHosts": ["127.0.0.1:8080"],
    "sessionTtlHours": 12,
    "maxConcurrentRequestsPerSession": 8
  },
  "log": {
    "console": true,
    "file": true,
    "level": "debug",
    "path": "~/.rust-sql.log"
  },
  "servers": [
    {
      "filter": {
        "database": {
          "only": [
            "company"
          ]
        },
        "schema": {
          "ignore": [
            "pg_catalog",
            "pg_temp_1"
          ]
        }
      },
      "id": "651abe80-ed50-44a1-b778-1bdfe97b0bec",
      "name": "rust-sql-localhost",
      "client": "postgresql",
      "statementTimeoutSecs": 30,
      "excludeFromHistory": false
    }
  ]
}
```

### `sshHostKeys`

Trusted SSH bastion host keys, written when you approve a fingerprint. The whole list is
encrypted with the OS keychain master key, the same way as saved passwords, so the file shows
neither the bastions nor their fingerprints:

```json
"sshHostKeys": { "ciphertext": "v2:..." }
```

Decrypted, it holds one entry per bastion `host:port` with the key algorithm, the `SHA256:`
fingerprint, and when you approved it (UTC). A pin older than 180 days must be re-confirmed in the
app before the tunnel connects. A different key for the same `host:port` is refused; remove the
pin (Settings → "Trusted SSH host keys") to accept a rotated key. The value cannot be edited by
hand: any change makes it undecryptable, and SSH tunnels are then refused until you delete the
`sshHostKeys` entry and approve each bastion again. Without the OS keychain, pins can be neither
read nor saved, so SSH tunnels fail rather than connect unverified.

Builds before 2026-11-18 stored this list as plaintext JSON. There is no migration: Rust-SQL
discards a plaintext list when it starts, and asks you to approve each bastion's fingerprint again.

### `emulateUnsupportedDialects`

Default `false`. MySQL, MariaDB, SQL Server, and Cassandra have no live driver yet, so connecting
to them fails with a "not implemented" error. Setting this to `true` (or starting Rust-SQL with
`--emulate`) lets those dialects run on an in-memory emulator instead. Emulated results are **not
from your server**, and every front end labels them "EMULATED — results are not from your
server." This setting has no effect on PostgreSQL, Redshift, or SQLite, which always connect live.

### `historyStoreBoundValues`

Default `false`. Query history keeps each query with its placeholders (`:email`, `$1`) and never
the parameter values. Set to `true` to also keep a display-only copy with the values filled in.

### `resultPageSize`

Default `1000`. Rows fetched per page when a query is a single `SELECT`; more rows load on demand
("Load more" in the GUI and web UI, <kbd>m</kbd> in the terminal UI).

### Per-connection: `statementTimeoutSecs` and `excludeFromHistory`

`statementTimeoutSecs` stops any statement that runs longer than this many seconds (absent or `0`
means no timeout). PostgreSQL and Redshift use the server's `statement_timeout`; SQLite interrupts
the statement and uses `busy_timeout` for lock waits. `excludeFromHistory: true` keeps every query
run on that connection out of query history.

### `web`

Settings for the multi-user web UI; see [web-server.md](web-server.md). Every field is optional.
`--port` and `--bind` override `port` and `bindAddress`. A non-loopback `bindAddress` requires
both `tlsCertPath` and `tlsKeyPath`. `allowInsecureHttp` (or `--insecure-http`) permits plain
HTTP on loopback for development only. Web accounts, sessions, grants and the audit log are
stored in `rust-sql.db` next to this file, never in it.
