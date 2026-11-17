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
