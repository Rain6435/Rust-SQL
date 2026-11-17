use crate::adapters::{
    build_query_select_top, wrap_identifier_for_adapter, CancellableQuery, DatabaseAdapter,
};
use crate::common::types::{
    AdapterVersion, DatabaseFilter, FieldInfo, ListRoutineResult, ListTableColumnsResult,
    ListTableResult, ListViewResult, QueryRowResult, SchemaFilter, Server, TableKeysResult,
};
use crate::common::utils::append_semi_colon;
use crate::core::errors::{CanceledByUserError, DbCoreError};
use crate::core::sql_identifier::{identify, ExecutionType};
use async_trait::async_trait;
use rusqlite::{types::ValueRef, Connection, InterruptHandle};
use serde_json::{Map, Number, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct SqliteAdapter {
    pub db_path: String,
    pub version: AdapterVersion,
    /// Persistent connection for `:memory:` databases or shared session access
    pub shared_conn: Arc<Mutex<Option<Connection>>>,
}

impl SqliteAdapter {
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
        }
    }

    fn open_or_with_conn<F, T>(&self, f: F) -> Result<T, DbCoreError>
    where
        F: FnOnce(&Connection) -> Result<T, DbCoreError>,
    {
        let mut guard = self
            .shared_conn
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        if guard.is_none() {
            let conn = if self.db_path == ":memory:" {
                Connection::open_in_memory().map_err(|e| DbCoreError::Message(e.to_string()))?
            } else {
                Connection::open(&self.db_path).map_err(|e| DbCoreError::Message(e.to_string()))?
            };
            *guard = Some(conn);
        }
        let conn = guard.as_ref().unwrap();
        f(conn)
    }
}

pub fn execute_sqlite_statement(
    conn: &Connection,
    stmt_text: &str,
    stmt_type: &str,
    exec_type: ExecutionType,
) -> Result<QueryRowResult, DbCoreError> {
    let is_mod = exec_type == ExecutionType::Modification
        && !stmt_type.eq_ignore_ascii_case("SELECT")
        && !stmt_type.eq_ignore_ascii_case("PRAGMA")
        && !stmt_type.eq_ignore_ascii_case("EXPLAIN");

    if is_mod {
        let changes = conn.execute(stmt_text, []).map_err(map_sqlite_err)?;
        Ok(QueryRowResult {
            command: stmt_type.to_string(),
            rows: Vec::new(),
            fields: Vec::new(),
            row_count: None,
            affected_rows: Some(changes),
        })
    } else {
        let mut stmt = conn.prepare(stmt_text).map_err(map_sqlite_err)?;
        let col_names: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let mut rows_out = Vec::new();

        let mut rows = stmt.query([]).map_err(map_sqlite_err)?;
        while let Some(row) = rows.next().map_err(map_sqlite_err)? {
            let mut map = Map::new();
            for (idx, col_name) in col_names.iter().enumerate() {
                let val_ref = row.get_ref(idx).map_err(map_sqlite_err)?;
                let json_val = match val_ref {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(i) => Value::Number(Number::from(i)),
                    ValueRef::Real(f) => Number::from_f64(f)
                        .map(Value::Number)
                        .unwrap_or(Value::Null),
                    ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
                    ValueRef::Blob(b) => {
                        if b.len() == 1 {
                            Value::String(b[0].to_string())
                        } else if let Ok(s) = std::str::from_utf8(b) {
                            Value::String(s.to_string())
                        } else {
                            Value::Array(b.iter().map(|&byte| Value::from(byte)).collect())
                        }
                    }
                };
                map.insert(col_name.clone(), json_val);
            }
            rows_out.push(map);
        }

        // Populate fields when rows are non-empty, in SQL column order
        let fields = if !rows_out.is_empty() {
            col_names
                .into_iter()
                .map(|name| FieldInfo { name })
                .collect()
        } else {
            Vec::new()
        };

        let row_count = rows_out.len();
        Ok(QueryRowResult {
            command: if stmt_type.is_empty() {
                "SELECT".to_string()
            } else {
                stmt_type.to_string()
            },
            rows: rows_out,
            fields,
            row_count: Some(row_count),
            affected_rows: Some(0),
        })
    }
}

fn map_sqlite_err(err: rusqlite::Error) -> DbCoreError {
    if let rusqlite::Error::SqliteFailure(ffi_err, _) = &err {
        if ffi_err.extended_code == 9 || ffi_err.code == rusqlite::ErrorCode::OperationInterrupted {
            return DbCoreError::CanceledByUser(CanceledByUserError::new());
        }
    }
    DbCoreError::Message(err.to_string())
}

struct SqliteCancellableQuery {
    shared_conn: Arc<Mutex<Option<Connection>>>,
    db_path: String,
    query_text: String,
    interrupt_handle: Mutex<Option<InterruptHandle>>,
    canceled: AtomicBool,
}

#[async_trait]
impl CancellableQuery for SqliteCancellableQuery {
    async fn execute(&self) -> Result<Vec<QueryRowResult>, DbCoreError> {
        let statements = identify(&self.query_text, false).unwrap_or_default();
        if statements.is_empty() {
            return Ok(Vec::new());
        }

        let mut guard = self
            .shared_conn
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        if guard.is_none() {
            let conn = if self.db_path == ":memory:" {
                Connection::open_in_memory().map_err(|e| DbCoreError::Message(e.to_string()))?
            } else {
                Connection::open(&self.db_path).map_err(|e| DbCoreError::Message(e.to_string()))?
            };
            *guard = Some(conn);
        }
        let conn = guard.as_ref().unwrap();
        if let Ok(mut ih) = self.interrupt_handle.lock() {
            *ih = Some(conn.get_interrupt_handle());
        }

        if self.canceled.load(Ordering::SeqCst) {
            return Err(DbCoreError::CanceledByUser(CanceledByUserError::new()));
        }

        let mut results = Vec::with_capacity(statements.len());
        for stmt in statements {
            if self.canceled.load(Ordering::SeqCst) {
                return Err(DbCoreError::CanceledByUser(CanceledByUserError::new()));
            }
            let res = execute_sqlite_statement(
                conn,
                &stmt.text,
                &stmt.statement_type,
                stmt.execution_type,
            )?;
            results.push(res);
        }
        Ok(results)
    }

    async fn cancel(&self) -> Result<(), DbCoreError> {
        self.canceled.store(true, Ordering::SeqCst);
        let guard = self
            .interrupt_handle
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        if let Some(ref handle) = *guard {
            handle.interrupt();
            Ok(())
        } else {
            Err(DbCoreError::QueryNotReadyToCancel)
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

    fn wrap_identifier(&self, value: &str) -> String {
        wrap_identifier_for_adapter("sqlite", value)
    }

    async fn list_databases(
        &self,
        _filter: Option<&DatabaseFilter>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.open_or_with_conn(|conn| {
            let mut stmt = conn
                .prepare("PRAGMA database_list;")
                .map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([], |row| {
                    let file: Option<String> = row.get(2).ok();
                    let val = file
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| ":memory:".to_string());
                    Ok(val)
                })
                .map_err(map_sqlite_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_sqlite_err)?);
            }
            Ok(out)
        })
    }

    async fn list_tables(
        &self,
        _filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListTableResult>, DbCoreError> {
        self.open_or_with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([], |row| {
                    let name: String = row.get(0)?;
                    Ok(ListTableResult { schema: None, name })
                })
                .map_err(map_sqlite_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_sqlite_err)?);
            }
            Ok(out)
        })
    }

    async fn list_views(
        &self,
        _filter: Option<&SchemaFilter>,
    ) -> Result<Vec<ListViewResult>, DbCoreError> {
        self.open_or_with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'view'")
                .map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([], |row| {
                    let name: String = row.get(0)?;
                    Ok(ListViewResult { schema: None, name })
                })
                .map_err(map_sqlite_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_sqlite_err)?);
            }
            Ok(out)
        })
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
        _schema: Option<&str>,
    ) -> Result<Vec<ListTableColumnsResult>, DbCoreError> {
        self.open_or_with_conn(|conn| {
            let sql = format!("PRAGMA table_info('{table}')");
            let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([], |row| {
                    let column_name: String = row.get(1)?;
                    let data_type: String = row.get(2)?;
                    Ok(ListTableColumnsResult {
                        column_name,
                        data_type,
                    })
                })
                .map_err(map_sqlite_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_sqlite_err)?);
            }
            Ok(out)
        })
    }

    async fn list_table_triggers(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.open_or_with_conn(|conn| {
            let sql = format!(
                "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = '{table}'"
            );
            let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(map_sqlite_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_sqlite_err)?);
            }
            Ok(out)
        })
    }

    async fn list_table_indexes(
        &self,
        table: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.open_or_with_conn(|conn| {
            let sql = format!("PRAGMA INDEX_LIST('{table}')");
            let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(map_sqlite_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_sqlite_err)?);
            }
            Ok(out)
        })
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

    fn query(&self, query_text: &str) -> Result<Arc<dyn CancellableQuery>, DbCoreError> {
        Ok(Arc::new(SqliteCancellableQuery {
            shared_conn: self.shared_conn.clone(),
            db_path: self.db_path.clone(),
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
            let mut results = Vec::with_capacity(statements.len());
            for stmt in statements {
                let res = execute_sqlite_statement(
                    conn,
                    &stmt.text,
                    &stmt.statement_type,
                    stmt.execution_type,
                )?;
                results.push(res);
            }
            Ok(results)
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
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.open_or_with_conn(|conn| {
            let sql = format!("SELECT sql FROM sqlite_master WHERE name = '{table}';");
            let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([], |row| {
                    let s: String = row.get(0)?;
                    Ok(append_semi_colon(&s))
                })
                .map_err(map_sqlite_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_sqlite_err)?);
            }
            Ok(out)
        })
    }

    async fn get_view_create_script(
        &self,
        view: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.open_or_with_conn(|conn| {
            let sql = format!("SELECT sql FROM sqlite_master WHERE name = '{view}';");
            let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([], |row| {
                    let s: String = row.get(0)?;
                    Ok(append_semi_colon(&s))
                })
                .map_err(map_sqlite_err)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r.map_err(map_sqlite_err)?);
            }
            Ok(out)
        })
    }

    async fn get_routine_create_script(
        &self,
        _routine: &str,
        _routine_type: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        Ok(Vec::new())
    }

    async fn truncate_all_tables(&self, schema: Option<&str>) -> Result<(), DbCoreError> {
        let tables = self.list_tables(None).await?;
        for t in tables {
            let _ = self
                .execute_query(&format!("DELETE FROM {};", t.name))
                .await?;
        }
        let _ = schema;
        Ok(())
    }
}
