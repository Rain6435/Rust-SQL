pub mod dialect_adapters;
pub mod facade;
pub mod sqlite;

pub use dialect_adapters::*;
pub use facade::*;
pub use sqlite::*;

use crate::common::types::{
    AdapterInfo, AdapterVersion, DatabaseFilter, ListRoutineResult, ListTableColumnsResult,
    ListTableResult, ListViewResult, QueryRowResult, SchemaFilter, Server, TableKeysResult,
};
use crate::core::errors::DbCoreError;
use async_trait::async_trait;
use std::sync::{Arc, Mutex, OnceLock};

pub static BUILTIN_ADAPTERS: &[(&str, &str, &str, Option<u32>, Option<&str>, &[&str])] = &[
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

pub static CLIENTS: std::sync::LazyLock<Vec<AdapterInfo>> = std::sync::LazyLock::new(|| {
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

pub static ADAPTERS: std::sync::LazyLock<Vec<AdapterInfo>> =
    std::sync::LazyLock::new(|| CLIENTS.clone());

static CUSTOM_ADAPTERS: OnceLock<Mutex<Vec<AdapterInfo>>> = OnceLock::new();

fn custom_adapters_store() -> &'static Mutex<Vec<AdapterInfo>> {
    CUSTOM_ADAPTERS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn get_adapters() -> Vec<AdapterInfo> {
    let mut all = CLIENTS.clone();
    if let Ok(guard) = custom_adapters_store().lock() {
        all.extend(guard.iter().cloned());
    }
    all
}

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

/// Identifier wrapping per adapter matching `sqlectron-db-core/src/adapters/*.ts`
pub fn wrap_identifier_for_adapter(adapter_key: &str, value: &str) -> String {
    if value == "*" {
        return "*".to_string();
    }
    match adapter_key {
        "mysql" | "mariadb" => {
            format!("`{}`", value.replace('`', "``"))
        }
        "sqlserver" => {
            format!("[{}]", value.replace('[', "["))
        }
        _ => {
            // postgresql, redshift, sqlite, cassandra
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

/// Builds the `SELECT TOP / LIMIT` query per adapter matching `getQuerySelectTop` in `sqlectron-db-core`
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

#[async_trait]
pub trait CancellableQuery: Send + Sync {
    async fn execute(&self) -> Result<Vec<QueryRowResult>, DbCoreError>;
    async fn cancel(&self) -> Result<(), DbCoreError>;
}

#[async_trait]
pub trait DatabaseAdapter: Send + Sync {
    async fn connect(&mut self) -> Result<(), DbCoreError>;
    async fn disconnect(&mut self) -> Result<(), DbCoreError>;
    fn get_version(&self) -> AdapterVersion;
    fn wrap_identifier(&self, value: &str) -> String;

    async fn list_databases(
        &self,
        filter: Option<&DatabaseFilter>,
    ) -> Result<Vec<String>, DbCoreError>;
    async fn list_schemas(
        &self,
        _filter: Option<&SchemaFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }
    async fn list_tables(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListTableResult>, DbCoreError>;
    async fn list_views(
        &self,
        _filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListViewResult>, DbCoreError> {
        Ok(Vec::new())
    }
    async fn list_routines(
        &self,
        _filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListRoutineResult>, DbCoreError> {
        Ok(Vec::new())
    }
    async fn list_table_columns(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ListTableColumnsResult>, DbCoreError>;
    async fn list_table_triggers(
        &self,
        _table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }
    async fn list_table_indexes(
        &self,
        _table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }
    async fn get_table_references(
        &self,
        _table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }
    async fn get_table_keys(
        &self,
        _table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<TableKeysResult>, DbCoreError> {
        Ok(Vec::new())
    }

    fn query(&self, query_text: &str) -> Result<Arc<dyn CancellableQuery>, DbCoreError>;
    async fn execute_query(&self, query_text: &str) -> Result<Vec<QueryRowResult>, DbCoreError>;

    async fn get_query_select_top(
        &self,
        table: &str,
        limit: u32,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError>;

    async fn get_table_create_script(
        &self,
        _table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }
    async fn get_view_create_script(
        &self,
        _view: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }
    async fn get_routine_create_script(
        &self,
        _routine: &str,
        _routine_type: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }
    async fn truncate_all_tables(&self, _schema: Option<&str>) -> Result<(), DbCoreError> {
        Ok(())
    }
}

pub fn adapter_factory(
    adapter_key: &str,
    server: &Server,
    database: Option<&str>,
) -> Result<Box<dyn DatabaseAdapter>, DbCoreError> {
    let adapters = get_adapters();
    if !adapters.iter().any(|a| a.key == adapter_key) {
        return Err(DbCoreError::UnknownAdapter(adapter_key.to_string()));
    }

    match adapter_key {
        "sqlite" => Ok(Box::new(SqliteAdapter::new(server, database))),
        "postgresql" | "redshift" | "mysql" | "mariadb" | "sqlserver" | "cassandra" => {
            Ok(Box::new(DialectAdapter::new(adapter_key, server, database)))
        }
        _ => Ok(Box::new(DialectAdapter::new(adapter_key, server, database))),
    }
}
