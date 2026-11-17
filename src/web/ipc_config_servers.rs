//! Web IPC handlers for configuration (`CONFIG_*`) and server management (`SERVERS_*`) events.

use crate::common::event::*;
use crate::common::types::{BaseConfig, ConfigFile, Server};
use crate::core::config::{self, DEFAULT_CRYPTO_SECRET};
use crate::core::servers;
use crate::web::{str_arg, IpcResponse};
use serde_json::Value;

fn parse_server(args: &[Value]) -> Result<Server, IpcResponse> {
    let Some(srv_val) = args.first() else {
        return Err(IpcResponse::err("Missing server"));
    };
    serde_json::from_value(srv_val.clone()).map_err(|e| IpcResponse::err(e.to_string()))
}

/// Handles `CONFIG_*` and `SERVERS_*` IPC events, returning `None` if `event` belongs to another domain.
pub async fn handle_config_servers_ipc(event: &str, args: &[Value]) -> Option<IpcResponse> {
    let resp = match event {
        CONFIG_PREPARE => {
            let secret = str_arg(args, 0).unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string());
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
                return Some(IpcResponse::err("Missing config data"));
            };
            let parsed: ConfigFile = match serde_json::from_value(val.clone()) {
                Ok(c) => c,
                Err(e) => return Some(IpcResponse::err(e.to_string())),
            };
            match config::save(&parsed).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        CONFIG_SAVE_SETTINS => {
            let Some(val) = args.first() else {
                return Some(IpcResponse::err("Missing settings data"));
            };
            let parsed: BaseConfig = match serde_json::from_value(val.clone()) {
                Ok(c) => c,
                Err(e) => return Some(IpcResponse::err(e.to_string())),
            };
            match config::save_settings(&parsed).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        SERVERS_GET_ALL => match servers::get_all().await {
            Ok(list) => IpcResponse::success(list),
            Err(e) => IpcResponse::err(e),
        },
        SERVERS_ADD => {
            let srv = match parse_server(args) {
                Ok(s) => s,
                Err(r) => return Some(r),
            };
            let secret = str_arg(args, 1).unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string());
            match servers::add(&srv, &secret).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        SERVERS_UPDATE => {
            let srv = match parse_server(args) {
                Ok(s) => s,
                Err(r) => return Some(r),
            };
            let secret = str_arg(args, 1).unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string());
            match servers::update(&srv, &secret).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        SERVERS_ADD_OR_UPDATE => {
            let srv = match parse_server(args) {
                Ok(s) => s,
                Err(r) => return Some(r),
            };
            let secret = str_arg(args, 1).unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string());
            match servers::add_or_update(&srv, &secret).await {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        SERVERS_REMOVE_BY_ID => {
            let id = str_arg(args, 0).unwrap_or_default();
            match servers::remove_by_id(&id).await {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        SERVERS_DECRYPT_SECRETS => {
            let srv = match parse_server(args) {
                Ok(s) => s,
                Err(r) => return Some(r),
            };
            let secret = str_arg(args, 1).unwrap_or_else(|| DEFAULT_CRYPTO_SECRET.to_string());
            match servers::decrypt_secrets(&srv, &secret) {
                Ok(res) => IpcResponse::success(res),
                Err(e) => IpcResponse::err(e),
            }
        }
        _ => return None,
    };
    Some(resp)
}
