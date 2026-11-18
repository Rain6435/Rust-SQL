//! Web import: `DB_IMPORT_PREVIEW` and `DB_IMPORT_RUN`.
//!
//! The browser reads the chosen file and sends its text; nothing is read from or written to
//! the server's disk. Both events need the caller's grant for the open connection (re-checked
//! on every call), and the CSRF token like every IPC call. Rows are inserted by the shared
//! [`crate::core::import`] code: prepared statements, all rows in one transaction, and imports
//! of [`LARGE_IMPORT_ROWS`] rows or more need `confirmed`. A Read-Only session refuses imports.
//! Each run is audited (row count and target table, never values).
//!
//! Arguments (both events): `database`, `table`, `schema` (or null), `fileName` (its extension
//! picks CSV, TSV, or JSON), `text`. `DB_IMPORT_RUN` adds `mapping` (one target column name or
//! null per source column) and `confirmed`. Files larger than the 1 MiB `/api/ipc` limit go
//! to `/api/ipc/import`, which accepts these two events only, up to
//! [`crate::web::settings::IMPORT_BODY_LIMIT_BYTES`].

use crate::common::event::{DB_IMPORT_PREVIEW, DB_IMPORT_RUN};
use crate::core::appdb::{AuditEvent, AuditKind};
use crate::core::import::{
    mapping_slots, mappings_from_slots, parse_named, ImportData, ImportRequest, LARGE_IMPORT_ROWS,
    PREVIEW_ROWS,
};
use crate::web::{str_arg, IpcCtx, IpcResponse};
use serde_json::{json, Value};

/// Events handled here.
pub const IMPORT_EVENTS: &[&str] = &[DB_IMPORT_PREVIEW, DB_IMPORT_RUN];

fn csv_delimiter() -> u8 {
    crate::core::config::get_sync()
        .ok()
        .and_then(|c| c.base.csv_delimiter)
        .and_then(|d| d.bytes().next())
        .unwrap_or(b',')
}

fn parse_args(args: &[Value]) -> Result<ImportData, IpcResponse> {
    let name = str_arg(args, 3).unwrap_or_default();
    let text = args.get(4).and_then(Value::as_str).unwrap_or_default();
    parse_named(&name, text, csv_delimiter()).map_err(IpcResponse::err)
}

fn mapping_arg(v: Option<&Value>) -> Result<Vec<Option<String>>, IpcResponse> {
    let Some(items) = v.and_then(Value::as_array) else {
        return Err(IpcResponse::err("mapping must be an array"));
    };
    items
        .iter()
        .map(|m| match m {
            Value::Null => Ok(None),
            Value::String(s) => Ok(Some(s.clone())),
            _ => Err(IpcResponse::err(
                "mapping entries must be column names or null",
            )),
        })
        .collect()
}

/// Handles the import events, returning `None` for other events.
pub async fn handle_import_ipc(
    ctx: &IpcCtx<'_>,
    event: &str,
    args: &[Value],
) -> Option<IpcResponse> {
    if !IMPORT_EVENTS.contains(&event) {
        return None;
    }
    let conn_id = match ctx.require_connected() {
        Ok(id) => id,
        Err(r) => return Some(r),
    };
    let database = str_arg(args, 0).unwrap_or_default();
    let table = str_arg(args, 1).unwrap_or_default();
    let schema = str_arg(args, 2);
    let data = match parse_args(args) {
        Ok(d) => d,
        Err(r) => return Some(r),
    };
    let db = ctx.session.db.lock().await;
    if event == DB_IMPORT_PREVIEW {
        let targets: Vec<String> = match db
            .list_table_columns(&database, &table, schema.as_deref())
            .await
        {
            Ok(c) => c.into_iter().map(|c| c.column_name).collect(),
            Err(e) => return Some(IpcResponse::err(e)),
        };
        let preview: Vec<Vec<Value>> = data
            .preview(PREVIEW_ROWS)
            .iter()
            .map(|r| r.iter().map(|v| v.to_json()).collect())
            .collect();
        return Some(IpcResponse::success(json!({
            "columns": data.columns,
            "rowCount": data.rows.len(),
            "preview": preview,
            "targetColumns": targets,
            "mapping": mapping_slots(&data.columns, &targets),
            "requiresConfirmation": data.requires_confirmation(),
            "largeImportRows": LARGE_IMPORT_ROWS,
        })));
    }
    if db.read_only {
        return Some(IpcResponse::err("Import is disabled in Read-Only mode"));
    }
    let slots = match mapping_arg(args.get(5)) {
        Ok(s) => s,
        Err(r) => return Some(r),
    };
    let mappings = mappings_from_slots(&slots);
    let req = ImportRequest {
        table: &table,
        schema: schema.as_deref(),
        mappings: &mappings,
        data: &data,
        confirmed: args.get(6).and_then(Value::as_bool).unwrap_or(false),
    };
    let res = db.import_rows(&database, req).await;
    let rows = res.as_ref().map_or(0, |r| r.rows_inserted);
    let ev = AuditEvent::new(AuditKind::QueryExecuted, &ctx.actor())
        .outcome(if res.is_ok() { "success" } else { "failure" })
        .connection(&conn_id)
        .detail(format!("import into {table}"))
        .statement(Some("IMPORT".into()), i64::try_from(rows).ok());
    ctx.app.audit(ev);
    Some(match res {
        Ok(r) => IpcResponse::success(json!({ "rowsInserted": r.rows_inserted })),
        Err(e) => IpcResponse::err(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_accepts_names_and_null_only() {
        let ok = mapping_arg(Some(&json!(["id", null]))).unwrap();
        assert_eq!(ok, vec![Some("id".to_string()), None]);
        assert!(mapping_arg(Some(&json!([1]))).is_err());
        assert!(mapping_arg(None).is_err());
    }
}
