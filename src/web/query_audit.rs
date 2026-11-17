//! Audit rows for statement execution: connection, user, time, statement type, and row count.
//! The query text is never recorded.

use crate::common::types::QueryRowResult;
use crate::core::appdb::{AuditEvent, AuditKind};
use crate::web::IpcCtx;

/// Records one `query_executed` row per statement result, or one failure row.
pub fn audit_query_results(
    ctx: &IpcCtx<'_>,
    connection_id: &str,
    result: &Result<Vec<QueryRowResult>, String>,
) {
    let actor = ctx.actor();
    match result {
        Ok(results) => {
            for r in results {
                let rows = r.row_count.or(r.affected_rows).unwrap_or(r.rows.len());
                let ev = AuditEvent::new(AuditKind::QueryExecuted, &actor)
                    .connection(connection_id)
                    .statement(Some(r.command.clone()), i64::try_from(rows).ok());
                ctx.app.audit(ev);
            }
        }
        Err(_) => {
            let ev = AuditEvent::new(AuditKind::QueryExecuted, &actor)
                .outcome("failure")
                .connection(connection_id);
            ctx.app.audit(ev);
        }
    }
}

/// Records a statement that the server generated itself (for example `TRUNCATE` of all tables).
pub fn audit_statement(ctx: &IpcCtx<'_>, connection_id: &str, kind: &str, ok: bool) {
    let ev = AuditEvent::new(AuditKind::QueryExecuted, &ctx.actor())
        .outcome(if ok { "success" } else { "failure" })
        .connection(connection_id)
        .statement(Some(kind.to_string()), None);
    ctx.app.audit(ev);
}
