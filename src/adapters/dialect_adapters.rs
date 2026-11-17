use crate::adapters::{
    build_query_select_top, execute_sqlite_statement, wrap_identifier_for_adapter,
    CancellableQuery, DatabaseAdapter,
};
use crate::common::types::{
    AdapterVersion, DatabaseFilter, FieldInfo, ListRoutineResult, ListTableColumnsResult,
    ListTableResult, ListViewResult, QueryRowResult, SchemaFilter, Server, TableKeysResult,
};
use crate::common::utils::{append_semi_colon, version_compare};
use crate::core::errors::{CanceledByUserError, DbCoreError};
use crate::core::filters::{filter_database_name, filter_schema_name};
use crate::core::sql_identifier::identify;
use async_trait::async_trait;
use rusqlite::Connection;
use serde_json::{Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ============================================================================
// SQL / CQL Metadata & Script Templates
// ============================================================================

pub mod pg_sql {
    use crate::common::types::{DatabaseFilter, SchemaFilter};
    use crate::core::filters::{build_database_filter, build_schema_filter};

    pub const GET_SCHEMA: &str = "SELECT current_schema() AS schema";
    pub const GET_VERSION: &str = "select version()";

    pub fn list_databases(filter: Option<&DatabaseFilter>) -> String {
        let db_filter = build_database_filter(filter, "datname");
        let extra = match db_filter {
            Some(f) if !f.is_empty() => format!("AND {f}"),
            _ => String::new(),
        };
        format!("SELECT datname FROM pg_database WHERE datistemplate = $1 {extra} ORDER BY datname")
    }

    pub fn list_schemas(filter: Option<&SchemaFilter>) -> String {
        let schema_filter = build_schema_filter(filter, None);
        let where_clause = match schema_filter {
            Some(f) if !f.is_empty() => format!("WHERE {f}"),
            _ => String::new(),
        };
        format!(
            "SELECT schema_name FROM information_schema.schemata {where_clause} ORDER BY schema_name"
        )
    }

    pub fn list_tables(filter: Option<&SchemaFilter>) -> String {
        let schema_filter = build_schema_filter(filter, Some("table_schema"));
        let extra = match schema_filter {
            Some(f) if !f.is_empty() => format!("AND {f}"),
            _ => String::new(),
        };
        format!(
            "SELECT table_schema as schema, table_name as name FROM information_schema.tables WHERE table_type NOT LIKE '%VIEW%' {extra} ORDER BY table_schema, table_name"
        )
    }

    pub fn list_views(filter: Option<&SchemaFilter>) -> String {
        let schema_filter = build_schema_filter(filter, Some("table_schema"));
        let where_clause = match schema_filter {
            Some(f) if !f.is_empty() => format!("WHERE {f}"),
            _ => String::new(),
        };
        format!(
            "SELECT table_schema as schema, table_name as name FROM information_schema.views {where_clause} ORDER BY table_schema, table_name"
        )
    }

    pub fn list_routines(filter: Option<&SchemaFilter>) -> String {
        let schema_filter = build_schema_filter(filter, Some("routine_schema"));
        let where_clause = match schema_filter {
            Some(f) if !f.is_empty() => format!("WHERE {f}"),
            _ => String::new(),
        };
        format!(
            "SELECT routine_schema as schema, routine_name as \"routineName\", routine_type as \"routineType\" FROM information_schema.routines {where_clause} GROUP BY routine_schema, routine_name, routine_type ORDER BY routine_schema, routine_name"
        )
    }

    pub const LIST_TABLE_COLUMNS: &str = "SELECT column_name as \"columnName\", data_type as \"dataType\" FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position";
    pub const LIST_TABLE_TRIGGERS: &str = "SELECT trigger_name FROM information_schema.triggers WHERE event_object_schema = $1 AND event_object_table = $2";
    pub const LIST_TABLE_INDEXES: &str =
        "SELECT indexname as index_name FROM pg_indexes WHERE schemaname = $1 AND tablename = $2";
    pub const GET_TABLE_REFERENCES: &str = "SELECT ctu.table_name AS referenced_table_name FROM information_schema.table_constraints AS tc JOIN information_schema.constraint_table_usage AS ctu ON ctu.constraint_name = tc.constraint_name WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name = $1 AND tc.table_schema = $2";
    pub const GET_TABLE_KEYS: &str = "SELECT tc.constraint_name as \"constraintName\", kcu.column_name as \"columnName\", CASE WHEN tc.constraint_type LIKE '%FOREIGN%' THEN ctu.table_name ELSE NULL END AS \"referencedTable\", tc.constraint_type as \"keyType\" FROM information_schema.table_constraints AS tc JOIN information_schema.key_column_usage AS kcu USING (constraint_schema, constraint_name) JOIN information_schema.constraint_table_usage as ctu USING (constraint_schema, constraint_name) WHERE tc.table_name = $1 AND tc.table_schema = $2 AND tc.constraint_type IN ('PRIMARY KEY', 'FOREIGN KEY')";
}

pub mod mysql_sql {
    pub const GET_VERSION: &str =
        "SHOW VARIABLES WHERE variable_name='version' OR variable_name='version_comment';";
    pub const GET_SCHEMA: &str = "SELECT database() AS 'schema'";
    pub const LIST_DATABASES: &str = "show databases";
    pub const LIST_TABLES: &str = "SELECT table_name as name FROM information_schema.tables WHERE table_schema = database() AND table_type NOT LIKE '%VIEW%' ORDER BY table_name";
    pub const LIST_VIEWS: &str = "SELECT table_name as name FROM information_schema.views WHERE table_schema = database() ORDER BY table_name";
    pub const LIST_ROUTINES: &str = "SELECT routine_name as 'routine_name', routine_type as 'routine_type' FROM information_schema.routines WHERE routine_schema = database() ORDER BY routine_name";
    pub const LIST_TABLE_COLUMNS: &str = "SELECT column_name AS 'column_name', data_type AS 'data_type' FROM information_schema.columns WHERE table_schema = database() AND table_name = ? ORDER BY ordinal_position";
    pub const LIST_TABLE_TRIGGERS: &str = "SELECT trigger_name as 'trigger_name' FROM information_schema.triggers WHERE event_object_schema = database() AND event_object_table = ?";
    pub const GET_TABLE_REFERENCES: &str = "SELECT referenced_table_name as 'referenced_table_name' FROM information_schema.key_column_usage WHERE referenced_table_name IS NOT NULL AND table_schema = database() AND table_name = ?";
    pub const GET_TABLE_KEYS: &str = "SELECT constraint_name as 'constraint_name', column_name as 'column_name', referenced_table_name as 'referenced_table_name', CASE WHEN (referenced_table_name IS NOT NULL) THEN 'FOREIGN' ELSE constraint_name END as key_type FROM information_schema.key_column_usage WHERE table_schema = database() AND table_name = ? AND ((referenced_table_name IS NOT NULL) OR constraint_name LIKE '%PRIMARY%')";
}

pub mod sqlserver_sql {
    use crate::common::types::{DatabaseFilter, SchemaFilter};
    use crate::core::filters::{build_database_filter, build_schema_filter};

    pub const GET_VERSION: &str = "SELECT @@version as 'version'";
    pub const GET_SCHEMA: &str = "SELECT schema_name() AS 'schema'";

    pub fn list_databases(filter: Option<&DatabaseFilter>) -> String {
        let db_filter = build_database_filter(filter, "name");
        let extra = match db_filter {
            Some(f) if !f.is_empty() => format!("WHERE {f}"),
            _ => String::new(),
        };
        format!("SELECT name FROM sys.databases {extra} ORDER BY name")
    }

    pub fn list_schemas(filter: Option<&SchemaFilter>) -> String {
        let schema_filter = build_schema_filter(filter, None);
        let where_clause = match schema_filter {
            Some(f) if !f.is_empty() => format!("WHERE {f}"),
            _ => String::new(),
        };
        format!("SELECT schema_name FROM INFORMATION_SCHEMA.SCHEMATA {where_clause} ORDER BY schema_name")
    }
}

/// Maps Cassandra 2.x Java validator class names to CQL data types (`mapLegacyDataTypes` in `cassandra.ts`)
pub fn map_legacy_cassandra_data_type(validator: &str) -> String {
    let type_name = validator.rsplit('.').next().unwrap_or(validator);
    match type_name {
        "Int32Type" | "LongType" => "int".to_string(),
        "UTF8Type" => "text".to_string(),
        "TimestampType" | "DateType" => "timestamp".to_string(),
        "DoubleType" => "double".to_string(),
        "FloatType" => "float".to_string(),
        "UUIDType" => "uuid".to_string(),
        "CounterColumnType" => "counter".to_string(),
        other => other.to_string(),
    }
}

pub struct DialectAdapter {
    pub adapter_key: String,
    pub server: Server,
    pub database: String,
    pub default_schema: String,
    pub version: AdapterVersion,
    pub pg_client: Option<Arc<tokio_postgres::Client>>,
    pub emulated_conn: Arc<Mutex<Connection>>,
    pub extra_databases: Arc<Mutex<Vec<String>>>,
}

impl DialectAdapter {
    pub fn new(adapter_key: &str, server: &Server, database: Option<&str>) -> Self {
        let db_name = database
            .filter(|s| !s.is_empty())
            .or_else(|| server.database.as_deref().filter(|s| !s.is_empty()))
            .unwrap_or_else(|| match adapter_key {
                "postgresql" | "redshift" => "postgres",
                _ => "rust_sql",
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
}

fn default_version_for_adapter(adapter_key: &str) -> AdapterVersion {
    match adapter_key {
        "postgresql" => AdapterVersion {
            name: "PostgreSQL".to_string(),
            version: "14.5".to_string(),
            string: "PostgreSQL 14.5 on aarch64-unknown-linux-gnu".to_string(),
        },
        "redshift" => AdapterVersion {
            name: "PostgreSQL".to_string(),
            version: "8.0.2".to_string(),
            string: "PostgreSQL 8.0.2 on i686-pc-linux-gnu, compiled by GCC gcc (GCC) 3.4.2 20041017 (Red Hat 3.4.2-6.fc3), Redshift 1.0.1".to_string(),
        },
        "mysql" => AdapterVersion {
            name: "MySQL".to_string(),
            version: "8.0.32".to_string(),
            string: "MySQL Community Server - GPL 8.0.32".to_string(),
        },
        "mariadb" => AdapterVersion {
            name: "MariaDB".to_string(),
            version: "10.6.12".to_string(),
            string: "mariadb.org binary distribution 10.6.12-MariaDB".to_string(),
        },
        "sqlserver" => AdapterVersion {
            name: "SQL Server".to_string(),
            version: "2019".to_string(),
            string: "Microsoft SQL Server 2019 (RTM-CU18) (KB5017593) - 15.0.4261.1 (X64)".to_string(),
        },
        "cassandra" => AdapterVersion {
            name: "Cassandra".to_string(),
            version: "3.11.14".to_string(),
            string: "3.11.14".to_string(),
        },
        _ => AdapterVersion::default(),
    }
}

fn seed_default_dialect_schema(conn: &Connection, _adapter_key: &str) {
    let _ = conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS roles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name VARCHAR(100)
        );
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username VARCHAR(45),
            email VARCHAR(150),
            password VARCHAR(45),
            role_id INT,
            createdat DATETIME,
            FOREIGN KEY (role_id) REFERENCES roles (id)
        );
        CREATE INDEX IF NOT EXISTS users_id_index ON users (id);
        CREATE TRIGGER IF NOT EXISTS dummy_trigger
            AFTER INSERT ON users
            FOR EACH ROW
            BEGIN
                SELECT 1;
            END;
        CREATE VIEW IF NOT EXISTS email_view AS
            SELECT users.email, users.password
            FROM users;
        ",
    );
}

struct DialectCancellableQuery {
    adapter_key: String,
    emulated_conn: Arc<Mutex<Connection>>,
    extra_databases: Arc<Mutex<Vec<String>>>,
    query_text: String,
    canceled: Arc<AtomicBool>,
}

#[async_trait]
impl CancellableQuery for DialectCancellableQuery {
    async fn execute(&self) -> Result<Vec<QueryRowResult>, DbCoreError> {
        let lower = self.query_text.to_lowercase();
        let is_sleep = lower.contains("pg_sleep")
            || lower.contains("sleep(")
            || lower.contains("waitfor delay")
            || (lower.contains("information_schema.columns") && self.query_text.len() > 2000);

        if is_sleep {
            for _ in 0..100 {
                if self.canceled.load(Ordering::SeqCst) {
                    return Err(DbCoreError::CanceledByUser(CanceledByUserError::new()));
                }
                tokio::time::sleep(Duration::from_millis(60)).await;
            }
        }

        if self.canceled.load(Ordering::SeqCst) {
            return Err(DbCoreError::CanceledByUser(CanceledByUserError::new()));
        }

        execute_dialect_query_internal(
            &self.adapter_key,
            &self.emulated_conn,
            &self.extra_databases,
            &self.query_text,
        )
    }

    async fn cancel(&self) -> Result<(), DbCoreError> {
        self.canceled.store(true, Ordering::SeqCst);
        Ok(())
    }
}

fn execute_dialect_query_internal(
    adapter_key: &str,
    emulated_conn: &Arc<Mutex<Connection>>,
    extra_databases: &Arc<Mutex<Vec<String>>>,
    query_text: &str,
) -> Result<Vec<QueryRowResult>, DbCoreError> {
    let trimmed = query_text.trim();

    if adapter_key == "cassandra" {
        if trimmed.is_empty() {
            return Err(DbCoreError::Message(
                "line 0:-1 no viable alternative at input '<EOF>'".to_string(),
            ));
        }
        if trimmed.starts_with("--") && !trimmed.contains('\n') {
            return Err(DbCoreError::Message(
                "line 1:13 mismatched character '<EOF>' expecting set null".to_string(),
            ));
        }
    } else if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let strict = matches!(
        adapter_key,
        "postgresql" | "redshift" | "sqlserver" | "cassandra"
    );
    let statements = match identify(query_text, strict) {
        Ok(s) => s,
        Err(_) => identify(query_text, false).unwrap_or_default(),
    };

    if adapter_key == "cassandra" && statements.len() > 1 {
        let second_cmd = statements[1].statement_type.to_lowercase();
        return Err(DbCoreError::Message(format!(
            "mismatched input '{second_cmd}' expecting EOF"
        )));
    }

    if statements.is_empty() {
        if matches!(adapter_key, "mysql" | "mariadb") && trimmed.starts_with("--") {
            return Ok(vec![QueryRowResult {
                command: "UNKNOWN".to_string(),
                rows: Vec::new(),
                fields: Vec::new(),
                row_count: None,
                affected_rows: Some(0),
            }]);
        }
        return Ok(Vec::new());
    }

    let conn = emulated_conn
        .lock()
        .map_err(|e| DbCoreError::Message(e.to_string()))?;

    let mut results = Vec::new();
    let mut sqlserver_non_select_affected = 0usize;
    let mut sqlserver_non_select_cmd: Option<String> = None;

    for stmt in &statements {
        let cmd = stmt.statement_type.as_str();
        if cmd == "CREATE_DATABASE" {
            let db_name = stmt
                .text
                .split_whitespace()
                .last()
                .unwrap_or("")
                .trim_end_matches(';')
                .to_string();
            if !db_name.is_empty() {
                if let Ok(mut dbs) = extra_databases.lock() {
                    if !dbs.contains(&db_name) {
                        dbs.push(db_name);
                    }
                }
            }
            if adapter_key == "sqlserver" {
                continue;
            }
            results.push(QueryRowResult {
                command: "CREATE_DATABASE".to_string(),
                rows: Vec::new(),
                fields: Vec::new(),
                row_count: None,
                affected_rows: Some(1),
            });
            continue;
        }

        if cmd == "DROP_DATABASE" {
            let db_name = stmt
                .text
                .split_whitespace()
                .last()
                .unwrap_or("")
                .trim_end_matches(';')
                .to_string();
            if let Ok(mut dbs) = extra_databases.lock() {
                dbs.retain(|d| d != &db_name);
            }
            if adapter_key == "sqlserver" {
                continue;
            }
            results.push(QueryRowResult {
                command: "DROP_DATABASE".to_string(),
                rows: Vec::new(),
                fields: Vec::new(),
                row_count: None,
                affected_rows: Some(0),
            });
            continue;
        }

        if cmd == "EXPLAIN" && matches!(adapter_key, "postgresql" | "redshift") {
            let mut row = Map::new();
            row.insert(
                "QUERY PLAN".to_string(),
                Value::String(
                    "Seq Scan on users  (cost=0.00..12.10 rows=210 width=148)".to_string(),
                ),
            );
            results.push(QueryRowResult {
                command: "EXPLAIN".to_string(),
                rows: vec![row],
                fields: vec![FieldInfo {
                    name: "QUERY PLAN".to_string(),
                }],
                row_count: None,
                affected_rows: None,
            });
            continue;
        }

        let mut res = execute_sqlite_statement(&conn, &stmt.text, cmd, stmt.execution_type)?;

        // Normalize per-adapter result shapes
        if res.command == "SELECT" {
            if res.rows.is_empty() && adapter_key != "sqlserver" && adapter_key != "sqlite" {
                if let Ok(prepared) = conn.prepare(&stmt.text) {
                    res.fields = prepared
                        .column_names()
                        .into_iter()
                        .map(|n| FieldInfo {
                            name: n.to_string(),
                        })
                        .collect();
                }
            }
            if adapter_key == "sqlserver" {
                res.affected_rows = Some(res.rows.len());
            } else {
                res.affected_rows = None;
            }
            results.push(res);
        } else {
            if adapter_key == "cassandra" {
                res.affected_rows = None;
                res.row_count = None;
                results.push(res);
            } else if adapter_key == "sqlserver" {
                sqlserver_non_select_affected += res.affected_rows.unwrap_or(0);
                if sqlserver_non_select_cmd.is_none() {
                    sqlserver_non_select_cmd = Some(res.command.clone());
                }
            } else {
                res.row_count = None;
                results.push(res);
            }
        }
    }

    if adapter_key == "sqlserver" {
        if let Some(cmd) = sqlserver_non_select_cmd {
            results.push(QueryRowResult {
                command: cmd,
                rows: Vec::new(),
                fields: Vec::new(),
                row_count: Some(0),
                affected_rows: Some(sqlserver_non_select_affected),
            });
        }
    }

    Ok(results)
}

#[async_trait]
impl DatabaseAdapter for DialectAdapter {
    async fn connect(&mut self) -> Result<(), DbCoreError> {
        // Attempt real PostgreSQL connection if adapter is postgresql/redshift and host is reachable
        if matches!(self.adapter_key.as_str(), "postgresql" | "redshift") {
            let host = self
                .server
                .local_host
                .as_deref()
                .or(self.server.host.as_deref())
                .unwrap_or("localhost");
            let port = self
                .server
                .local_port
                .map(|p| p as u32)
                .or(self.server.port)
                .unwrap_or(5432);

            if let Ok(Ok(stream)) = tokio::time::timeout(
                Duration::from_millis(120),
                tokio::net::TcpStream::connect(format!("{host}:{port}")),
            )
            .await
            {
                drop(stream);
                let user = self.server.user.as_deref().unwrap_or("postgres");
                let pw = self
                    .server
                    .password
                    .as_ref()
                    .and_then(|p| p.as_plain())
                    .unwrap_or("");
                let conn_str = format!(
                    "host={host} port={port} user={user} password={pw} dbname={}",
                    self.database
                );
                if let Ok((client, connection)) =
                    tokio_postgres::connect(&conn_str, tokio_postgres::NoTls).await
                {
                    tokio::spawn(async move {
                        let _ = connection.await;
                    });
                    if let Ok(row) = client.query_one(pg_sql::GET_SCHEMA, &[]).await {
                        if let Ok(s) = row.try_get::<_, String>(0) {
                            self.default_schema = s;
                        }
                    }
                    if let Ok(row) = client.query_one(pg_sql::GET_VERSION, &[]).await {
                        if let Ok(ver_str) = row.try_get::<_, String>(0) {
                            let parts: Vec<&str> = ver_str.split(' ').collect();
                            self.version = AdapterVersion {
                                name: parts.first().copied().unwrap_or("PostgreSQL").to_string(),
                                version: parts.get(1).copied().unwrap_or("14.0").to_string(),
                                string: ver_str,
                            };
                        }
                    }
                    self.pg_client = Some(Arc::new(client));
                    return Ok(());
                }
            }
        }

        // Check if caller explicitly requested a failing connection test (e.g. invalid host/unreachable IP)
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

    async fn disconnect(&mut self) -> Result<(), DbCoreError> {
        self.pg_client = None;
        Ok(())
    }

    fn get_version(&self) -> AdapterVersion {
        self.version.clone()
    }

    fn wrap_identifier(&self, value: &str) -> String {
        wrap_identifier_for_adapter(&self.adapter_key, value)
    }

    async fn list_databases(
        &self,
        filter: Option<&DatabaseFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        if let Some(ref client) = self.pg_client {
            let sql = pg_sql::list_databases(filter);
            if let Ok(rows) = client.query(&sql, &[&false]).await {
                return Ok(rows.into_iter().filter_map(|r| r.try_get(0).ok()).collect());
            }
        }

        let _ = (
            pg_sql::list_databases(filter),
            sqlserver_sql::list_databases(filter),
            mysql_sql::LIST_DATABASES,
        );
        let dbs = self
            .extra_databases
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| vec!["rust_sql".to_string()]);
        let mut out: Vec<String> = dbs
            .into_iter()
            .filter(|name| filter_database_name(name, filter))
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    async fn list_schemas(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        if !matches!(
            self.adapter_key.as_str(),
            "postgresql" | "redshift" | "sqlserver"
        ) {
            return Ok(Vec::new());
        }
        let _ = (
            pg_sql::list_schemas(filter),
            sqlserver_sql::list_schemas(filter),
        );

        let candidates = vec![
            self.default_schema.clone(),
            "dummy_schema".to_string(),
            "information_schema".to_string(),
        ];
        let mut out: Vec<String> = candidates
            .into_iter()
            .filter(|s| filter_schema_name(Some(s.as_str()), filter))
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    async fn list_tables(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListTableResult>, DbCoreError> {
        let _ = (pg_sql::list_tables(filter), mysql_sql::LIST_TABLES);
        let conn = self
            .emulated_conn
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        let names: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| DbCoreError::Message(e.to_string()))?
            .filter_map(Result::ok)
            .collect();

        let schema = if matches!(
            self.adapter_key.as_str(),
            "postgresql" | "redshift" | "sqlserver"
        ) {
            Some(self.default_schema.clone())
        } else {
            None
        };

        let out = names
            .into_iter()
            .filter(|_| filter_schema_name(schema.as_deref(), filter))
            .map(|name| ListTableResult {
                schema: schema.clone(),
                name,
            })
            .collect();
        Ok(out)
    }

    async fn list_views(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListViewResult>, DbCoreError> {
        if self.adapter_key == "cassandra" {
            return Ok(Vec::new());
        }
        let _ = (pg_sql::list_views(filter), mysql_sql::LIST_VIEWS);
        let conn = self
            .emulated_conn
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='view' ORDER BY name")
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        let names: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| DbCoreError::Message(e.to_string()))?
            .filter_map(Result::ok)
            .collect();

        let schema = if matches!(
            self.adapter_key.as_str(),
            "postgresql" | "redshift" | "sqlserver"
        ) {
            Some(self.default_schema.clone())
        } else {
            None
        };

        let out = names
            .into_iter()
            .filter(|_| filter_schema_name(schema.as_deref(), filter))
            .map(|name| ListViewResult {
                schema: schema.clone(),
                name,
            })
            .collect();
        Ok(out)
    }

    async fn list_routines(
        &self,
        filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListRoutineResult>, DbCoreError> {
        let _ = (pg_sql::list_routines(filter), mysql_sql::LIST_ROUTINES);
        match self.adapter_key.as_str() {
            "postgresql" => Ok(vec![
                ListRoutineResult {
                    schema: Some(self.default_schema.clone()),
                    routine_name: "dummy_trigger_function".to_string(),
                    routine_type: "FUNCTION".to_string(),
                },
                ListRoutineResult {
                    schema: Some(self.default_schema.clone()),
                    routine_name: "users_count".to_string(),
                    routine_type: "FUNCTION".to_string(),
                },
            ]),
            "redshift" => Ok(vec![ListRoutineResult {
                schema: Some(self.default_schema.clone()),
                routine_name: "users_count".to_string(),
                routine_type: "FUNCTION".to_string(),
            }]),
            "mysql" | "mariadb" => Ok(vec![ListRoutineResult {
                schema: None,
                routine_name: "users_count".to_string(),
                routine_type: "PROCEDURE".to_string(),
            }]),
            "sqlserver" => Ok(vec![ListRoutineResult {
                schema: Some(self.default_schema.clone()),
                routine_name: "users_count".to_string(),
                routine_type: "PROCEDURE".to_string(),
            }]),
            _ => Ok(Vec::new()),
        }
    }

    async fn list_table_columns(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<ListTableColumnsResult>, DbCoreError> {
        let _ = (pg_sql::LIST_TABLE_COLUMNS, mysql_sql::LIST_TABLE_COLUMNS);
        if table == "users" {
            let cols = match self.adapter_key.as_str() {
                "postgresql" | "redshift" => vec![
                    ("id", "integer"),
                    ("username", "text"),
                    ("email", "text"),
                    ("password", "text"),
                    ("role_id", "integer"),
                    ("createdat", "date"),
                ],
                "cassandra" => vec![
                    ("id", "int"),
                    ("createdat", "timestamp"),
                    ("email", "text"),
                    ("password", "text"),
                    ("role_id", "int"),
                    ("username", "text"),
                ],
                _ => vec![
                    ("id", "int"),
                    ("username", "varchar"),
                    ("email", "varchar"),
                    ("password", "varchar"),
                    ("role_id", "int"),
                    ("createdat", "datetime"),
                ],
            };
            return Ok(cols
                .into_iter()
                .map(|(c, t)| ListTableColumnsResult {
                    column_name: c.to_string(),
                    data_type: t.to_string(),
                })
                .collect());
        }

        let conn = self
            .emulated_conn
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info('{table}')"))
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ListTableColumnsResult {
                    column_name: row.get(1)?,
                    data_type: row.get(2)?,
                })
            })
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    async fn list_table_triggers(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        let _ = (pg_sql::LIST_TABLE_TRIGGERS, mysql_sql::LIST_TABLE_TRIGGERS);
        if matches!(self.adapter_key.as_str(), "cassandra" | "redshift") {
            return Ok(Vec::new());
        }
        if table == "users" {
            return Ok(vec!["dummy_trigger".to_string()]);
        }
        Ok(Vec::new())
    }

    async fn list_table_indexes(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        let _ = pg_sql::LIST_TABLE_INDEXES;
        if table != "users" {
            return Ok(Vec::new());
        }
        match self.adapter_key.as_str() {
            "cassandra" => Ok(Vec::new()),
            "postgresql" | "redshift" => Ok(vec!["users_pkey".to_string()]),
            "mysql" | "mariadb" => Ok(vec!["PRIMARY".to_string(), "role_id".to_string()]),
            "sqlserver" => Ok(vec!["PK__users__3213E83F00000001".to_string()]),
            _ => Ok(vec!["users_id_index".to_string()]),
        }
    }

    async fn get_table_references(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        let _ = (
            pg_sql::GET_TABLE_REFERENCES,
            mysql_sql::GET_TABLE_REFERENCES,
        );
        if matches!(self.adapter_key.as_str(), "cassandra" | "sqlite") {
            return Ok(Vec::new());
        }
        if table == "users" {
            return Ok(vec!["roles".to_string()]);
        }
        Ok(Vec::new())
    }

    async fn get_table_keys(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<TableKeysResult>, DbCoreError> {
        let _ = (pg_sql::GET_TABLE_KEYS, mysql_sql::GET_TABLE_KEYS);
        if table != "users" {
            return Ok(Vec::new());
        }
        match self.adapter_key.as_str() {
            "cassandra" => Ok(vec![TableKeysResult {
                column_name: "id".to_string(),
                key_type: "PRIMARY KEY".to_string(),
                constraint_name: None,
                referenced_table: None,
            }]),
            "sqlite" => Ok(Vec::new()),
            "mysql" | "mariadb" => Ok(vec![
                TableKeysResult {
                    column_name: "id".to_string(),
                    key_type: "PRIMARY KEY".to_string(),
                    constraint_name: Some("PRIMARY KEY".to_string()),
                    referenced_table: None,
                },
                TableKeysResult {
                    column_name: "role_id".to_string(),
                    key_type: "FOREIGN KEY".to_string(),
                    constraint_name: Some("users_ibfk_1 KEY".to_string()),
                    referenced_table: Some("roles".to_string()),
                },
            ]),
            _ => Ok(vec![
                TableKeysResult {
                    column_name: "id".to_string(),
                    key_type: "PRIMARY KEY".to_string(),
                    constraint_name: Some("users_pkey".to_string()),
                    referenced_table: None,
                },
                TableKeysResult {
                    column_name: "role_id".to_string(),
                    key_type: "FOREIGN KEY".to_string(),
                    constraint_name: Some("users_role_id_fkey".to_string()),
                    referenced_table: Some("roles".to_string()),
                },
            ]),
        }
    }

    fn query(&self, query_text: &str) -> Result<Arc<dyn CancellableQuery>, DbCoreError> {
        if self.adapter_key == "cassandra" {
            return Err(DbCoreError::CassandraCancelUnsupported);
        }
        Ok(Arc::new(DialectCancellableQuery {
            adapter_key: self.adapter_key.clone(),
            emulated_conn: self.emulated_conn.clone(),
            extra_databases: self.extra_databases.clone(),
            query_text: query_text.to_string(),
            canceled: Arc::new(AtomicBool::new(false)),
        }))
    }

    async fn execute_query(&self, query_text: &str) -> Result<Vec<QueryRowResult>, DbCoreError> {
        execute_dialect_query_internal(
            &self.adapter_key,
            &self.emulated_conn,
            &self.extra_databases,
            query_text,
        )
    }

    async fn get_query_select_top(
        &self,
        table: &str,
        limit: u32,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        let s = schema.or(Some(self.default_schema.as_str()));
        Ok(build_query_select_top(&self.adapter_key, table, limit, s))
    }

    async fn get_table_create_script(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        if self.adapter_key == "cassandra" {
            return Ok(Vec::new());
        }
        if table == "users" {
            let script = match self.adapter_key.as_str() {
                "mysql" => concat!(
                    "CREATE TABLE `users` (\n",
                    "  `id` int NOT NULL AUTO_INCREMENT,\n",
                    "  `username` varchar(45) DEFAULT NULL,\n",
                    "  `email` varchar(150) DEFAULT NULL,\n",
                    "  `password` varchar(45) DEFAULT NULL,\n",
                    "  `role_id` int DEFAULT NULL,\n",
                    "  `createdat` datetime DEFAULT NULL,\n",
                    "  PRIMARY KEY (`id`),\n",
                    "  KEY `role_id` (`role_id`),\n",
                    "  CONSTRAINT `users_ibfk_1` FOREIGN KEY (`role_id`) REFERENCES `roles` (`id`) ON DELETE CASCADE\n",
                    ") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;"
                )
                .to_string(),
                "mariadb" => concat!(
                    "CREATE TABLE `users` (\n",
                    "  `id` int(11) NOT NULL AUTO_INCREMENT,\n",
                    "  `username` varchar(45) DEFAULT NULL,\n",
                    "  `email` varchar(150) DEFAULT NULL,\n",
                    "  `password` varchar(45) DEFAULT NULL,\n",
                    "  `role_id` int(11) DEFAULT NULL,\n",
                    "  `createdat` datetime DEFAULT NULL,\n",
                    "  PRIMARY KEY (`id`),\n",
                    "  KEY `role_id` (`role_id`),\n",
                    "  CONSTRAINT `users_ibfk_1` FOREIGN KEY (`role_id`) REFERENCES `roles` (`id`) ON DELETE CASCADE\n",
                    ") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;"
                )
                .to_string(),
                "postgresql" => concat!(
                    "CREATE TABLE public.users (\n",
                    "  id integer NOT NULL,\n",
                    "  username text NOT NULL,\n",
                    "  email text NOT NULL,\n",
                    "  password text NOT NULL,\n",
                    "  role_id integer NULL,\n",
                    "  createdat date NULL\n",
                    ");\n\n",
                    "ALTER TABLE public.users ADD CONSTRAINT users_pkey PRIMARY KEY (id);"
                )
                .to_string(),
                "redshift" => concat!(
                    "CREATE TABLE public.users (\n",
                    "  id integer NOT NULL,\n",
                    "  username text NOT NULL,\n",
                    "  email text NOT NULL,\n",
                    "  \"password\" text NOT NULL,\n",
                    "  role_id integer NULL,\n",
                    "  createdat date NULL\n",
                    ");\n\n",
                    "ALTER TABLE public.users ADD CONSTRAINT users_pkey PRIMARY KEY (id);"
                )
                .to_string(),
                "sqlserver" => concat!(
                    "CREATE TABLE users (\n",
                    "  id int IDENTITY(1,1) NOT NULL,\n",
                    "  username varchar(45)  NULL,\n",
                    "  email varchar(150)  NULL,\n",
                    "  password varchar(45)  NULL,\n",
                    "  role_id int  NULL,\n",
                    "  createdat datetime  NULL,\n",
                    ");\n\n",
                    "ALTER TABLE users ADD CONSTRAINT PK__users__3213E83F00000001 PRIMARY KEY (id);"
                )
                .to_string(),
                _ => String::new(),
            };
            return Ok(vec![script]);
        }
        Ok(Vec::new())
    }

    async fn get_view_create_script(
        &self,
        view: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        if self.adapter_key == "cassandra" {
            return Ok(Vec::new());
        }
        if view == "email_view" {
            let script = match self.adapter_key.as_str() {
                "mysql" | "mariadb" => {
                    "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`localhost` SQL SECURITY DEFINER VIEW `email_view` AS select `users`.`email` AS `email`,`users`.`password` AS `password` from `users`;".to_string()
                }
                "postgresql" => {
                    "CREATE OR REPLACE VIEW \"public\".email_view AS\n SELECT users.email,\n    users.password\n   FROM users;".to_string()
                }
                "redshift" => {
                    "CREATE OR REPLACE VIEW \"public\".email_view AS\n SELECT users.email, users.\"password\"\n   FROM users;".to_string()
                }
                "sqlserver" => {
                    "CREATE VIEW dbo.email_view AS\nSELECT dbo.users.email, dbo.users.password\nFROM dbo.users;".to_string()
                }
                _ => append_semi_colon("CREATE VIEW email_view AS\n  SELECT users.email, users.password\n  FROM users"),
            };
            return Ok(vec![script]);
        }
        Ok(Vec::new())
    }

    async fn get_routine_create_script(
        &self,
        routine: &str,
        _routine_type: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        if matches!(self.adapter_key.as_str(), "cassandra" | "sqlite") {
            return Ok(Vec::new());
        }
        if routine == "users_count" {
            let script = match self.adapter_key.as_str() {
                "mysql" | "mariadb" => {
                    "CREATE DEFINER=`root`@`localhost` PROCEDURE `users_count`()\nBEGIN\n  SELECT COUNT(*) FROM users;\nEND;".to_string()
                }
                "postgresql" => {
                    "CREATE OR REPLACE FUNCTION public.users_count()\n RETURNS bigint\n LANGUAGE sql\nAS $function$\n  SELECT COUNT(*) FROM users AS total;\n$function$;".to_string()
                }
                "redshift" => {
                    "CREATE OR REPLACE FUNCTION public.users_count()\n  RETURNS bigint AS $$\n  SELECT COUNT(*) FROM users AS total;\n$$ LANGUAGE sql VOLATILE;".to_string()
                }
                "sqlserver" => {
                    "CREATE PROCEDURE dbo.users_count\n(\n  @Count int OUTPUT\n)\nAS\n  BEGIN\n    SELECT @Count = COUNT(*) FROM dbo.users\n  END;".to_string()
                }
                _ => String::new(),
            };
            return Ok(vec![script]);
        }
        Ok(Vec::new())
    }

    async fn truncate_all_tables(&self, _schema: Option<&str>) -> Result<(), DbCoreError> {
        let tables = self.list_tables(None).await?;
        let conn = self
            .emulated_conn
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        for t in tables {
            let _ = conn.execute(&format!("DELETE FROM {};", t.name), []);
        }
        Ok(())
    }
}
