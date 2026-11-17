//! Emulator-only metadata helpers for schemas, tables, views, routines, columns,
//! triggers, indexes, foreign key references, and primary/foreign key constraints.

use crate::common::types::{
    ListRoutineResult, ListTableColumnsResult, ListTableResult, ListViewResult, SchemaFilter,
    TableKeysResult,
};
use crate::core::errors::DbCoreError;
use crate::core::filters::filter_schema_name;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

/// Lists schemas for `adapter_key`, applying `filter`.
pub fn list_schemas_for_adapter(
    adapter_key: &str,
    default_schema: &str,
    filter: Option<&SchemaFilter>,
) -> Result<Vec<String>, DbCoreError> {
    if !matches!(adapter_key, "postgresql" | "redshift" | "sqlserver") {
        return Ok(Vec::new());
    }
    let mut out: Vec<String> = [default_schema, "dummy_schema", "information_schema"]
        .into_iter()
        .filter(|s| filter_schema_name(Some(s), filter))
        .map(String::from)
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

fn query_master_names(
    emulated_conn: &Arc<Mutex<Connection>>,
    obj_type: &str,
) -> Result<Vec<String>, DbCoreError> {
    let conn = emulated_conn
        .lock()
        .map_err(|e| DbCoreError::Message(e.to_string()))?;
    let sql = format!(
        "SELECT name FROM sqlite_master WHERE type='{obj_type}' AND name NOT LIKE 'sqlite_%' ORDER BY name"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| DbCoreError::Message(e.to_string()))?;
    let rows = stmt
        .query_map([], |r| r.get(0))
        .map_err(|e| DbCoreError::Message(e.to_string()))?;
    Ok(rows.filter_map(Result::ok).collect())
}

fn schema_for_adapter(adapter_key: &str, default_schema: &str) -> Option<String> {
    if matches!(adapter_key, "postgresql" | "redshift" | "sqlserver") {
        Some(default_schema.to_string())
    } else {
        None
    }
}

/// Lists user tables for `adapter_key`, applying `filter`.
pub fn list_tables_for_adapter(
    adapter_key: &str,
    default_schema: &str,
    emulated_conn: &Arc<Mutex<Connection>>,
    filter: Option<&SchemaFilter>,
) -> Result<Vec<ListTableResult>, DbCoreError> {
    let schema = schema_for_adapter(adapter_key, default_schema);
    if !filter_schema_name(schema.as_deref(), filter) {
        return Ok(Vec::new());
    }
    Ok(query_master_names(emulated_conn, "table")?
        .into_iter()
        .map(|name| ListTableResult {
            schema: schema.clone(),
            name,
        })
        .collect())
}

/// Lists views for `adapter_key`, applying `filter`.
pub fn list_views_for_adapter(
    adapter_key: &str,
    default_schema: &str,
    emulated_conn: &Arc<Mutex<Connection>>,
    filter: Option<&SchemaFilter>,
) -> Result<Vec<ListViewResult>, DbCoreError> {
    if adapter_key == "cassandra" {
        return Ok(Vec::new());
    }
    let schema = schema_for_adapter(adapter_key, default_schema);
    if !filter_schema_name(schema.as_deref(), filter) {
        return Ok(Vec::new());
    }
    Ok(query_master_names(emulated_conn, "view")?
        .into_iter()
        .map(|name| ListViewResult {
            schema: schema.clone(),
            name,
        })
        .collect())
}

/// Lists stored functions and procedures for `adapter_key`.
pub fn list_routines_for_adapter(
    adapter_key: &str,
    default_schema: &str,
    _filter: Option<&SchemaFilter>,
) -> Result<Vec<ListRoutineResult>, DbCoreError> {
    let s = Some(default_schema.to_string());
    let item = |schema: Option<String>, name: &str, rtype: &str| ListRoutineResult {
        schema,
        routine_name: name.to_string(),
        routine_type: rtype.to_string(),
    };
    match adapter_key {
        "postgresql" => Ok(vec![
            item(s.clone(), "dummy_trigger_function", "FUNCTION"),
            item(s, "users_count", "FUNCTION"),
        ]),
        "redshift" => Ok(vec![item(s, "users_count", "FUNCTION")]),
        "mysql" | "mariadb" => Ok(vec![item(None, "users_count", "PROCEDURE")]),
        "sqlserver" => Ok(vec![item(s, "users_count", "PROCEDURE")]),
        _ => Ok(Vec::new()),
    }
}

/// Lists columns and their dialect-specific data types for `table`.
pub fn list_columns_for_adapter(
    adapter_key: &str,
    emulated_conn: &Arc<Mutex<Connection>>,
    table: &str,
) -> Result<Vec<ListTableColumnsResult>, DbCoreError> {
    if table == "users" {
        let cols: &[(&str, &str)] = match adapter_key {
            "postgresql" | "redshift" => &[
                ("id", "integer"),
                ("username", "text"),
                ("email", "text"),
                ("password", "text"),
                ("role_id", "integer"),
                ("createdat", "date"),
            ],
            "cassandra" => &[
                ("id", "int"),
                ("createdat", "timestamp"),
                ("email", "text"),
                ("password", "text"),
                ("role_id", "int"),
                ("username", "text"),
            ],
            _ => &[
                ("id", "int"),
                ("username", "varchar"),
                ("email", "varchar"),
                ("password", "varchar"),
                ("role_id", "int"),
                ("createdat", "datetime"),
            ],
        };
        return Ok(cols
            .iter()
            .map(|(c, t)| ListTableColumnsResult {
                column_name: (*c).to_string(),
                data_type: (*t).to_string(),
            })
            .collect());
    }

    let conn = emulated_conn
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

/// Lists triggers attached to `table` for `adapter_key`.
pub fn list_triggers_for_adapter(
    adapter_key: &str,
    table: &str,
) -> Result<Vec<String>, DbCoreError> {
    if !matches!(adapter_key, "cassandra" | "redshift") && table == "users" {
        return Ok(vec!["dummy_trigger".to_string()]);
    }
    Ok(Vec::new())
}

/// Lists indexes defined on `table` for `adapter_key`.
pub fn list_indexes_for_adapter(
    adapter_key: &str,
    table: &str,
) -> Result<Vec<String>, DbCoreError> {
    if table != "users" {
        return Ok(Vec::new());
    }
    match adapter_key {
        "cassandra" => Ok(Vec::new()),
        "postgresql" | "redshift" => Ok(vec!["users_pkey".to_string()]),
        "mysql" | "mariadb" => Ok(vec!["PRIMARY".to_string(), "role_id".to_string()]),
        "sqlserver" => Ok(vec!["PK__users__3213E83F00000001".to_string()]),
        _ => Ok(vec!["users_id_index".to_string()]),
    }
}

/// Lists foreign-key referenced table names for `table`.
pub fn get_references_for_adapter(
    adapter_key: &str,
    table: &str,
) -> Result<Vec<String>, DbCoreError> {
    if !matches!(adapter_key, "cassandra" | "sqlite") && table == "users" {
        return Ok(vec!["roles".to_string()]);
    }
    Ok(Vec::new())
}

/// Lists primary and foreign key constraints for `table`.
pub fn get_keys_for_adapter(
    adapter_key: &str,
    table: &str,
) -> Result<Vec<TableKeysResult>, DbCoreError> {
    if table != "users" || adapter_key == "sqlite" {
        return Ok(Vec::new());
    }
    let key = |col: &str, ktype: &str, cname: Option<&str>, rtable: Option<&str>| TableKeysResult {
        column_name: col.to_string(),
        key_type: ktype.to_string(),
        constraint_name: cname.map(String::from),
        referenced_table: rtable.map(String::from),
    };
    match adapter_key {
        "cassandra" => Ok(vec![key("id", "PRIMARY KEY", None, None)]),
        "mysql" | "mariadb" => Ok(vec![
            key("id", "PRIMARY KEY", Some("PRIMARY KEY"), None),
            key(
                "role_id",
                "FOREIGN KEY",
                Some("users_ibfk_1 KEY"),
                Some("roles"),
            ),
        ]),
        _ => Ok(vec![
            key("id", "PRIMARY KEY", Some("users_pkey"), None),
            key(
                "role_id",
                "FOREIGN KEY",
                Some("users_role_id_fkey"),
                Some("roles"),
            ),
        ]),
    }
}
