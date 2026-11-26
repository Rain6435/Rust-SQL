//! SQL query tab management, parameter binding, and query execution actions on [`AppStore`].

use crate::common::types::QueryRowResult;
use crate::core::params::{bind_for_display, extract_query_parameters, QueryParams};
use crate::state::{AppStore, QueryTab};
use std::time::Instant;

impl AppStore {
    /// Creates and activates a new query tab for `database` (optionally named after `table`).
    pub fn add_new_query(&mut self, database: &str, table: Option<&str>) -> u64 {
        let new_id = self.queries.last_created_id + 1;
        let name = match table.filter(|t| !t.is_empty()) {
            Some(t) => format!("{database} / {t} #{new_id}"),
            None => format!("{database} #{new_id}"),
        };
        let tab = QueryTab {
            id: new_id,
            database: database.to_string(),
            name,
            filename: None,
            is_executing: false,
            is_default_select: false,
            did_invalidate: true,
            query: String::new(),
            selected_query: String::new(),
            query_history: Vec::new(),
            results: None,
            error: None,
            copied: None,
            saved: None,
            is_canceling: false,
            result_items_per_page: self.queries.result_items_per_page,
            execution_time: None,
            page: None,
        };
        self.queries.last_created_id = new_id;
        self.queries.current_query_id = Some(new_id);
        self.queries.query_ids.push(new_id);
        self.queries.queries_by_id.insert(new_id, tab);
        new_id
    }

    /// Creates a new untitled query tab for `database`.
    pub fn new_query(&mut self, database: &str) -> u64 {
        self.add_new_query(database, None)
    }

    /// Switches the active query tab to `id`.
    pub fn select_query(&mut self, id: u64) {
        if self.queries.queries_by_id.contains_key(&id) {
            self.queries.current_query_id = Some(id);
        }
    }

    /// Renames the currently active query tab to `name`.
    pub fn rename_query(&mut self, name: &str) {
        if !name.is_empty() {
            if let Some(q) = self.current_query_mut() {
                q.name = name.to_string();
            }
        }
    }

    /// Closes `query_id`, automatically creating a fresh tab if it was the last open tab.
    pub fn remove_query(&mut self, query_id: u64) {
        let Some(current_db) = self
            .queries
            .queries_by_id
            .get(&query_id)
            .map(|q| q.database.clone())
        else {
            return;
        };
        let index = self
            .queries
            .query_ids
            .iter()
            .position(|&id| id == query_id)
            .unwrap_or(0);
        self.queries.queries_by_id.remove(&query_id);
        self.queries.query_ids.retain(|&id| id != query_id);
        if self.queries.query_ids.is_empty() {
            self.add_new_query(&current_db, None);
        } else {
            let next_idx = index.saturating_sub(1);
            self.queries.current_query_id = self.queries.query_ids.get(next_idx).copied();
        }
    }

    /// Updates the SQL buffer and optional selection text of the active query tab.
    pub fn update_query_if_needed(
        &mut self,
        query: &str,
        selected_query: Option<&str>,
        table: Option<&str>,
    ) {
        let Some(current) = self.current_query_mut() else {
            return;
        };
        if current.is_executing {
            return;
        }
        current.query = query.to_string();
        current.selected_query = selected_query.unwrap_or("").to_string();
        current.copied = Some(false);
        if let Some(t) = table.filter(|t| !t.is_empty()) {
            current.name = format!("{} / {} #{}", current.database, t, current.id);
        }
    }

    /// Appends `sql` on a new line in the active query tab.
    pub fn append_query(&mut self, sql: &str) {
        let Some(current) = self.current_query_mut() else {
            return;
        };
        if current.is_executing {
            return;
        }
        current.query = if current.query.is_empty() {
            sql.to_string()
        } else {
            format!("{}\n{}", current.query, sql)
        };
        current.selected_query.clear();
        current.copied = Some(false);
    }

    /// Typed values for the placeholders in `query_text`, taken from [`AppStore::query_params`].
    ///
    /// Inputs for parameters the query does not use are ignored. A placeholder with no input
    /// is left for the driver layer, which reports it by name.
    pub fn params_for_query(&self, query_text: &str) -> Result<QueryParams, String> {
        extract_query_parameters(query_text)
            .into_iter()
            .filter_map(|name| {
                let input = self.query_params.get(&name)?;
                Some(input.to_value(&name).map(|v| (name, v)))
            })
            .collect()
    }

    /// Executes `query_text` in the active tab, recording audit history and invalidating affected DDL slices.
    pub async fn execute_query_if_needed(
        &mut self,
        query_text: &str,
        is_default_select: bool,
    ) -> Result<Vec<QueryRowResult>, String> {
        let Some(query_id) = self.queries.current_query_id else {
            return Err("No active query tab".to_string());
        };
        let (db_name, is_executing) = {
            let q = self.queries.queries_by_id.get(&query_id).unwrap();
            (q.database.clone(), q.is_executing)
        };
        if is_executing {
            return Ok(Vec::new());
        }

        let safety = self.analyze_safety(query_text);
        let enforce = self.db.set_read_only(self.read_only_mode).await;
        if safety.is_blocked() || enforce.is_err() {
            let reason = match enforce {
                Err(e) => format!("Read-Only mode could not be enforced by the database: {e}"),
                Ok(()) => safety
                    .blocked_reason()
                    .unwrap_or_else(|| "Query blocked by Read-Only mode".to_string()),
            };
            if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
                q.results = None;
                q.is_executing = false;
                q.error = Some(reason.clone());
            }
            return Err(reason);
        }

        let params = match self.params_for_query(query_text) {
            Ok(p) => p,
            Err(reason) => {
                if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
                    q.results = None;
                    q.error = Some(reason.clone());
                }
                return Err(reason);
            }
        };
        let client = self
            .connections
            .server
            .as_ref()
            .map(|s| s.client.clone())
            .unwrap_or_default();
        let policy = self.history_policy();
        let display_sql = (!params.is_empty() && policy.store_bound_values)
            .then(|| bind_for_display(query_text, &params, &client));

        let start = Instant::now();
        if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
            q.copied = Some(false);
            q.is_executing = true;
            q.is_default_select = is_default_select;
            q.did_invalidate = false;
            q.execution_time = None;
            if !policy.excluded {
                q.query_history.push(query_text.to_string());
            }
        }
        self.status = "Executing query...".to_string();

        let (exec_res, page) = self
            .run_tab_query(&db_name, query_id, query_text, &params)
            .await;

        let elapsed_ms = start.elapsed().as_millis() as u64;
        self.status.clear();

        match exec_res {
            Ok(results) => {
                let total_rows: usize = results.iter().map(|r| r.rows.len()).sum();
                self.record_execution(
                    policy,
                    &db_name,
                    query_text,
                    display_sql,
                    elapsed_ms,
                    Ok(total_rows),
                );
                if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
                    q.error = None;
                    q.is_executing = false;
                    q.results = Some(std::sync::Arc::new(results.clone()));
                    q.execution_time = Some(elapsed_ms);
                    q.page = page;
                }
                for r in &results {
                    match r.command.as_str() {
                        "CREATE_DATABASE" | "DROP_DATABASE" => self.databases.did_invalidate = true,
                        "CREATE_SCHEMA" | "DROP_SCHEMA" => self.schemas.did_invalidate = true,
                        "CREATE_TABLE" | "DROP_TABLE" => self.tables.did_invalidate = true,
                        _ => {}
                    }
                }
                Ok(results)
            }
            Err(err) => {
                self.record_execution(
                    policy,
                    &db_name,
                    query_text,
                    display_sql,
                    elapsed_ms,
                    Err(err.clone()),
                );
                if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
                    q.results = None;
                    q.is_executing = false;
                    q.error = Some(err.clone());
                    q.page = None;
                }
                Err(err)
            }
        }
    }
}
