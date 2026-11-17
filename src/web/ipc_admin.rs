//! Admin-only IPC events: accounts, connection grants, session revocation, and the audit log.
//!
//! Every event here requires the admin flag; a non-admin gets 403 and the attempt is audited.
//! Account and grant changes are audited with the target in `detail`.

use crate::common::event::*;
use crate::core::appdb::{AppDbError, AuditEvent, AuditKind};
use crate::core::servers;
use crate::web::{str_arg, IpcCtx, IpcResponse};
use serde_json::{json, Value};

const ADMIN_EVENTS: &[&str] = &[
    USERS_LIST,
    USERS_CREATE,
    USERS_DELETE,
    USERS_SET_ADMIN,
    USERS_SET_PASSWORD,
    USERS_REVOKE_SESSIONS,
    GRANTS_LIST,
    GRANTS_ADD,
    GRANTS_REMOVE,
    AUDIT_LIST,
];

fn int_arg(args: &[Value], idx: usize) -> Result<i64, IpcResponse> {
    args.get(idx)
        .and_then(Value::as_i64)
        .ok_or_else(|| IpcResponse::err(format!("Argument {idx} must be a user ID")))
}

fn db_err(e: AppDbError) -> IpcResponse {
    match e {
        AppDbError::NotFound(what) => IpcResponse::not_found(format!("{what} not found")),
        other => IpcResponse::err(other.to_string()),
    }
}

fn audit(ctx: &IpcCtx<'_>, kind: AuditKind, detail: String) {
    ctx.app
        .audit(AuditEvent::new(kind, &ctx.actor()).detail(detail));
}

async fn run(ctx: &IpcCtx<'_>, event: &str, args: &[Value]) -> Result<Value, IpcResponse> {
    let db = &ctx.app.db;
    match event {
        USERS_LIST => Ok(json!(db.list_users().map_err(db_err)?)),
        USERS_CREATE => {
            let username = str_arg(args, 0).unwrap_or_default();
            let password = str_arg(args, 1).unwrap_or_default();
            let is_admin = args.get(2).and_then(Value::as_bool).unwrap_or(false);
            let user = {
                let db = db.clone();
                tokio::task::spawn_blocking(move || db.create_user(&username, &password, is_admin))
                    .await
                    .map_err(|e| IpcResponse::err(e.to_string()))?
                    .map_err(db_err)?
            };
            audit(
                ctx,
                AuditKind::UserCreated,
                format!("user {} admin={is_admin}", user.id),
            );
            Ok(json!(user))
        }
        USERS_DELETE => {
            let id = int_arg(args, 0)?;
            db.delete_user(id).map_err(db_err)?;
            ctx.app.drop_user_session_states(id).await;
            audit(ctx, AuditKind::UserDeleted, format!("user {id}"));
            Ok(Value::Null)
        }
        USERS_SET_ADMIN => {
            let id = int_arg(args, 0)?;
            let is_admin = args.get(1).and_then(Value::as_bool).unwrap_or(false);
            db.set_admin(id, is_admin).map_err(db_err)?;
            audit(
                ctx,
                AuditKind::UserUpdated,
                format!("user {id} admin={is_admin}"),
            );
            Ok(Value::Null)
        }
        USERS_SET_PASSWORD => {
            let id = int_arg(args, 0)?;
            let password = str_arg(args, 1).unwrap_or_default();
            let db2 = db.clone();
            tokio::task::spawn_blocking(move || db2.set_password(id, &password))
                .await
                .map_err(|e| IpcResponse::err(e.to_string()))?
                .map_err(db_err)?;
            ctx.app.drop_user_session_states(id).await;
            audit(
                ctx,
                AuditKind::UserUpdated,
                format!("user {id} password changed"),
            );
            Ok(Value::Null)
        }
        USERS_REVOKE_SESSIONS => {
            let id = int_arg(args, 0)?;
            let n = db.revoke_user_sessions(id).map_err(db_err)?;
            ctx.app.drop_user_session_states(id).await;
            audit(
                ctx,
                AuditKind::SessionsRevoked,
                format!("user {id} sessions={n}"),
            );
            Ok(json!({ "revoked": n }))
        }
        GRANTS_LIST => {
            let user = args.first().and_then(Value::as_i64);
            Ok(json!(db.list_grants(user).map_err(db_err)?))
        }
        GRANTS_ADD | GRANTS_REMOVE => {
            let user_id = int_arg(args, 0)?;
            let conn_id = str_arg(args, 1).unwrap_or_default();
            let adding = event == GRANTS_ADD;
            if adding {
                let all = servers::get_all_sync().map_err(IpcResponse::err)?;
                if !all.iter().any(|s| s.id == conn_id) {
                    return Err(IpcResponse::not_found("Saved connection not found"));
                }
            }
            let changed = if adding {
                db.add_grant(user_id, &conn_id, Some(ctx.user.id))
            } else {
                db.remove_grant(user_id, &conn_id)
            }
            .map_err(db_err)?;
            if changed {
                let kind = if adding {
                    AuditKind::GrantAdded
                } else {
                    AuditKind::GrantRemoved
                };
                let ev = AuditEvent::new(kind, &ctx.actor())
                    .connection(conn_id)
                    .detail(format!("user {user_id}"));
                ctx.app.audit(ev);
            }
            Ok(json!({ "changed": changed }))
        }
        AUDIT_LIST => {
            let limit = args
                .first()
                .and_then(Value::as_u64)
                .unwrap_or(200)
                .min(1000);
            Ok(json!(db.list_audit(limit as u32).map_err(db_err)?))
        }
        _ => Err(IpcResponse::err(format!("Unknown admin event: {event}"))),
    }
}

/// Handles admin events, returning `None` for other events.
pub async fn handle_admin_ipc(
    ctx: &IpcCtx<'_>,
    event: &str,
    args: &[Value],
) -> Option<IpcResponse> {
    if !ADMIN_EVENTS.contains(&event) {
        return None;
    }
    if let Err(r) = ctx.require_admin(event) {
        return Some(r);
    }
    Some(match run(ctx, event, args).await {
        Ok(v) => IpcResponse::success(v),
        Err(r) => r,
    })
}
