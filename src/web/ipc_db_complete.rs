//! `DB_COMPLETE`: SQL editor autocomplete for the web UI, from the shared completion engine
//! ([`crate::core::completion`]).
//!
//! Arguments: `database`, `text`, `cursor` (a character index, counting Unicode code points),
//! `explicit` (the user pressed Ctrl+Space). The response is `{ start, prefix, items: [{ label,
//! kind }] }`, with `start` in code points too. Needs the caller's grant for the open
//! connection. Table names and the columns of referenced tables are cached per session and
//! database; an explicit request reloads the table list. The config's `enabledAutoComplete`
//! and `enabledLiveAutoComplete` toggles are honored.

use crate::common::event::DB_COMPLETE;
use crate::core::completion::{
    complete, referenced_tables, CompletionKind, CompletionSchema, Completions,
};
use crate::web::state::CompletionCache;
use crate::web::{str_arg, IpcCtx, IpcResponse};
use serde_json::{json, Value};

/// Most tables whose columns one request may load.
const MAX_COLUMN_LOADS: usize = 8;

/// Largest editor text accepted, in bytes.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;

fn enabled(explicit: bool) -> bool {
    let cfg = crate::core::config::get_sync().ok();
    let on = |v: Option<bool>| v.unwrap_or(true);
    let auto = on(cfg.as_ref().and_then(|c| c.base.enabled_auto_complete));
    let live = on(cfg.as_ref().and_then(|c| c.base.enabled_live_auto_complete));
    auto && (explicit || live)
}

fn to_json(c: &Completions) -> Value {
    let items: Vec<Value> = c
        .items
        .iter()
        .map(|i| {
            let kind = match i.kind {
                CompletionKind::Column => "column",
                CompletionKind::Table => "table",
                CompletionKind::Keyword => "keyword",
            };
            json!({ "label": i.label, "kind": kind })
        })
        .collect();
    json!({ "start": c.start, "prefix": c.prefix, "items": items })
}

/// Fills `cache` with the table list (if empty or `reload`) and the columns of tables that
/// `text` references.
async fn load(
    ctx: &IpcCtx<'_>,
    database: &str,
    text: &str,
    cache: &mut CompletionCache,
    reload: bool,
) -> Result<(), String> {
    let db = ctx.session.db.lock().await;
    if reload || cache.tables.is_empty() {
        cache.tables = db.list_tables(database, None).await?;
    }
    let mut loads = 0;
    for (name, _) in referenced_tables(text) {
        let Some(t) = cache
            .tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(&name))
        else {
            continue;
        };
        if cache.columns.contains_key(&t.name) || loads >= MAX_COLUMN_LOADS {
            continue;
        }
        loads += 1;
        if let Ok(cols) = db
            .list_table_columns(database, &t.name, t.schema.as_deref())
            .await
        {
            let names = cols.into_iter().map(|c| c.column_name).collect();
            cache.columns.insert(t.name.clone(), names);
        }
    }
    Ok(())
}

/// Handles `DB_COMPLETE`, returning `None` for other events.
pub async fn handle_complete_ipc(
    ctx: &IpcCtx<'_>,
    event: &str,
    args: &[Value],
) -> Option<IpcResponse> {
    if event != DB_COMPLETE {
        return None;
    }
    let conn_id = match ctx.require_connected() {
        Ok(id) => id,
        Err(r) => return Some(r),
    };
    let database = str_arg(args, 0).unwrap_or_default();
    let text = str_arg(args, 1).unwrap_or_default();
    if text.len() > MAX_TEXT_BYTES {
        return Some(IpcResponse::err("Query text is too long for autocomplete"));
    }
    let cursor = args.get(2).and_then(Value::as_u64).unwrap_or(0) as usize;
    let explicit = args.get(3).and_then(Value::as_bool).unwrap_or(false);
    if !enabled(explicit) {
        return Some(IpcResponse::success(to_json(&Completions::default())));
    }
    let lock = |s: &crate::web::WebIpcState| {
        s.completion
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&database)
            .cloned()
            .unwrap_or_default()
    };
    let mut cache = lock(ctx.session);
    if let Err(e) = load(ctx, &database, &text, &mut cache, explicit).await {
        return Some(IpcResponse::err(e));
    }
    let schema = CompletionSchema {
        tables: cache.tables.iter().map(|t| t.name.clone()).collect(),
        columns: cache.columns.clone(),
    };
    // Keep the cache only if the session is still on the same connection.
    if ctx.session.connected_server().as_deref() == Some(conn_id.as_str()) {
        ctx.session
            .completion
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(database, cache);
    }
    Some(IpcResponse::success(to_json(&complete(
        &text, cursor, &schema, explicit,
    ))))
}
