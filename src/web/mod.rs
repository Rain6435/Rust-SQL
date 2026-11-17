//! Embedded HTTP/IPC web server (`--web`) powered by `axum`.

pub mod assets;
pub mod ipc_config_servers;
pub mod ipc_db_metadata;
pub mod ipc_db_queries;
pub mod ipc_ssh_host_keys;

pub use assets::RUST_SQL_WEB_HTML;

use crate::adapters::DatabaseFacade;
use crate::common::event::{BROWSER_CLIPBOARD_WRITE, LOGGER_ERROR};
use crate::common::types::HostKeyChallenge;
use crate::core::export::write_clipboard_text;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state held by the embedded `axum` web server.
#[derive(Default)]
pub struct WebIpcState {
    /// Mutex-protected [`DatabaseFacade`] handling all IPC database operations.
    pub db: Mutex<DatabaseFacade>,
    /// Host-key challenges issued by `DB_CONNECT`; only these can be approved.
    pub pending_host_keys: Mutex<Vec<HostKeyChallenge>>,
}

/// JSON payload for an incoming `/api/ipc` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    /// IPC event name constant (see [`crate::common::event`]).
    pub event: String,
    /// Positional JSON arguments passed to the IPC handler.
    #[serde(default)]
    pub args: Vec<Value>,
}

/// JSON response returned by `/api/ipc`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    /// Whether the IPC call succeeded.
    pub ok: bool,
    /// Optional serialized return value when `ok == true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    /// Optional error message when `ok == false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// SSH host key the user must verify and approve before connecting again.
    #[serde(
        rename = "hostKeyChallenge",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub host_key_challenge: Option<Box<HostKeyChallenge>>,
}

impl IpcResponse {
    /// Constructs a successful [`IpcResponse`] serializing `val` into `data`.
    pub fn success<T: Serialize>(val: T) -> Self {
        Self {
            ok: true,
            data: serde_json::to_value(val).ok(),
            error: None,
            host_key_challenge: None,
        }
    }

    /// Constructs a failed [`IpcResponse`] with error message `msg`.
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(msg.into()),
            host_key_challenge: None,
        }
    }
}

pub(crate) fn str_arg(args: &[Value], idx: usize) -> Option<String> {
    args.get(idx)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Dispatches an [`IpcRequest`] across config, server, metadata, and query IPC handlers.
pub async fn dispatch_ipc_event(state: &WebIpcState, req: IpcRequest) -> IpcResponse {
    let event = req.event.as_str();
    let args = &req.args;

    if let Some(resp) = ipc_config_servers::handle_config_servers_ipc(event, args).await {
        return resp;
    }
    if let Some(resp) = ipc_db_metadata::handle_db_metadata_ipc(state, event, args).await {
        return resp;
    }
    if let Some(resp) = ipc_db_queries::handle_db_queries_ipc(state, event, args).await {
        return resp;
    }
    if let Some(resp) = ipc_ssh_host_keys::handle_ssh_host_keys_ipc(state, event, args).await {
        return resp;
    }

    match event {
        BROWSER_CLIPBOARD_WRITE => {
            write_clipboard_text(&str_arg(args, 0).unwrap_or_default());
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

/// Builds the [`axum::Router`] serving `/`, `/api/health`, and `/api/ipc`.
pub fn build_web_router(state: Arc<WebIpcState>) -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/api/health", get(handle_health))
        .route("/api/ipc", post(handle_ipc))
        .with_state(state)
}

/// Starts the embedded HTTP/IPC web server bound to `127.0.0.1:{port}`.
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
