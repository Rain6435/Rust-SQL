//! Web IPC handlers for database connection and schema/table metadata discovery events.
//!
//! `DB_CONNECT` takes a saved connection ID (never a full profile with credentials), checks the
//! caller's grant, and decrypts the saved credentials on the server. Every metadata event
//! re-checks the grant for the session's open connection; a refusal is a 403 and is audited.

use crate::adapters::DatabaseFacade;
use crate::common::event::*;
use crate::common::types::{ConnectionMode, DatabaseFilter, SchemaFilter, EMULATED_BANNER};
use crate::core::servers;
use crate::web::{str_arg, IpcCtx, IpcResponse};
use serde_json::{json, Value};

/// Builds the `{ mode, banner, statementTimeoutSecs }` payload so web clients can show the
/// EMULATED warning and the connection's statement timeout.
fn mode_response(db: &DatabaseFacade, database: &str) -> IpcResponse {
    let timeout = db
        .server_session
        .as_ref()
        .and_then(|s| s.config.statement_timeout_secs)
        .filter(|s| *s > 0);
    match db.connection_mode(database) {
        Ok(mode) => IpcResponse::success(json!({
            "mode": mode,
            "banner": (mode == ConnectionMode::Emulated).then_some(EMULATED_BANNER),
            "statementTimeoutSecs": timeout,
        })),
        Err(e) => IpcResponse::err(e),
    }
}

/// Metadata events that read from the open connection and therefore need a grant.
const CONNECTION_EVENTS: &[&str] = &[
    DB_GET_CONNECTION_MODE,
    DB_LIST_DATABASES,
    DB_LIST_SCHEMAS,
    DB_LIST_TABLES,
    DB_LIST_VIEWS,
    DB_LIST_ROUTINES,
    DB_LIST_TABLE_COLUMNS,
    DB_LIST_TABLE_TRIGGERS,
    DB_LIST_TABLE_INDEXES,
    DB_GET_TABLE_REFERENCES,
    DB_GET_TABLE_KEYS,
];

async fn connect(ctx: &IpcCtx<'_>, args: &[Value]) -> IpcResponse {
    let Some(id) = str_arg(args, 0).filter(|s| !s.is_empty()) else {
        return IpcResponse::err("DB_CONNECT takes a saved connection ID");
    };
    let database = str_arg(args, 1);
    if let Err(r) = ctx.require_grant(&id) {
        return r;
    }
    let saved = match servers::get_all_sync() {
        Ok(all) => all.into_iter().find(|s| s.id == id),
        Err(e) => return IpcResponse::err(e),
    };
    let Some(saved) = saved else {
        return IpcResponse::not_found("Saved connection not found");
    };
    let srv = match servers::decrypt_secrets(&saved, &*ctx.app.keys) {
        Ok(s) => s,
        Err(e) => return IpcResponse::err(e),
    };
    let mut db = ctx.session.db.lock().await;
    if ctx.session.connected_server().is_some_and(|cur| cur != id) {
        let _ = db.disconnect_server().await;
        ctx.session.set_connected_server(None);
    }
    db.keys = crate::core::keys::SharedKeyProvider(ctx.app.keys.clone());
    match db.connect_checked(srv, database.as_deref()).await {
        Ok(()) => {
            ctx.session.set_connected_server(Some(id));
            mode_response(&db, database.as_deref().unwrap_or("undefined"))
        }
        Err(e) => crate::web::ipc_ssh_host_keys::connect_error_response(ctx.session, e).await,
    }
}

/// Handles database connection and metadata listing IPC events, returning `None` if `event` is unhandled.
pub async fn handle_db_metadata_ipc(
    ctx: &IpcCtx<'_>,
    event: &str,
    args: &[Value],
) -> Option<IpcResponse> {
    if CONNECTION_EVENTS.contains(&event) {
        if let Err(r) = ctx.require_connected() {
            return Some(r);
        }
    }
    let state = ctx.session;
    let resp = match event {
        DB_GET_CLIENTS_SYNC => {
            let db = state.db.lock().await;
            IpcResponse::success(db.get_clients_sync())
        }
        DB_CONNECT => connect(ctx, args).await,
        DB_GET_CONNECTION_MODE => {
            let database = str_arg(args, 0).unwrap_or_default();
            let db = state.db.lock().await;
            mode_response(&db, &database)
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
                Ok(()) => {
                    state.set_connected_server(None);
                    IpcResponse::success(Value::Null)
                }
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
