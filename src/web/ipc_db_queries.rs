//! Web IPC handlers for query execution, script generation, and exports.
//!
//! `DB_EXECUTE_QUERY` and `DB_CREATE_CANCELLABLE_QUERY` take an optional JSON object of typed
//! parameter values, bound through the driver's prepared statements.
//!
//! Every event that touches the open connection re-checks the caller's grant. Statement
//! execution is audited without the query text. Events that read or write the server's file
//! system or change process-wide limits require an administrator, and exports never touch
//! the server's clipboard.

use crate::common::event::*;
use crate::core::export::export_query_result_to_string;
use crate::web::ipc_policy::{QUERY_ADMIN_EVENTS, QUERY_CONNECTION_EVENTS};
use crate::web::query_audit::{audit_query_results, audit_statement};
use crate::web::{params_arg, str_arg, IpcCtx, IpcResponse};
use serde_json::{Map, Value};

fn db_table_schema(args: &[Value]) -> (String, String, Option<String>) {
    (
        str_arg(args, 0).unwrap_or_default(),
        str_arg(args, 1).unwrap_or_default(),
        str_arg(args, 2),
    )
}

/// Handles query execution, script generation, and export IPC events, returning `None` if unhandled.
pub async fn handle_db_queries_ipc(
    ctx: &IpcCtx<'_>,
    event: &str,
    args: &[Value],
) -> Option<IpcResponse> {
    let mut conn_id = String::new();
    if QUERY_CONNECTION_EVENTS.contains(&event) {
        match ctx.require_connected() {
            Ok(id) => conn_id = id,
            Err(r) => return Some(r),
        }
    }
    if QUERY_ADMIN_EVENTS.contains(&event) {
        if let Err(r) = ctx.require_admin(event) {
            return Some(r);
        }
    }
    let state = ctx.session;
    let resp = match event {
        DB_CREATE_CANCELLABLE_QUERY => {
            let database = str_arg(args, 0).unwrap_or_default();
            let query_id = args.get(1).and_then(|v| v.as_u64()).unwrap_or(1);
            let query_text = str_arg(args, 2).unwrap_or_default();
            let params = match params_arg(args, 3) {
                Ok(p) => p,
                Err(r) => return Some(r),
            };
            let mut db = state.db.lock().await;
            match db.create_cancellable_query_with_params(&database, query_id, &query_text, &params)
            {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_CANCEL_CANCELLABLE_QUERY => {
            let query_id = args.first().and_then(|v| v.as_u64()).unwrap_or(1);
            let mut db = state.db.lock().await;
            match db.cancel_cancellable_query(query_id).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_EXECUTE_CANCELLABLE_QUERY => {
            let query_id = args.first().and_then(|v| v.as_u64()).unwrap_or(1);
            let mut db = state.db.lock().await;
            let result = db.execute_cancellable_query(query_id).await;
            audit_query_results(ctx, &conn_id, &result);
            match result {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_EXECUTE_QUERY => {
            let database = str_arg(args, 0).unwrap_or_default();
            let query_text = str_arg(args, 1).unwrap_or_default();
            let params = match params_arg(args, 2) {
                Ok(p) => p,
                Err(r) => return Some(r),
            };
            let db = state.db.lock().await;
            let result = db
                .execute_query_with_params(&database, &query_text, &params)
                .await;
            audit_query_results(ctx, &conn_id, &result);
            match result {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_QUERY_SELECT_TOP => {
            let (database, table, schema) = db_table_schema(args);
            let limit = args.get(3).and_then(|v| v.as_u64()).map(|n| n as u32);
            let db = state.db.lock().await;
            match db
                .get_query_select_top(&database, &table, schema.as_deref(), limit)
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_TABLE_CREATE_SCRIPT => {
            let (database, table, schema) = db_table_schema(args);
            let db = state.db.lock().await;
            match db
                .get_table_create_script(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_TABLE_SELECT_SCRIPT => {
            let (database, table, schema) = db_table_schema(args);
            let db = state.db.lock().await;
            match db
                .get_table_select_script(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_TABLE_INSERT_SCRIPT => {
            let (database, table, schema) = db_table_schema(args);
            let db = state.db.lock().await;
            match db
                .get_table_insert_script(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_TABLE_UPDATE_SCRIPT => {
            let (database, table, schema) = db_table_schema(args);
            let db = state.db.lock().await;
            match db
                .get_table_update_script(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_TABLE_DELETE_SCRIPT => {
            let (database, table, schema) = db_table_schema(args);
            let db = state.db.lock().await;
            match db
                .get_table_delete_script(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_VIEW_CREATE_SCRIPT => {
            let (database, view, schema) = db_table_schema(args);
            let db = state.db.lock().await;
            match db
                .get_view_create_script(&database, &view, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_ROUTINE_CREATE_SCRIPT => {
            let (database, routine) = (
                str_arg(args, 0).unwrap_or_default(),
                str_arg(args, 1).unwrap_or_default(),
            );
            let r_type = str_arg(args, 2).unwrap_or_default();
            let schema = str_arg(args, 3);
            let db = state.db.lock().await;
            match db
                .get_routine_create_script(&database, &routine, &r_type, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_TRUNCATE_ALL_TABLES => {
            let database = str_arg(args, 0).unwrap_or_default();
            let schema = str_arg(args, 1);
            let db = state.db.lock().await;
            let result = db.truncate_all_tables(&database, schema.as_deref()).await;
            audit_statement(ctx, &conn_id, "TRUNCATE_ALL_TABLES", result.is_ok());
            match result {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_TABLE_COLUMN_NAMES => {
            let (database, table, schema) = db_table_schema(args);
            let db = state.db.lock().await;
            match db
                .get_table_column_names(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_SET_SELECT_LIMIT => {
            let limit = args.first().and_then(|v| v.as_u64()).unwrap_or(1000) as u32;
            let db = state.db.lock().await;
            db.set_select_limit(limit);
            IpcResponse::success(Value::Null)
        }
        DB_EXPORT_QUERY_RESULT_TO_CLIPBOARD => {
            let rows: Vec<Map<String, Value>> = args
                .first()
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let export_type = str_arg(args, 1).unwrap_or_else(|| "CSV".to_string());
            let delimiter = str_arg(args, 2);
            match export_query_result_to_string(&rows, &export_type, delimiter.as_deref()) {
                Ok(txt) => IpcResponse::success(txt),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_EXPORT_QUERY_RESULT_TO_FILE => {
            let rows: Vec<Map<String, Value>> = args
                .first()
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let export_type = str_arg(args, 1).unwrap_or_else(|| "CSV".to_string());
            let delimiter = str_arg(args, 2);
            let filename = str_arg(args, 3).unwrap_or_else(|| "/tmp/rust_sql_export".to_string());
            let db = state.db.lock().await;
            match db.export_query_result_to_file(
                &rows,
                &export_type,
                delimiter.as_deref(),
                &filename,
            ) {
                Ok(path) => IpcResponse::success(path),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_QUERY_SAVE => {
            let filename = str_arg(args, 1).unwrap_or_else(|| "/tmp/query.sql".to_string());
            let query = str_arg(args, 2).unwrap_or_default();
            let db = state.db.lock().await;
            match db.save_query(&filename, &query) {
                Ok(info) => IpcResponse::success(info),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_QUERY_OPEN => {
            let filename = str_arg(args, 0).unwrap_or_default();
            let db = state.db.lock().await;
            match db.open_query(&filename) {
                Ok(info) => IpcResponse::success(info),
                Err(e) => IpcResponse::err(e),
            }
        }
        _ => return None,
    };
    Some(resp)
}
