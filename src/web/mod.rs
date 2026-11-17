use crate::adapters::DatabaseFacade;
use crate::common::event::*;
use crate::common::types::{BaseConfig, ConfigFile, DatabaseFilter, SchemaFilter, Server};
use crate::core::config::{self, DEFAULT_CRYPTO_SECRET};
use crate::core::export::write_clipboard_text;
use crate::core::servers;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default)]
pub struct WebIpcState {
    pub db: Mutex<DatabaseFacade>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    pub event: String,
    #[serde(default)]
    pub args: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn success<T: Serialize>(val: T) -> Self {
        Self {
            ok: true,
            data: serde_json::to_value(val).ok(),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

/// Handles all IPC events registered in `src/browser/ipcMain.ts`.
pub async fn dispatch_ipc_event(state: &WebIpcState, req: IpcRequest) -> IpcResponse {
    let args = &req.args;
    let str_arg = |idx: usize| -> Option<String> {
        args.get(idx)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    match req.event.as_str() {
        // 1. Config IPC handlers
        CONFIG_PREPARE => {
            let secret = str_arg(0).unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string());
            match config::prepare(&secret).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        CONFIG_PATH => IpcResponse::success(config::path()),
        CONFIG_GET => match config::get().await {
            Ok(cfg) => IpcResponse::success(cfg),
            Err(e) => IpcResponse::err(e),
        },
        CONFIG_GET_FULL | CONFIG_GET_FULL_SYNC => {
            let clean = args.first().and_then(|v| v.as_bool()).unwrap_or(false);
            match config::get_full_sync(clean) {
                Ok(cfg) => IpcResponse::success(cfg),
                Err(e) => IpcResponse::err(e),
            }
        }
        CONFIG_SAVE => {
            let Some(val) = args.first() else {
                return IpcResponse::err("Missing config data");
            };
            let parsed: ConfigFile = match serde_json::from_value(val.clone()) {
                Ok(c) => c,
                Err(e) => return IpcResponse::err(e.to_string()),
            };
            match config::save(&parsed).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        CONFIG_SAVE_SETTINS => {
            let Some(val) = args.first() else {
                return IpcResponse::err("Missing settings data");
            };
            let parsed: BaseConfig = match serde_json::from_value(val.clone()) {
                Ok(c) => c,
                Err(e) => return IpcResponse::err(e.to_string()),
            };
            match config::save_settings(&parsed).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }

        // 2. Servers IPC handlers
        SERVERS_GET_ALL => match servers::get_all().await {
            Ok(list) => IpcResponse::success(list),
            Err(e) => IpcResponse::err(e),
        },
        SERVERS_ADD => {
            let Some(srv_val) = args.first() else {
                return IpcResponse::err("Missing server");
            };
            let srv: Server = match serde_json::from_value(srv_val.clone()) {
                Ok(s) => s,
                Err(e) => return IpcResponse::err(e.to_string()),
            };
            let secret = str_arg(1).unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string());
            match servers::add(&srv, &secret).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        SERVERS_UPDATE => {
            let Some(srv_val) = args.first() else {
                return IpcResponse::err("Missing server");
            };
            let srv: Server = match serde_json::from_value(srv_val.clone()) {
                Ok(s) => s,
                Err(e) => return IpcResponse::err(e.to_string()),
            };
            let secret = str_arg(1).unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string());
            match servers::update(&srv, &secret).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        SERVERS_ADD_OR_UPDATE => {
            let Some(srv_val) = args.first() else {
                return IpcResponse::err("Missing server");
            };
            let srv: Server = match serde_json::from_value(srv_val.clone()) {
                Ok(s) => s,
                Err(e) => return IpcResponse::err(e.to_string()),
            };
            let secret = str_arg(1).unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string());
            match servers::add_or_update(&srv, &secret).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        SERVERS_REMOVE_BY_ID => {
            let id = str_arg(0).unwrap_or_default();
            match servers::remove_by_id(&id).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        SERVERS_DECRYPT_SECRETS => {
            let Some(srv_val) = args.first() else {
                return IpcResponse::err("Missing server");
            };
            let srv: Server = match serde_json::from_value(srv_val.clone()) {
                Ok(s) => s,
                Err(e) => return IpcResponse::err(e.to_string()),
            };
            let secret = str_arg(1).unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string());
            match servers::decrypt_secrets(&srv, &secret) {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }

        // 3. DB IPC handlers
        DB_GET_CLIENTS_SYNC => {
            let db = state.db.lock().await;
            IpcResponse::success(db.get_clients_sync())
        }
        DB_CONNECT => {
            let Some(srv_val) = args.first() else {
                return IpcResponse::err("Missing server");
            };
            let srv: Server = match serde_json::from_value(srv_val.clone()) {
                Ok(s) => s,
                Err(e) => return IpcResponse::err(e.to_string()),
            };
            let database = str_arg(1);
            let mut db = state.db.lock().await;
            match db.connect(srv, database.as_deref()).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_DISCONNECT_DATABASE => {
            let database = str_arg(0);
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
            let database = str_arg(0).unwrap_or_default();
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
            let database = str_arg(0).unwrap_or_default();
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
            let database = str_arg(0).unwrap_or_default();
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
            let database = str_arg(0).unwrap_or_default();
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
            let database = str_arg(0).unwrap_or_default();
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
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
            let db = state.db.lock().await;
            match db
                .get_table_keys(&database, &table, schema.as_deref())
                .await
            {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_CREATE_CANCELLABLE_QUERY => {
            let database = str_arg(0).unwrap_or_default();
            let query_id = args.get(1).and_then(|v| v.as_u64()).unwrap_or(1);
            let query_text = str_arg(2).unwrap_or_default();
            let mut db = state.db.lock().await;
            match db.create_cancellable_query(&database, query_id, &query_text) {
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
            match db.execute_cancellable_query(query_id).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_EXECUTE_QUERY => {
            let database = str_arg(0).unwrap_or_default();
            let query_text = str_arg(1).unwrap_or_default();
            let db = state.db.lock().await;
            match db.execute_query(&database, &query_text).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_QUERY_SELECT_TOP => {
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let view = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let database = str_arg(0).unwrap_or_default();
            let routine = str_arg(1).unwrap_or_default();
            let r_type = str_arg(2).unwrap_or_default();
            let schema = str_arg(3);
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
            let database = str_arg(0).unwrap_or_default();
            let schema = str_arg(1);
            let db = state.db.lock().await;
            match db.truncate_all_tables(&database, schema.as_deref()).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_GET_TABLE_COLUMN_NAMES => {
            let database = str_arg(0).unwrap_or_default();
            let table = str_arg(1).unwrap_or_default();
            let schema = str_arg(2);
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
            let export_type = str_arg(1).unwrap_or_else(|| "CSV".to_string());
            let delimiter = str_arg(2);
            let db = state.db.lock().await;
            match db.export_query_result_to_clipboard(&rows, &export_type, delimiter.as_deref()) {
                Ok(txt) => IpcResponse::success(txt),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_EXPORT_QUERY_RESULT_TO_FILE => {
            let rows: Vec<Map<String, Value>> = args
                .first()
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let export_type = str_arg(1).unwrap_or_else(|| "CSV".to_string());
            let delimiter = str_arg(2);
            let filename = str_arg(3).unwrap_or_else(|| "/tmp/rust_sql_export".to_string());
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
            let filename = str_arg(1).unwrap_or_else(|| "/tmp/query.sql".to_string());
            let query = str_arg(2).unwrap_or_default();
            let db = state.db.lock().await;
            match db.save_query(&filename, &query) {
                Ok(info) => IpcResponse::success(info),
                Err(e) => IpcResponse::err(e),
            }
        }
        DB_QUERY_OPEN => {
            let filename = str_arg(0).unwrap_or_default();
            let db = state.db.lock().await;
            match db.open_query(&filename) {
                Ok(info) => IpcResponse::success(info),
                Err(e) => IpcResponse::err(e),
            }
        }
        BROWSER_CLIPBOARD_WRITE => {
            let text = str_arg(0).unwrap_or_default();
            write_clipboard_text(&text);
            IpcResponse::success(Value::Null)
        }
        LOGGER_ERROR => IpcResponse::success(Value::Null),
        other => IpcResponse::err(format!("Unknown IPC event: {other}")),
    }
}

async fn handle_ipc(
    State(state): State<Arc<WebIpcState>>,
    Json(req): Json<IpcRequest>,
) -> impl IntoResponse {
    let resp = dispatch_ipc_event(&state, req).await;
    let status = if resp.ok {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, Json(resp))
}

async fn handle_index() -> Html<&'static str> {
    Html(RUST_SQL_WEB_HTML)
}

async fn handle_health() -> Json<Value> {
    Json(json!({
        "name": "Rust-SQL",
        "version": env!("CARGO_PKG_VERSION"),
        "status": "ok"
    }))
}

pub fn build_web_router(state: Arc<WebIpcState>) -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/api/health", get(handle_health))
        .route("/api/ipc", post(handle_ipc))
        .with_state(state)
}

pub async fn run_web_server(port: u16) -> Result<(), String> {
    let state = Arc::new(WebIpcState::default());
    let app = build_web_router(state);
    let addr = format!("127.0.0.1:{port}");
    println!("Rust-SQL Web GUI listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| e.to_string())?;
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

pub const RUST_SQL_WEB_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <title>Rust-SQL</title>
  <style>
    * { box-sizing: border-box; font-family: 'Lato', 'Helvetica Neue', Arial, sans-serif; }
    body { margin: 0; background: #f8fafc; color: #0f172a; display: flex; flex-direction: column; height: 100vh; }
    body.dark-theme { background: #0f172a; color: #f8fafc; }
    header { height: 50px; background: #1e293b; color: #fff; display: flex; align-items: center; justify-content: space-between; padding: 0 16px; }
    footer { height: 32px; background: #e2e8f0; display: flex; align-items: center; justify-content: space-between; padding: 0 16px; font-size: 12px; }
    #main-area { flex: 1; display: flex; overflow: hidden; }
    #server-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 16px; padding: 16px; width: 100%; align-content: start; }
    .server-card { background: #fff; border: 1px solid #cbd5e1; border-radius: 6px; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
    #sidebar { width: 250px; border-right: 1px solid #cbd5e1; padding: 12px; overflow-y: auto; background: #fff; }
    #workspace { flex: 1; display: flex; flex-direction: column; padding: 12px; gap: 12px; overflow: hidden; }
    textarea.query-editor { width: 100%; height: 150px; font-family: monospace; font-size: 13px; padding: 8px; border: 1px solid #cbd5e1; border-radius: 4px; }
    .grid-query-wrapper { flex: 1; overflow: auto; border: 1px solid #cbd5e1; background: #fff; }
    table { width: 100%; border-collapse: collapse; font-size: 13px; }
    th, td { border: 1px solid #e2e8f0; padding: 6px 10px; text-align: left; }
    th { background: #f1f5f9; font-weight: bold; }
    button { cursor: pointer; padding: 6px 12px; border-radius: 4px; border: 1px solid #94a3b8; background: #f8fafc; }
    button.primary { background: #16a34a; color: white; border-color: #15803d; }
  </style>
</head>
<body>
  <header>
    <div><strong>Rust-SQL</strong></div>
    <div id="breadcrumb">servers</div>
    <div id="conn-actions"></div>
  </header>
  <div id="main-area">
    <div id="server-list"></div>
  </div>
  <footer>
    <span id="status-bar">Ready</span>
    <span>Rust-SQL v1.39.0</span>
  </footer>
  <script>
    async function ipc(event, ...args) {
      const res = await fetch('/api/ipc', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ event, args })
      });
      const json = await res.json();
      if (!json.ok) throw new Error(json.error || 'IPC Error');
      return json.data;
    }

    window.rustSql = { ipc };

    async function loadServers() {
      const cfg = await ipc('CONFIG_GET_FULL', true);
      const container = document.getElementById('main-area');
      container.innerHTML = '<div id="server-list"></div>';
      const list = document.getElementById('server-list');
      for (const srv of cfg.servers) {
        const meta = srv.host ? `${srv.host}:${srv.port || ''}` : (srv.socketPath || srv.database || '');
        const card = document.createElement('div');
        card.className = 'server-card';
        card.innerHTML = `
          <div data-testid="server-name" style="font-weight:bold">${srv.name}</div>
          <div data-testid="server-meta" style="color:#64748b;font-size:12px">${meta}</div>
          <button class="primary" data-testid="connect-button">Connect</button>
        `;
        card.querySelector('[data-testid="connect-button"]').onclick = () => connectServer(srv, cfg.crypto.secret);
        list.appendChild(card);
      }
    }

    async function connectServer(srv, secret) {
      const dec = await ipc('SERVERS_DECRYPT_SECRETS', srv, secret);
      const db = dec.database || ':memory:';
      await ipc('DB_CONNECT', dec, db);
      const tables = await ipc('DB_LIST_TABLES', db);
      const container = document.getElementById('main-area');
      container.innerHTML = `
        <div id="sidebar">
          <div data-testid="db-header" style="font-weight:bold;margin-bottom:8px">${db}</div>
          <div id="table-items"></div>
        </div>
        <div id="workspace" class="react-tabs__tab-panel react-tabs__tab-panel--selected">
          <div id="querybox1"><div contenteditable="true" id="editor-box" style="border:1px solid #ccc;padding:8px;min-height:80px;font-family:monospace"></div></div>
          <div><button class="primary" id="exec-btn">Execute</button></div>
          <div id="query-result"></div>
        </div>
      `;
      const tblContainer = document.getElementById('table-items');
      for (const t of tables) {
        const item = document.createElement('div');
        item.setAttribute('data-testid', 'db-item-Table');
        item.style.cursor = 'pointer';
        item.style.padding = '4px 0';
        item.innerHTML = `<span>${t.name}</span>`;
        item.querySelector('span').onclick = async () => {
          const sql = await ipc('DB_GET_QUERY_SELECT_TOP', db, t.name);
          document.getElementById('editor-box').innerText = sql;
          await runQuery(db, sql);
        };
        tblContainer.appendChild(item);
      }
      document.getElementById('exec-btn').onclick = () => {
        runQuery(db, document.getElementById('editor-box').innerText);
      };
    }

    async function runQuery(db, sql) {
      const results = await ipc('DB_EXECUTE_QUERY', db, sql);
      const resDiv = document.getElementById('query-result');
      resDiv.innerHTML = '';
      for (const r of results) {
        const wrap = document.createElement('div');
        wrap.className = 'grid-query-wrapper';
        for (const f of r.fields) {
          const cell = document.createElement('div');
          cell.className = 'ReactVirtualized__Grid__cell';
          cell.style.fontWeight = 'bold';
          cell.innerText = f.name;
          wrap.appendChild(cell);
        }
        for (const row of r.rows) {
          for (const f of r.fields) {
            const cell = document.createElement('div');
            cell.className = 'ReactVirtualized__Grid__cell';
            cell.innerText = row[f.name] === null ? 'NULL' : String(row[f.name]);
            wrap.appendChild(cell);
          }
        }
        resDiv.appendChild(wrap);
      }
    }

    loadServers();
  </script>
</body>
</html>
"#;
