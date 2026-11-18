//! Web IPC handlers for configuration (`CONFIG_*`) and saved connections (`SERVERS_*`).
//!
//! - No event accepts an encryption secret; the server uses its own key provider.
//! - Responses contain [`PublicServer`] values only: secrets become `has*` flags.
//! - Users see only the connections they hold a grant for (admins see all).
//! - Creating, changing, and deleting connections and settings requires an administrator.
//!   Clients send plaintext credentials only when creating or updating a connection; an
//!   omitted password keeps the saved one, and an empty string clears it.

use crate::common::event::*;
use crate::common::types::{BaseConfig, ConfigFile, Password, Server, ServerResult, SslConfig};
use crate::core::appdb::{AuditEvent, AuditKind};
use crate::core::{config, servers};
use crate::web::public_server::PublicServer;
use crate::web::{str_arg, IpcCtx, IpcResponse};
use serde_json::{json, Value};

fn reject_extra_args(event: &str, args: &[Value], expected: usize) -> Result<(), IpcResponse> {
    if args.len() > expected {
        return Err(IpcResponse::err(format!(
            "{event} takes {expected} argument(s); encryption secrets are never accepted from clients"
        )));
    }
    Ok(())
}

fn visible_servers(ctx: &IpcCtx<'_>, all: &[Server]) -> Result<Vec<PublicServer>, IpcResponse> {
    let mut out = Vec::new();
    for s in all {
        if ctx.may_use_connection(&s.id)? {
            out.push(PublicServer::from(s));
        }
    }
    Ok(out)
}

fn with_masked_servers(ctx: &IpcCtx<'_>, mut val: Value, all: &[Server]) -> IpcResponse {
    let visible = match visible_servers(ctx, all) {
        Ok(v) => v,
        Err(r) => return r,
    };
    val["servers"] = json!(visible);
    if !ctx.user.is_admin {
        if let Some(obj) = val.as_object_mut() {
            obj.remove("web");
        }
    }
    IpcResponse::success(val)
}

fn reject_ciphertext(pw: &Option<Password>) -> Result<(), IpcResponse> {
    if matches!(pw, Some(Password::Encrypted(_))) {
        return Err(IpcResponse::err(
            "Send passwords as plain text over TLS; encrypted values are not accepted",
        ));
    }
    Ok(())
}

/// Validates a client-sent profile and fills omitted secrets from the saved record.
fn prepare_incoming(mut srv: Server, existing: Option<&Server>) -> Result<Server, IpcResponse> {
    reject_ciphertext(&srv.password)?;
    if let Some(ssh) = srv.ssh.as_ref() {
        reject_ciphertext(&ssh.password)?;
        reject_ciphertext(&ssh.passphrase)?;
    }
    srv.local_host = None;
    srv.local_port = None;
    let Some(old) = existing else {
        return Ok(srv);
    };
    if srv.password.is_none() {
        srv.password = old.password.clone();
    }
    if let (Some(ssh), Some(old_ssh)) = (srv.ssh.as_mut(), old.ssh.as_ref()) {
        ssh.password = ssh.password.take().or_else(|| old_ssh.password.clone());
        ssh.passphrase = ssh.passphrase.take().or_else(|| old_ssh.passphrase.clone());
        ssh.private_key = ssh
            .private_key
            .take()
            .or_else(|| old_ssh.private_key.clone());
    }
    if let (Some(SslConfig::Details(new)), Some(SslConfig::Details(old_ssl))) =
        (srv.ssl.as_mut(), old.ssl.as_ref())
    {
        new.key = new.key.take().or_else(|| old_ssl.key.clone());
    }
    Ok(srv)
}

fn masked_result(res: ServerResult) -> Value {
    json!({
        "data": res.data.as_ref().map(PublicServer::from),
        "validationErrors": res.validation_errors,
    })
}

async fn save_server(ctx: &IpcCtx<'_>, event: &str, args: &[Value]) -> IpcResponse {
    if let Err(r) = ctx
        .require_admin(event)
        .and(reject_extra_args(event, args, 1))
    {
        return r;
    }
    let incoming: Server = match args.first().cloned().map(serde_json::from_value) {
        Some(Ok(s)) => s,
        Some(Err(e)) => return IpcResponse::err(e.to_string()),
        None => return IpcResponse::err("Missing server"),
    };
    let all = match servers::get_all_sync() {
        Ok(a) => a,
        Err(e) => return IpcResponse::err(e),
    };
    let has_id = !incoming.id.trim().is_empty();
    let existing = all
        .iter()
        .find(|s| event != SERVERS_ADD && has_id && s.id == incoming.id);
    if event == SERVERS_UPDATE && existing.is_none() {
        return IpcResponse::not_found("Saved connection not found");
    }
    let creating = existing.is_none();
    let srv = match prepare_incoming(incoming, existing) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let keys = &*ctx.app.keys;
    let result = if creating {
        servers::add_sync(
            &Server {
                id: String::new(),
                ..srv
            },
            keys,
        )
    } else {
        servers::update_sync(&srv, keys)
    };
    match result {
        Ok(res) => {
            if let Some(saved) = &res.data {
                let kind = if creating {
                    AuditKind::ConnectionCreated
                } else {
                    AuditKind::ConnectionUpdated
                };
                ctx.app
                    .audit(AuditEvent::new(kind, &ctx.actor()).connection(saved.id.clone()));
            }
            IpcResponse::success(masked_result(res))
        }
        Err(e) => IpcResponse::err(e),
    }
}

async fn remove_server(ctx: &IpcCtx<'_>, args: &[Value]) -> IpcResponse {
    if let Err(r) = ctx.require_admin(SERVERS_REMOVE_BY_ID) {
        return r;
    }
    let id = str_arg(args, 0).unwrap_or_default();
    if let Err(e) = servers::remove_by_id_sync(&id) {
        return IpcResponse::err(e);
    }
    if let Err(e) = ctx.app.db.remove_grants_for_connection(&id) {
        return IpcResponse::err(e.to_string());
    }
    ctx.app
        .audit(AuditEvent::new(AuditKind::ConnectionDeleted, &ctx.actor()).connection(id));
    IpcResponse::success(Value::Null)
}

fn save_settings(ctx: &IpcCtx<'_>, event: &str, args: &[Value]) -> IpcResponse {
    if let Err(r) = ctx.require_admin(event) {
        return r;
    }
    let Some(val) = args.first() else {
        return IpcResponse::err("Missing settings data");
    };
    let result = if event == CONFIG_SAVE {
        match serde_json::from_value::<ConfigFile>(val.clone()) {
            Ok(parsed) if !parsed.servers.is_empty() || parsed.ssh_host_keys.is_some() => Err(
                "CONFIG_SAVE saves settings only; use SERVERS_* and SSH_HOST_KEY_* events"
                    .to_string(),
            ),
            Ok(parsed) => config::get_sync().and_then(|mut current| {
                current.base = parsed.base;
                config::save_sync(&current)
            }),
            Err(e) => Err(e.to_string()),
        }
    } else {
        serde_json::from_value::<BaseConfig>(val.clone())
            .map_err(|e| e.to_string())
            .and_then(|parsed| config::save_settings_sync(&parsed))
    };
    match result {
        Ok(()) => IpcResponse::success(Value::Null),
        Err(e) => IpcResponse::err(e),
    }
}

/// Handles `CONFIG_*` and `SERVERS_*` events, returning `None` for other events.
pub async fn handle_config_servers_ipc(
    ctx: &IpcCtx<'_>,
    event: &str,
    args: &[Value],
) -> Option<IpcResponse> {
    let resp = match event {
        CONFIG_PREPARE => {
            if let Err(r) = ctx
                .require_admin(event)
                .and(reject_extra_args(event, args, 0))
            {
                return Some(r);
            }
            match config::prepare_sync(&*ctx.app.keys) {
                Ok(()) => IpcResponse::success(Value::Null),
                Err(e) => IpcResponse::err(e),
            }
        }
        CONFIG_PATH => IpcResponse::success(config::path()),
        CONFIG_GET => match config::get_sync() {
            Ok(cfg) => {
                let val = serde_json::to_value(&cfg).unwrap_or(Value::Null);
                with_masked_servers(ctx, val, &cfg.servers)
            }
            Err(e) => IpcResponse::err(e),
        },
        CONFIG_GET_FULL | CONFIG_GET_FULL_SYNC => {
            let clean = args.first().and_then(|v| v.as_bool()).unwrap_or(false);
            match config::get_full_sync(clean) {
                Ok(cfg) => {
                    let val = serde_json::to_value(&cfg).unwrap_or(Value::Null);
                    with_masked_servers(ctx, val, &cfg.servers)
                }
                Err(e) => IpcResponse::err(e),
            }
        }
        CONFIG_SAVE | CONFIG_SAVE_SETTINS => save_settings(ctx, event, args),
        SERVERS_GET_ALL => match servers::get_all_sync() {
            Ok(list) => match visible_servers(ctx, &list) {
                Ok(v) => IpcResponse::success(v),
                Err(r) => r,
            },
            Err(e) => IpcResponse::err(e),
        },
        SERVERS_ADD | SERVERS_UPDATE | SERVERS_ADD_OR_UPDATE => save_server(ctx, event, args).await,
        SERVERS_REMOVE_BY_ID => remove_server(ctx, args).await,
        _ => return None,
    };
    Some(resp)
}
