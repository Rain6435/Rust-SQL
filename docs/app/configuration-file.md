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
      "client": "postgresql"
    }
  ]
}
```

### `sshHostKeys`

Trusted SSH bastion host keys, written when you approve a fingerprint. One entry per bastion:

```json
"sshHostKeys": [
  {
    "host": "bastion.example.com",
    "port": 22,
    "keyAlgorithm": "ssh-ed25519",
    "fingerprintSha256": "SHA256:BCOriMCRjlJk88UWPoBgqazxhQtK7SfD3idd6RvyZTQ",
    "approvedAt": "2026-11-17T18:30:00Z"
  }
]
```

`approvedAt` is UTC. A pin older than 180 days must be re-confirmed in the app before the
tunnel connects. A different key for the same `host:port` is refused; remove the entry (Settings →
"Trusted SSH host keys") to accept a rotated key. Editing a fingerprint by hand does not bypass the
check: a mismatch is always refused.

### `emulateUnsupportedDialects`

Default `false`. MySQL, MariaDB, SQL Server, and Cassandra have no live driver yet, so connecting
to them fails with a "not implemented" error. Setting this to `true` (or starting Rust-SQL with
`--emulate`) lets those dialects run on an in-memory emulator instead. Emulated results are **not
from your server**, and every front end labels them "EMULATED — results are not from your
server." This setting has no effect on PostgreSQL, Redshift, or SQLite, which always connect live.

### `web`

Settings for the multi-user web UI; see [web-server.md](web-server.md). Every field is optional.
`--port` and `--bind` override `port` and `bindAddress`. A non-loopback `bindAddress` requires
both `tlsCertPath` and `tlsKeyPath`. `allowInsecureHttp` (or `--insecure-http`) permits plain
HTTP on loopback for development only. Web accounts, sessions, grants and the audit log are
stored in `rust-sql.db` next to this file, never in it.
