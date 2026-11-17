//! Multi-user web server (`--web`) powered by `axum`.
//!
//! Accounts, sessions, connection grants, and the audit log live in the application database
//! ([`crate::core::appdb`]). See [`server`] for the HTTP guards and TLS, [`auth`] for login and
//! CSRF, and [`state`] for per-session state and authorization helpers. Encryption keys never
//! travel to or from clients, and no response contains a saved password.

pub mod assets;
pub mod auth;
pub mod ipc_admin;
pub mod ipc_config_servers;
pub mod ipc_db_metadata;
pub mod ipc_db_queries;
pub mod ipc_policy;
pub mod ipc_ssh_host_keys;
pub mod public_server;
pub mod query_audit;
pub mod rate_limit;
pub mod server;
pub mod settings;
pub mod state;

pub use assets::RUST_SQL_WEB_HTML;
pub use server::{build_web_router, run_web_server, serve_on};
pub use settings::{WebLaunchOptions, WebSettings};
pub use state::{IpcCtx, WebApp, WebIpcState};

use crate::common::event::{BROWSER_CLIPBOARD_WRITE, LOGGER_ERROR};
use crate::common::types::HostKeyChallenge;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// HTTP status override (for example 403 for a missing grant); not serialized.
    #[serde(skip)]
    pub status: Option<u16>,
}

impl IpcResponse {
    /// Constructs a successful [`IpcResponse`] serializing `val` into `data`.
    pub fn success<T: Serialize>(val: T) -> Self {
        Self {
            ok: true,
            data: serde_json::to_value(val).ok(),
            error: None,
            host_key_challenge: None,
            status: None,
        }
    }

    /// Constructs a failed [`IpcResponse`] with error message `msg`.
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(msg.into()),
            host_key_challenge: None,
            status: None,
        }
    }

    /// Constructs a 403 response.
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self {
            status: Some(403),
            ..Self::err(msg)
        }
    }

    /// Constructs a 404 response.
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            status: Some(404),
            ..Self::err(msg)
        }
    }
}

/// Reads the optional typed parameter object at `idx` (`{"id": 5, "name": "x"}`).
///
/// JSON types are kept: numbers stay numbers and strings stay text. The values are bound
/// through prepared statements, never substituted into the SQL.
pub(crate) fn params_arg(
    args: &[Value],
    idx: usize,
) -> Result<crate::core::params::QueryParams, IpcResponse> {
    crate::core::params::ParamValue::map_from_json(args.get(idx).unwrap_or(&Value::Null))
        .map_err(IpcResponse::err)
}

pub(crate) fn str_arg(args: &[Value], idx: usize) -> Option<String> {
    args.get(idx)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Dispatches an authenticated [`IpcRequest`] to the admin, config, server, metadata, query,
/// and host-key handlers.
pub async fn dispatch_ipc_event(ctx: &IpcCtx<'_>, req: IpcRequest) -> IpcResponse {
    let event = req.event.as_str();
    let args = &req.args;

    if let Some(resp) = ipc_admin::handle_admin_ipc(ctx, event, args).await {
        return resp;
    }
    if let Some(resp) = ipc_config_servers::handle_config_servers_ipc(ctx, event, args).await {
        return resp;
    }
    if let Some(resp) = ipc_db_metadata::handle_db_metadata_ipc(ctx, event, args).await {
        return resp;
    }
    if let Some(resp) = ipc_db_queries::handle_db_queries_ipc(ctx, event, args).await {
        return resp;
    }
    if let Some(resp) = ipc_ssh_host_keys::handle_ssh_host_keys_ipc(ctx, event, args).await {
        return resp;
    }

    match event {
        // The server's clipboard is not the user's; the browser copies locally instead.
        BROWSER_CLIPBOARD_WRITE => {
            IpcResponse::err("Clipboard writes happen in the browser, not on the server")
        }
        LOGGER_ERROR => IpcResponse::success(Value::Null),
        other => IpcResponse::err(format!("Unknown IPC event: {other}")),
    }
}
