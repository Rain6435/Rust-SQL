//! Result paging: fetch a query's rows one page at a time, on demand.
//!
//! A script that is exactly one read-only `SELECT` (as classified by the parser in
//! [`crate::core::safety`]) is pageable. Each page runs the query wrapped as
//! `SELECT * FROM (<query>) AS rsql_page LIMIT <size + 1> OFFSET <offset>`; the extra row only
//! tells whether another page exists and is dropped. The cursor handed to the front end is the
//! [`PageCursor`] for the next page, so fetching more is stateless on the server.
//!
//! Pages are computed with `LIMIT`/`OFFSET`, so a page reflects the data at the time it is
//! fetched: rows inserted or deleted between pages can shift later pages. Add an `ORDER BY`
//! to the query for a stable order. Anything else (several statements, writes, `EXPLAIN`,
//! Cassandra) runs unpaged, as before.

use crate::common::types::QueryRowResult;
use crate::core::safety::{analyze_script, SqlDialect, StatementAccess};
use crate::core::sql_identifier::StatementKind;
use serde::{Deserialize, Serialize};

/// Position of a page: rows `offset .. offset + page_size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageCursor {
    /// Rows to skip.
    pub offset: u64,
    /// Rows per page.
    pub page_size: u32,
}

impl PageCursor {
    /// The first page of `page_size` rows.
    pub fn first(page_size: u32) -> Self {
        Self {
            offset: 0,
            page_size: page_size.max(1),
        }
    }
}

/// One fetched page and the cursor of the next one, if more rows exist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultPage {
    /// The page's rows.
    pub result: QueryRowResult,
    /// Where this page starts and how big pages are.
    pub cursor: PageCursor,
    /// The next page, or `None` when this is the last one.
    pub next: Option<PageCursor>,
}

/// The query text to page, without a trailing `;`, when `query` is one read-only `SELECT`.
pub fn pageable_select(query: &str, adapter_key: &str) -> Option<String> {
    if adapter_key == "cassandra" {
        return None;
    }
    let analysis = analyze_script(query, SqlDialect::from_adapter_key(adapter_key), false);
    let [stmt] = analysis.statements.as_slice() else {
        return None;
    };
    let ok = stmt.parsed
        && stmt.kind == StatementKind::Select
        && stmt.access == StatementAccess::ReadOnly;
    let text = query.trim().trim_end_matches(';').trim_end();
    (ok && !text.is_empty() && !text.contains(';')).then(|| text.to_string())
}

/// The SQL that fetches the page at `cursor` (one extra row detects a following page).
pub fn page_sql(select: &str, cursor: PageCursor) -> String {
    format!(
        "SELECT * FROM (\n{select}\n) AS rsql_page LIMIT {} OFFSET {}",
        u64::from(cursor.page_size) + 1,
        cursor.offset
    )
}

/// Trims the extra look-ahead row from `result` and computes the next cursor.
pub fn finish_page(mut result: QueryRowResult, cursor: PageCursor) -> ResultPage {
    let size = cursor.page_size as usize;
    let has_more = result.rows.len() > size;
    result.rows.truncate(size);
    result.row_count = Some(result.rows.len());
    ResultPage {
        result,
        cursor,
        next: has_more.then(|| PageCursor {
            offset: cursor.offset + u64::from(cursor.page_size),
            page_size: cursor.page_size,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value};

    #[test]
    fn only_a_single_read_only_select_is_pageable() {
        assert_eq!(
            pageable_select(
                "SELECT * FROM t WHERE id > :id ORDER BY id;  ",
                "postgresql"
            ),
            Some("SELECT * FROM t WHERE id > :id ORDER BY id".to_string())
        );
        assert!(pageable_select("WITH x AS (SELECT 1 AS a) SELECT a FROM x", "sqlite").is_some());
        for q in [
            "SELECT 1; SELECT 2",
            "DELETE FROM t",
            "EXPLAIN SELECT 1",
            "SELECT * INTO new_t FROM t",
            "WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d",
            "SELEC broken",
        ] {
            assert_eq!(pageable_select(q, "postgresql"), None, "{q}");
        }
        assert_eq!(pageable_select("SELECT 1", "cassandra"), None);
    }

    #[test]
    fn page_sql_fetches_one_extra_row() {
        let sql = page_sql(
            "SELECT a FROM t",
            PageCursor {
                offset: 200,
                page_size: 100,
            },
        );
        assert!(
            sql.ends_with(") AS rsql_page LIMIT 101 OFFSET 200"),
            "{sql}"
        );
    }

    #[test]
    fn finish_page_trims_the_look_ahead_row() {
        let rows = |n: usize| QueryRowResult {
            command: "SELECT".into(),
            rows: vec![Map::from_iter([("a".to_string(), Value::from(1))]); n],
            fields: vec![],
            row_count: Some(n),
            affected_rows: None,
        };
        let c = PageCursor::first(2);
        let p = finish_page(rows(3), c);
        assert_eq!(p.result.rows.len(), 2);
        assert_eq!(
            p.next,
            Some(PageCursor {
                offset: 2,
                page_size: 2
            })
        );
        assert_eq!(finish_page(rows(2), c).next, None);
    }
}
