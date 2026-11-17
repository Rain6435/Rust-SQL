//! Construction and connection of [`DialectAdapter`]: live PostgreSQL/Redshift connections,
//! and the explicitly opted-in in-memory emulator for dialects without a live driver.

use crate::adapters::dialect_adapters::live_pg::connect_live_postgres;
use crate::adapters::get_adapters;
use crate::common::types::{AdapterVersion, DatabaseFilter, Server};
use crate::core::errors::DbCoreError;
use crate::core::filters::filter_database_name;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

/// Multi-dialect adapter: live for PostgreSQL/Redshift, emulated (opt-in only) for the rest.
pub struct DialectAdapter {
    /// Adapter key (`postgresql`, `redshift`, `mysql`, `mariadb`, `sqlserver`, `cassandra`).
    pub adapter_key: String,
    /// Server connection configuration.
    pub server: Server,
    /// Target database or keyspace name.
    pub database: String,
    /// Default schema name (`"public"` for PostgreSQL/Redshift, `"dbo"` for SQL Server).
    pub default_schema: String,
    /// Server version (live) or the imitated dialect's placeholder version (emulated).
    pub version: AdapterVersion,
    /// Whether the user opted in to emulation for dialects without a live driver.
    pub allow_emulation: bool,
    /// Live `tokio_postgres::Client`, set only after a successful live connection.
    pub pg_client: Option<Arc<tokio_postgres::Client>>,
    /// In-memory emulator, set only when emulation was opted in and chosen at connect time.
    pub emulated_conn: Option<Arc<Mutex<Connection>>>,
    /// Database list tracked by the emulator for `CREATE DATABASE` / `DROP DATABASE`.
    pub extra_databases: Arc<Mutex<Vec<String>>>,
}

impl DialectAdapter {
    pub(crate) fn schema_or_default<'a>(&'a self, schema: Option<&'a str>) -> &'a str {
        schema
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.default_schema)
    }
}

/// Placeholder version metadata reported by the emulator for `adapter_key`.
///
/// Emulated sessions have no server, so this only names the dialect being imitated.
pub fn default_version_for_adapter(adapter_key: &str) -> AdapterVersion {
    let mk = |name: &str, ver: &str, s: &str| AdapterVersion {
        name: name.to_string(),
        version: ver.to_string(),
        string: s.to_string(),
    };
    match adapter_key {
        "postgresql" => mk("PostgreSQL", "15.2", "PostgreSQL 15.2"),
        "redshift" => mk("Redshift", "1.0.4", "PostgreSQL 8.0.2 on Redshift 1.0.4"),
        "mysql" => mk("MySQL", "8.0.32", "MySQL 8.0.32"),
        "mariadb" => mk("MariaDB", "10.11.2", "MariaDB 10.11.2"),
        "sqlserver" => mk(
            "Microsoft SQL Server",
            "15.0.2000",
            "Microsoft SQL Server 15.0.2000",
        ),
        "cassandra" => mk("Cassandra", "4.1.0", "Cassandra 4.1.0"),
        _ => AdapterVersion::default(),
    }
}

/// Lists the databases tracked by an emulated session, applying `filter`.
pub fn list_databases_for_emulator(
    extra_databases: &Arc<Mutex<Vec<String>>>,
    filter: Option<&DatabaseFilter>,
) -> Result<Vec<String>, DbCoreError> {
    let dbs = extra_databases
        .lock()
        .map(|g| g.clone())
        .map_err(|e| DbCoreError::Message(e.to_string()))?;
    let mut out: Vec<String> = dbs
        .into_iter()
        .filter(|name| filter_database_name(name, filter))
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Seeds the default `roles`, `users`, and `email_view` objects in an emulated session.
pub fn seed_default_dialect_schema(conn: &Connection, adapter_key: &str) {
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS roles (id INTEGER PRIMARY KEY AUTOINCREMENT, name VARCHAR(100));
         CREATE TABLE IF NOT EXISTS users (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             username VARCHAR(45),
             email VARCHAR(150),
             password VARCHAR(45),
             role_id INT,
             createdat DATETIME
         );",
    );
    if adapter_key != "cassandra" {
        let _ = conn.execute_batch(
            "CREATE VIEW IF NOT EXISTS email_view AS SELECT users.email, users.password FROM users;",
        );
    }
}

/// Returns the display name registered for `adapter_key`, or the key itself.
pub(crate) fn dialect_display_name(adapter_key: &str) -> String {
    get_adapters()
        .into_iter()
        .find(|a| a.key == adapter_key)
        .map(|a| a.name)
        .unwrap_or_else(|| adapter_key.to_string())
}

/// Borrowed handle to whichever backend a connected [`DialectAdapter`] uses.
pub(crate) enum Backend<'a> {
    /// Live `tokio-postgres` client.
    Live(&'a Arc<tokio_postgres::Client>),
    /// Opted-in in-memory emulator.
    Emulated(&'a Arc<Mutex<Connection>>),
}

impl DialectAdapter {
    /// Creates a new, disconnected [`DialectAdapter`] for `adapter_key`, `server`, and `database`.
    ///
    /// Emulation is disallowed until [`with_emulation_allowed`](Self::with_emulation_allowed)
    /// opts in, so dialects without a live driver fail to connect by default.
    pub fn new(adapter_key: &str, server: &Server, database: Option<&str>) -> Self {
        let db_name = database
            .filter(|s| !s.is_empty())
            .or_else(|| server.database.as_deref().filter(|s| !s.is_empty()))
            .unwrap_or(if matches!(adapter_key, "postgresql" | "redshift") {
                "postgres"
            } else {
                "rust_sql"
            })
            .to_string();
        let default_schema = match adapter_key {
            "postgresql" | "redshift" => "public".to_string(),
            "sqlserver" => "dbo".to_string(),
            _ => db_name.clone(),
        };
        Self {
            adapter_key: adapter_key.to_string(),
            server: server.clone(),
            database: db_name.clone(),
            default_schema,
            version: AdapterVersion::default(),
            allow_emulation: false,
            pg_client: None,
            emulated_conn: None,
            extra_databases: Arc::new(Mutex::new(vec!["rust_sql".to_string(), db_name])),
        }
    }

    /// Sets whether this adapter may use the emulator when the dialect has no live driver.
    ///
    /// This never affects PostgreSQL or Redshift, which always connect live.
    pub fn with_emulation_allowed(mut self, allowed: bool) -> Self {
        self.allow_emulation = allowed;
        self
    }

    /// Returns the connected backend, or [`DbCoreError::NoConnectionAvailable`].
    pub(crate) fn backend(&self) -> Result<Backend<'_>, DbCoreError> {
        match (&self.pg_client, &self.emulated_conn) {
            (Some(client), _) => Ok(Backend::Live(client)),
            (None, Some(conn)) => Ok(Backend::Emulated(conn)),
            (None, None) => Err(DbCoreError::NoConnectionAvailable),
        }
    }

    /// Error returned by operations that have no live implementation yet.
    pub(crate) fn live_unsupported(&self, what: &str) -> DbCoreError {
        DbCoreError::Message(format!(
            "{what} is not implemented for live {} connections yet.",
            dialect_display_name(&self.adapter_key)
        ))
    }

    pub(crate) async fn connect_internal(&mut self) -> Result<(), DbCoreError> {
        self.pg_client = None;
        self.emulated_conn = None;
        if matches!(self.adapter_key.as_str(), "postgresql" | "redshift") {
            let live =
                connect_live_postgres(&self.adapter_key, &self.server, &self.database).await?;
            if let Some(schema) = live.schema {
                self.default_schema = schema;
            }
            self.version = live.version;
            self.pg_client = Some(live.client);
            return Ok(());
        }
        if !self.allow_emulation {
            return Err(DbCoreError::LiveConnectionNotImplemented(
                dialect_display_name(&self.adapter_key),
            ));
        }
        let conn = Connection::open_in_memory()
            .map_err(|e| DbCoreError::Message(format!("Could not start the emulator: {e}")))?;
        seed_default_dialect_schema(&conn, &self.adapter_key);
        self.version = default_version_for_adapter(&self.adapter_key);
        self.emulated_conn = Some(Arc::new(Mutex::new(conn)));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::DatabaseAdapter;
    use crate::common::types::ConnectionMode;

    fn server(key: &str) -> Server {
        Server {
            client: key.to_string(),
            host: Some("db.example.test".to_string()),
            port: Some(3306),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn dialects_without_driver_refuse_to_connect_without_opt_in() {
        for key in ["mysql", "mariadb", "sqlserver", "cassandra"] {
            let mut adapter = DialectAdapter::new(key, &server(key), None);
            let err = adapter.connect().await.expect_err("must not connect");
            assert!(
                matches!(err, DbCoreError::LiveConnectionNotImplemented(_)),
                "{key}: {err}"
            );
            assert!(err.to_string().contains("not implemented"), "{key}: {err}");
            assert!(adapter.query("SELECT 1").is_err(), "{key}");
            assert!(adapter.execute_query("SELECT * FROM users").await.is_err());
        }
    }

    #[tokio::test]
    async fn opted_in_dialects_report_emulated_mode() {
        for key in ["mysql", "mariadb", "sqlserver", "cassandra"] {
            let mut adapter =
                DialectAdapter::new(key, &server(key), None).with_emulation_allowed(true);
            adapter.connect().await.expect("emulated connect");
            assert_eq!(adapter.connection_mode(), ConnectionMode::Emulated);
        }
    }
}
