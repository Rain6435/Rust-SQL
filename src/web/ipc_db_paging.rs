//! `DB_EXECUTE_PAGED`: runs a query and returns one page of rows when it is a single `SELECT`.
//!
//! Arguments: `database`, `sql`, optional parameters object, optional `{ offset, pageSize }`
//! cursor (from the previous response's `page.next`). The response is
//! `{ results, page: { cursor, next } | null }`; `page` is `null` for scripts that cannot be
//! paged, which run in full. Each call is audited like `DB_EXECUTE_QUERY`, without the text.

use crate::adapters::DEFAULT_LIMIT;
use crate::core::paging::PageCursor;
use crate::web::query_audit::audit_query_results;
use crate::web::{params_arg, str_arg, IpcCtx, IpcResponse};
use serde_json::Value;

/// Largest page a client may ask for.
pub const MAX_PAGE_SIZE: u32 = 10_000;

fn cursor_arg(v: Option<&Value>) -> PageCursor {
    let offset = v.and_then(|c| c["offset"].as_u64()).unwrap_or(0);
    let size = v
        .and_then(|c| c["pageSize"].as_u64())
        .map(|n| n.clamp(1, u64::from(MAX_PAGE_SIZE)) as u32)
        .unwrap_or(DEFAULT_LIMIT);
    PageCursor {
        offset,
        page_size: size,
    }
}

/// Handles `DB_EXECUTE_PAGED` (the grant was checked by the query handler's policy list).
pub async fn execute_paged(ctx: &IpcCtx<'_>, args: &[Value]) -> IpcResponse {
    let conn_id = match ctx.require_connected() {
        Ok(id) => id,
        Err(r) => return r,
    };
    let database = str_arg(args, 0).unwrap_or_default();
    let query_text = str_arg(args, 1).unwrap_or_default();
    let params = match params_arg(args, 2) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let cursor = cursor_arg(args.get(3));
    let db = ctx.session.db.lock().await;
    let res = db
        .execute_paged(&database, &query_text, &params, cursor)
        .await;
    audit_query_results(
        ctx,
        &conn_id,
        &res.as_ref()
            .map(|r| r.results.clone())
            .map_err(Clone::clone),
    );
    match res {
        Ok(page) => IpcResponse::success(page),
        Err(e) => IpcResponse::err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cursor_defaults_and_caps_the_page_size() {
        assert_eq!(
            cursor_arg(None),
            PageCursor {
                offset: 0,
                page_size: DEFAULT_LIMIT
            }
        );
        let c = cursor_arg(Some(&json!({"offset": 50, "pageSize": 999_999})));
        assert_eq!(
            c,
            PageCursor {
                offset: 50,
                page_size: MAX_PAGE_SIZE
            }
        );
    }
}
