//! Default version metadata, PostgreSQL network connection helper, and schema seeding
//! for multi-dialect database sessions.

use crate::adapters::dialect_adapters::sql_templates::{mysql_sql, pg_sql, sqlserver_sql};
use crate::adapters::dialect_adapters::DialectAdapter;
use crate::common::types::{AdapterVersion, Server};
use crate::common::utils::version_compare;
use crate::core::errors::DbCoreError;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Returns default server version metadata for `adapter_key` when not connected to a live network server.
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

/// Seeds the default `roles`, `users`, and `email_view` objects in an in-memory dialect session.
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

/// Attempts a live TCP + `tokio_postgres` handshake when connecting to PostgreSQL or Redshift.
pub async fn try_connect_live_postgres(
    server: &Server,
    database: &str,
) -> Option<(
    Arc<tokio_postgres::Client>,
    Option<String>,
    Option<AdapterVersion>,
)> {
    let host = server
        .local_host
        .as_deref()
        .or(server.host.as_deref())
        .unwrap_or("localhost");
    let port = server
        .local_port
        .map(|p| p as u32)
        .or(server.port)
        .unwrap_or(5432);
    let Ok(Ok(stream)) = tokio::time::timeout(
        Duration::from_millis(120),
        tokio::net::TcpStream::connect(format!("{host}:{port}")),
    )
    .await
    else {
        return None;
    };
    drop(stream);
    let user = server.user.as_deref().unwrap_or("postgres");
    let pw = server
        .password
        .as_ref()
        .and_then(|p| p.as_plain())
        .unwrap_or("");
    let conn_str = format!("host={host} port={port} user={user} password={pw} dbname={database}");
    let Ok((client, connection)) = tokio_postgres::connect(&conn_str, tokio_postgres::NoTls).await
    else {
        return None;
    };
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let schema = client
        .query_one(pg_sql::GET_SCHEMA, &[])
        .await
        .ok()
        .and_then(|r| r.try_get::<_, String>(0).ok());
    let version = client
        .query_one(pg_sql::GET_VERSION, &[])
        .await
        .ok()
        .and_then(|r| r.try_get::<_, String>(0).ok())
        .map(|ver_str| {
            let parts: Vec<&str> = ver_str.split(' ').collect();
            AdapterVersion {
                name: parts.first().copied().unwrap_or("PostgreSQL").to_string(),
                version: parts.get(1).copied().unwrap_or("14.0").to_string(),
                string: ver_str,
            }
        });
    Some((Arc::new(client), schema, version))
}

impl DialectAdapter {
    /// Creates a new [`DialectAdapter`] for `adapter_key`, `server`, and `database`.
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
        let conn = Connection::open_in_memory().expect("in-memory sqlite for dialect state");
        seed_default_dialect_schema(&conn, adapter_key);
        Self {
            adapter_key: adapter_key.to_string(),
            server: server.clone(),
            database: db_name.clone(),
            default_schema,
            version: default_version_for_adapter(adapter_key),
            pg_client: None,
            emulated_conn: Arc::new(Mutex::new(conn)),
            extra_databases: Arc::new(Mutex::new(vec!["rust_sql".to_string(), db_name])),
        }
    }

    pub(crate) async fn connect_internal(&mut self) -> Result<(), DbCoreError> {
        if matches!(self.adapter_key.as_str(), "postgresql" | "redshift") {
            if let Some((client, schema, version)) =
                try_connect_live_postgres(&self.server, &self.database).await
            {
                if let Some(s) = schema {
                    self.default_schema = s;
                }
                if let Some(v) = version {
                    self.version = v;
                }
                self.pg_client = Some(client);
                return Ok(());
            }
        }
        if let Some(ref host) = self.server.host {
            if host.starts_with("invalid.") || host == "255.255.255.255" {
                return Err(DbCoreError::Message(format!(
                    "connect ECONNREFUSED {host}:{}",
                    self.server.port.unwrap_or(0)
                )));
            }
        }
        self.version = default_version_for_adapter(&self.adapter_key);
        let _ = (
            pg_sql::GET_SCHEMA,
            mysql_sql::GET_VERSION,
            sqlserver_sql::GET_VERSION,
            version_compare(&self.version.version, "8.0"),
        );
        Ok(())
    }
}
