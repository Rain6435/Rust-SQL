//! Paged query execution on [`DatabaseFacade`] (see [`crate::core::paging`]).

use crate::adapters::facade::DatabaseFacade;
use crate::common::types::QueryRowResult;
use crate::core::paging::{finish_page, page_sql, pageable_select, PageCursor};
use crate::core::params::QueryParams;
use serde::{Deserialize, Serialize};

/// Results of a paged execution: one page for a single `SELECT`, every result otherwise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedExecution {
    /// Statement results (a single page when [`page`](Self::page) is set).
    pub results: Vec<QueryRowResult>,
    /// The page that was fetched, or `None` when the script is not pageable.
    pub page: Option<PageInfo>,
}

/// Where a fetched page sits in the full result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    /// The fetched page.
    pub cursor: PageCursor,
    /// The next page, or `None` when no more rows exist.
    pub next: Option<PageCursor>,
}

impl DatabaseFacade {
    /// Runs `query_text`, returning only the page at `cursor` when it is a single `SELECT`.
    ///
    /// Scripts that cannot be paged run in full and report `page: None`.
    pub async fn execute_paged(
        &self,
        database: &str,
        query_text: &str,
        params: &QueryParams,
        cursor: PageCursor,
    ) -> Result<PagedExecution, String> {
        let key = self.adapter_key().unwrap_or_default().to_string();
        let Some(select) = pageable_select(query_text, &key) else {
            let results = self
                .execute_query_with_params(database, query_text, params)
                .await?;
            return Ok(PagedExecution {
                results,
                page: None,
            });
        };
        let mut results = self
            .execute_query_with_params(database, &page_sql(&select, cursor), params)
            .await?;
        let first = results.pop().ok_or("The paged query returned no result")?;
        let page = finish_page(first, cursor);
        Ok(PagedExecution {
            results: vec![page.result],
            page: Some(PageInfo {
                cursor: page.cursor,
                next: page.next,
            }),
        })
    }
}
