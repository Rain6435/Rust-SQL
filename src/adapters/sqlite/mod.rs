//! Native SQLite database adapter powered by `rusqlite`.

pub mod connection;
pub mod executor;

pub use connection::open_sqlite;
pub use executor::{execute_sqlite_statement, map_sqlite_err, SqliteCancellableQuery};

use crate::adapters::{
    build_query_select_top, wrap_identifier_for_adapter, CancellableQuery, DatabaseAdapter,
};
use crate::common::types::{
    AdapterVersion, ConnectionMode, DatabaseFilter, ListRoutineResult, ListTableColumnsResult,
    ListTableResult, ListViewResult, QueryRowResult, SchemaFilter, Server, TableKeysResult,
};
use crate::common::utils::append_semi_colon;
use crate::core::errors::DbCoreError;
use crate::core::sql_identifier::identify;
use async_trait::async_trait;
use rusqlite::Connection;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// SQLite database adapter supporting both file-backed databases and persistent `:memory:` sessions.
pub struct SqliteAdapter {
    /// Target database file path or `":memory:"`.
    pub db_path: String,
    /// Discovered SQLite library version.
    pub version: AdapterVersion,
    /// Shared SQLite connection kept open across queries on the same session.
    pub shared_conn: Arc<Mutex<Option<Connection>>>,
    /// Whether the session is read-only (see [`connection`]).
    pub read_only: Arc<AtomicBool>,
}

impl SqliteAdapter {
    /// Creates a new [`SqliteAdapter`] configured for `database` (or `server.database`, defaulting to `":memory:"`).
    pub fn new(server: &Server, database: Option<&str>) -> Self {
        let db_path = database
            .filter(|s| !s.is_empty())
            .or_else(|| server.database.as_deref().filter(|s| !s.is_empty()))
            .unwrap_or(":memory:")
            .to_string();
        Self {
            db_path,
            version: AdapterVersion::default(),
            shared_conn: Arc::new(Mutex::new(None)),
            read_only: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[async_trait]
impl DatabaseAdapter for SqliteAdapter {
    async fn connect(&mut self) -> Result<(), DbCoreError> {
        let ver: String = self.open_or_with_conn(|conn| {
            conn.query_row("SELECT sqlite_version() as version", [], |row| row.get(0))
                .map_err(map_sqlite_err)
        })?;
        self.version = AdapterVersion {
            name: "SQLite".to_string(),
            version: ver.clone(),
            string: format!("SQLite {ver}"),
        };
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), DbCoreError> {
        if let Ok(mut guard) = self.shared_conn.lock() {
            *guard = None;
        }
        Ok(())
    }

    fn get_version(&self) -> AdapterVersion {
        self.version.clone()
    }

    fn connection_mode(&self) -> ConnectionMode {
        ConnectionMode::Live
    }

    async fn set_read_only(&self, read_only: bool) -> Result<(), DbCoreError> {
        self.apply_read_only(read_only)
    }

    fn wrap_identifier(&self, value: &str) -> String {
        wrap_identifier_for_adapter("sqlite", value)
    }

    async fn list_databases(&self, _: Option<&DatabaseFilter>) -> Result<Vec<String>, DbCoreError> {
        let list = self.query_strings("PRAGMA database_list;", 2)?;
        Ok(list
            .into_iter()
            .map(|s| {
                if s.is_empty() {
                    ":memory:".to_string()
                } else {
                    s
                }
            })
            .collect())
    }

    async fn list_tables(
        &self,
        _: Option<&SchemaFilter>,
    ) -> Result<Vec<ListTableResult>, DbCoreError> {
        let sql = "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name";
        Ok(self
            .query_strings(sql, 0)?
            .into_iter()
            .map(|name| ListTableResult { schema: None, name })
            .collect())
    }

    async fn list_views(
        &self,
        _: Option<&SchemaFilter>,
    ) -> Result<Vec<ListViewResult>, DbCoreError> {
        Ok(self
            .query_strings("SELECT name FROM sqlite_master WHERE type = 'view'", 0)?
            .into_iter()
            .map(|name| ListViewResult { schema: None, name })
            .collect())
    }

    async fn list_routines(
        &self,
        _: Option<&SchemaFilter>,
    ) -> Result<Vec<ListRoutineResult>, DbCoreError> {
        Ok(Vec::new())
    }

    async fn list_table_columns(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<ListTableColumnsResult>, DbCoreError> {
        self.open_or_with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info('{table}')"))
                .map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(ListTableColumnsResult {
                        column_name: r.get(1)?,
                        data_type: r.get(2)?,
                    })
                })
                .map_err(map_sqlite_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(map_sqlite_err)
        })
    }

    async fn list_table_triggers(
        &self,
        table: &str,
        _: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        let sql = format!(
            "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = '{table}'"
        );
        self.query_strings(&sql, 0)
    }

    async fn list_table_indexes(
        &self,
        table: &str,
        _: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.query_strings(&format!("PRAGMA INDEX_LIST('{table}')"), 1)
    }

    async fn get_table_references(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }

    async fn get_table_keys(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<Vec<TableKeysResult>, DbCoreError> {
        Ok(Vec::new())
    }

    fn query(&self, query_text: &str) -> Result<Arc<dyn CancellableQuery>, DbCoreError> {
        Ok(Arc::new(SqliteCancellableQuery {
            shared_conn: self.shared_conn.clone(),
            db_path: self.db_path.clone(),
            read_only: self.read_only.clone(),
            query_text: query_text.to_string(),
            interrupt_handle: Mutex::new(None),
            canceled: AtomicBool::new(false),
        }))
    }

    async fn execute_query(&self, query_text: &str) -> Result<Vec<QueryRowResult>, DbCoreError> {
        let statements = identify(query_text, false).unwrap_or_default();
        if statements.is_empty() {
            return Ok(Vec::new());
        }
        self.open_or_with_conn(|conn| {
            statements
                .into_iter()
                .map(|s| {
                    execute_sqlite_statement(conn, &s.text, s.statement_type, s.execution_type)
                })
                .collect()
        })
    }

    async fn get_query_select_top(
        &self,
        table: &str,
        limit: u32,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        Ok(build_query_select_top("sqlite", table, limit, schema))
    }

    async fn get_table_create_script(
        &self,
        table: &str,
        _: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        let sql = format!("SELECT sql FROM sqlite_master WHERE name = '{table}';");
        Ok(self
            .query_strings(&sql, 0)?
            .into_iter()
            .map(|s| append_semi_colon(&s))
            .collect())
    }

    async fn get_view_create_script(
        &self,
        view: &str,
        _: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.get_table_create_script(view, None).await
    }

    async fn truncate_all_tables(&self, _schema: Option<&str>) -> Result<(), DbCoreError> {
        for t in self.list_tables(None).await? {
            let _ = self
                .execute_query(&format!("DELETE FROM {};", t.name))
                .await?;
        }
        Ok(())
    }
}
