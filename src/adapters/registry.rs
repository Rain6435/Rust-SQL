//! Built-in database client registry (`CLIENTS` / `ADAPTERS`), custom adapter registration,
//! dialect-specific identifier quoting, and `SELECT TOP / LIMIT` query builders.

use crate::common::types::AdapterInfo;
use crate::core::errors::DbCoreError;
use std::sync::{LazyLock, Mutex, OnceLock};

/// Static specification tuple for a built-in database adapter:
/// `(key, name, protocol, default_port, default_database, disabled_features)`.
pub type BuiltinAdapterSpec = (
    &'static str,
    &'static str,
    &'static str,
    Option<u32>,
    Option<&'static str>,
    &'static [&'static str],
);

/// Static specification table for the 7 built-in database adapters.
pub static BUILTIN_ADAPTERS: &[BuiltinAdapterSpec] = &[
    (
        "mysql",
        "MySQL",
        "mysql",
        Some(3306),
        None,
        &["server:schema", "server:domain"],
    ),
    (
        "mariadb",
        "MariaDB",
        "mariadb",
        Some(3306),
        None,
        &["server:schema", "server:domain"],
    ),
    (
        "postgresql",
        "PostgreSQL",
        "postgres",
        Some(5432),
        Some("postgres"),
        &["server:domain"],
    ),
    (
        "redshift",
        "Redshift",
        "redshift",
        Some(5432),
        Some("postgres"),
        &["server:domain"],
    ),
    (
        "sqlserver",
        "Microsoft SQL Server",
        "mssql",
        Some(1433),
        None,
        &[],
    ),
    (
        "sqlite",
        "SQLite",
        "file",
        None,
        Some(":memory:"),
        &[
            "server:ssl",
            "server:host",
            "server:port",
            "server:socketPath",
            "server:user",
            "server:password",
            "server:schema",
            "server:domain",
            "server:ssh",
            "cancelQuery",
        ],
    ),
    (
        "cassandra",
        "Cassandra",
        "cassandra",
        Some(9042),
        None,
        &[
            "server:ssl",
            "server:socketPath",
            "server:schema",
            "server:domain",
            "scriptCreateTable",
            "cancelQuery",
        ],
    ),
];

/// List of built-in database client descriptors (`CLIENTS`).
pub static CLIENTS: LazyLock<Vec<AdapterInfo>> = LazyLock::new(|| {
    BUILTIN_ADAPTERS
        .iter()
        .map(
            |(key, name, protocol, default_port, default_database, disabled)| AdapterInfo {
                key: (*key).to_string(),
                name: (*name).to_string(),
                protocol: (*protocol).to_string(),
                default_port: *default_port,
                default_database: default_database.map(|s| s.to_string()),
                disabled_features: disabled.iter().map(|s| (*s).to_string()).collect(),
            },
        )
        .collect()
});

/// Alias for [`CLIENTS`] providing the registered adapter list (`ADAPTERS`).
pub static ADAPTERS: LazyLock<Vec<AdapterInfo>> = LazyLock::new(|| CLIENTS.clone());

static CUSTOM_ADAPTERS: OnceLock<Mutex<Vec<AdapterInfo>>> = OnceLock::new();

fn custom_adapters_store() -> &'static Mutex<Vec<AdapterInfo>> {
    CUSTOM_ADAPTERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Returns all registered database adapters (built-in [`CLIENTS`] plus any custom adapters).
pub fn get_adapters() -> Vec<AdapterInfo> {
    let mut all = CLIENTS.clone();
    if let Ok(guard) = custom_adapters_store().lock() {
        all.extend(guard.iter().cloned());
    }
    all
}

/// Registers a new custom [`AdapterInfo`] if its `key` is not already taken.
pub fn register_adapter(adapter: AdapterInfo) -> Result<(), DbCoreError> {
    let existing = get_adapters();
    if existing.iter().any(|a| a.key == adapter.key) {
        return Err(DbCoreError::AdapterAlreadyRegistered);
    }
    if let Ok(mut guard) = custom_adapters_store().lock() {
        guard.push(adapter);
    }
    Ok(())
}

/// Quotes a SQL identifier (`value`) using the syntax required by `adapter_key`:
/// - `mysql` / `mariadb`: backticks (`` `ident` ``)
/// - `sqlserver`: square brackets (`[ident]`)
/// - `postgresql` / `redshift` / `sqlite` / `cassandra`: double quotes (`"ident"`), preserving array subscripts (`"col"[0]`)
pub fn wrap_identifier_for_adapter(adapter_key: &str, value: &str) -> String {
    if value == "*" {
        return "*".to_string();
    }
    match adapter_key {
        "mysql" | "mariadb" => format!("`{}`", value.replace('`', "``")),
        "sqlserver" => format!("[{value}]"),
        _ => {
            if value.len() >= 3 && value.ends_with(']') {
                let bytes = value.as_bytes();
                let n = bytes.len();
                if bytes[n - 3] == b'[' && bytes[n - 2].is_ascii_digit() {
                    let prefix = &value[..n - 3];
                    let suffix = &value[n - 3..];
                    return format!(
                        "{}{}",
                        wrap_identifier_for_adapter(adapter_key, prefix),
                        suffix
                    );
                }
            }
            format!("\"{}\"", value.replace('"', "\"\""))
        }
    }
}

/// Builds a dialect-specific `SELECT TOP / LIMIT` query string for `table`.
pub fn build_query_select_top(
    adapter_key: &str,
    table: &str,
    limit: u32,
    schema: Option<&str>,
) -> String {
    match adapter_key {
        "postgresql" | "redshift" => {
            let s = schema.filter(|s| !s.is_empty()).unwrap_or("public");
            format!(
                "SELECT * FROM {}.{} LIMIT {}",
                wrap_identifier_for_adapter(adapter_key, s),
                wrap_identifier_for_adapter(adapter_key, table),
                limit
            )
        }
        "sqlserver" => {
            format!(
                "SELECT TOP {} * FROM {}",
                limit,
                wrap_identifier_for_adapter(adapter_key, table)
            )
        }
        _ => {
            format!(
                "SELECT * FROM {} LIMIT {}",
                wrap_identifier_for_adapter(adapter_key, table),
                limit
            )
        }
    }
}
