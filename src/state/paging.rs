//! Paged execution for query tabs: the first page runs with the query, "load more" fetches
//! the next one (see [`crate::core::paging`]).

use crate::common::types::QueryRowResult;
use crate::core::paging::{finish_page, page_sql, pageable_select, PageCursor};
use crate::core::params::QueryParams;
use crate::state::{AppStore, TabPage};

impl AppStore {
    /// Rows per result page, from the configuration.
    pub fn result_page_size(&self) -> u32 {
        self.config
            .data
            .as_ref()
            .map(|c| c.result_page_size)
            .unwrap_or_else(crate::common::types::default_result_page_size)
            .max(1)
    }

    fn adapter_key(&self) -> String {
        self.db.adapter_key().unwrap_or_default().to_string()
    }

    /// Runs `query_text` for tab `query_id` (cancellable when the dialect allows it). A single
    /// `SELECT` fetches only its first page and returns the tab's paging state.
    pub(crate) async fn run_tab_query(
        &mut self,
        db_name: &str,
        query_id: u64,
        query_text: &str,
        params: &QueryParams,
    ) -> (Result<Vec<QueryRowResult>, String>, Option<TabPage>) {
        let cursor = PageCursor::first(self.result_page_size());
        let select = pageable_select(query_text, &self.adapter_key());
        let sql = select
            .as_deref()
            .map(|s| page_sql(s, cursor))
            .unwrap_or_else(|| query_text.to_string());
        let can_cancel = !self
            .connections
            .disabled_features
            .iter()
            .any(|f| f == "cancelQuery");
        let res = if can_cancel {
            match self
                .db
                .create_cancellable_query_with_params(db_name, query_id, &sql, params)
            {
                Ok(()) => self.db.execute_cancellable_query(query_id).await,
                Err(e) => Err(e),
            }
        } else {
            self.db
                .execute_query_with_params(db_name, &sql, params)
                .await
        };
        let mut results = match (&select, res) {
            (Some(_), Ok(results)) => results,
            (_, other) => return (other, None),
        };
        let Some(first) = results.pop() else {
            return (Ok(results), None);
        };
        let page = finish_page(first, cursor);
        let state = TabPage {
            query: query_text.to_string(),
            params: params.clone(),
            page_size: cursor.page_size,
            next: page.next,
        };
        (Ok(vec![page.result]), Some(state))
    }

    /// Fetches the next page of the active tab's result and appends its rows.
    ///
    /// Returns the number of rows added (0 when everything is already loaded).
    pub async fn fetch_more_rows(&mut self) -> Result<usize, String> {
        let Some(tab) = self.current_query() else {
            return Err("No active query tab".to_string());
        };
        let (db_name, query_id) = (tab.database.clone(), tab.id);
        let Some(page) = tab.page.clone() else {
            return Ok(0);
        };
        let Some(cursor) = page.next else {
            return Ok(0);
        };
        let select = pageable_select(&page.query, &self.adapter_key())
            .ok_or("This result cannot be paged")?;
        let mut results = self
            .db
            .execute_query_with_params(&db_name, &page_sql(&select, cursor), &page.params)
            .await?;
        let fetched = finish_page(results.pop().ok_or("No rows returned")?, cursor);
        let added = fetched.result.rows.len();
        if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
            if let Some(first) = q
                .results
                .as_mut()
                .and_then(|r| std::sync::Arc::make_mut(r).first_mut())
            {
                first.rows.extend(fetched.result.rows);
                first.row_count = Some(first.rows.len());
            }
            if let Some(p) = q.page.as_mut() {
                p.next = fetched.next;
            }
        }
        Ok(added)
    }
}
