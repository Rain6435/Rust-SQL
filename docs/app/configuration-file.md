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

### `emulateUnsupportedDialects`

Default `false`. MySQL, MariaDB, SQL Server, and Cassandra have no live driver yet, so connecting
to them fails with a "not implemented" error. Setting this to `true` (or starting Rust-SQL with
`--emulate`) lets those dialects run on an in-memory emulator instead. Emulated results are **not
from your server**, and every front end labels them "EMULATED — results are not from your
server." This setting has no effect on PostgreSQL, Redshift, or SQLite, which always connect live.
