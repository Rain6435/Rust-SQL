//! Web IPC handlers for database connection and schema/table metadata discovery events.

use crate::common::event::*;
use crate::common::types::{DatabaseFilter, SchemaFilter, Server};
use crate::web::{str_arg, IpcResponse, WebIpcState};
use serde_json::Value;

/// Handles database connection and metadata listing IPC events, returning `None` if `event` is unhandled.
pub async fn handle_db_metadata_ipc(
    state: &WebIpcState,
    event: &str,
    args: &[Value],
) -> Option<IpcResponse> {
    let resp = match event {
        DB_GET_CLIENTS_SYNC => {
            let db = state.db.lock().await;
            IpcResponse::success(db.get_clients_sync())
        }
        DB_CONNECT => {
            let Some(srv_val) = args.first() else {
                return Some(IpcResponse::err("Missing server"));
            };
            let srv: Server = match serde_json::from_value(srv_val.clone()) {
                Ok(s) => s,
                Err(e) => return Some(IpcResponse::err(e.to_string())),
            };
            let database = str_arg(args, 1);
            let mut db = state.db.lock().await;
            match db.connect(srv, database.as_deref()).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_DISCONNECT_DATABASE => {
            let database = str_arg(args, 0);
            let mut db = state.db.lock().await;
            match db.disconnect_database(database.as_deref()).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_DISCONNECT_SERVER => {
            let mut db = state.db.lock().await;
            match db.disconnect_server().await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_LIST_DATABASES => {
            let database = str_arg(args, 0).unwrap_or_default();
            let filter: Option<DatabaseFilter> = args
                .get(1)
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let db = state.db.lock().await;
            match db.list_databases(&database, filter.as_ref()).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_LIST_SCHEMAS => {
            let database = str_arg(args, 0).unwrap_or_default();
            let filter: Option<SchemaFilter> = args
                .get(1)
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let db = state.db.lock().await;
            match db.list_schemas(&database, filter.as_ref()).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_LIST_TABLES => {
            let database = str_arg(args, 0).unwrap_or_default();
            let filter: Option<SchemaFilter> = args
                .get(1)
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let db = state.db.lock().await;
            match db.list_tables(&database, filter.as_ref()).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_LIST_VIEWS => {
            let database = str_arg(args, 0).unwrap_or_default();
            let filter: Option<SchemaFilter> = args
                .get(1)
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let db = state.db.lock().await;
            match db.list_views(&database, filter.as_ref()).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_LIST_ROUTINES => {
            let database = str_arg(args, 0).unwrap_or_default();
            let filter: Option<SchemaFilter> = args
                .get(1)
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let db = state.db.lock().await;
            match db.list_routines(&database, filter.as_ref()).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_LIST_TABLE_COLUMNS => {
            let (database, table, schema) = (
                str_arg(args, 0).unwrap_or_default(),
                str_arg(args, 1).unwrap_or_default(),
                str_arg(args, 2),
            );
            let db = state.db.lock().await;
            match db
                .list_table_columns(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_LIST_TABLE_TRIGGERS => {
            let (database, table, schema) = (
                str_arg(args, 0).unwrap_or_default(),
                str_arg(args, 1).unwrap_or_default(),
                str_arg(args, 2),
            );
            let db = state.db.lock().await;
            match db
                .list_table_triggers(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_LIST_TABLE_INDEXES => {
            let (database, table, schema) = (
                str_arg(args, 0).unwrap_or_default(),
                str_arg(args, 1).unwrap_or_default(),
                str_arg(args, 2),
            );
            let db = state.db.lock().await;
            match db
                .list_table_indexes(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_TABLE_REFERENCES => {
            let (database, table, schema) = (
                str_arg(args, 0).unwrap_or_default(),
                str_arg(args, 1).unwrap_or_default(),
                str_arg(args, 2),
            );
            let db = state.db.lock().await;
            match db
                .get_table_references(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_TABLE_KEYS => {
            let (database, table, schema) = (
                str_arg(args, 0).unwrap_or_default(),
                str_arg(args, 1).unwrap_or_default(),
                str_arg(args, 2),
            );
            let db = state.db.lock().await;
            match db
                .get_table_keys(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        _ => return None,
    };
    Some(resp)
}
