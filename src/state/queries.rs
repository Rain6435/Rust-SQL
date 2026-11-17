//! SQL query tab management, parameter binding, and query execution actions on [`AppStore`].

use crate::common::types::QueryRowResult;
use crate::core::params::bind_query_parameters;
use crate::core::safety::analyze_query_safety;
use crate::state::{AppStore, QueryTab};
use serde_json::Value;
use std::collections::HashMap;
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

        let safety = analyze_query_safety(query_text, self.read_only_mode);
        if self.read_only_mode && !safety.is_read_only_safe {
            let reason = safety
                .findings
                .first()
                .map(|f| f.reason.clone())
                .unwrap_or_else(|| "Query blocked by Read-Only mode".to_string());
            if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
                q.results = None;
                q.is_executing = false;
                q.error = Some(reason.clone());
            }
            return Err(reason);
        }

        let bound_query = if self.query_params.is_empty() {
            query_text.to_string()
        } else {
            let param_vals: HashMap<String, Value> = self
                .query_params
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            bind_query_parameters(query_text, &param_vals)
        };

        let start = Instant::now();
        if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
            q.copied = Some(false);
            q.is_executing = true;
            q.is_default_select = is_default_select;
            q.did_invalidate = false;
            q.execution_time = None;
            q.query_history.push(bound_query.clone());
        }
        self.status = "Executing query...".to_string();

        let can_cancel = !self
            .connections
            .disabled_features
            .iter()
            .any(|f| f == "cancelQuery");
        let exec_res = if can_cancel {
            self.db
                .create_cancellable_query(&db_name, query_id, &bound_query)?;
            self.db.execute_cancellable_query(query_id).await
        } else {
            self.db.execute_query(&db_name, &bound_query).await
        };

        let elapsed_ms = start.elapsed().as_millis() as u64;
        self.status.clear();

        match exec_res {
            Ok(results) => {
                let total_rows: usize = results.iter().map(|r| r.rows.len()).sum();
                self.bookmark_manager.record_history(
                    &db_name,
                    &bound_query,
                    elapsed_ms,
                    total_rows,
                    None,
                );
                if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
                    q.error = None;
                    q.is_executing = false;
                    q.results = Some(results.clone());
                    q.execution_time = Some(elapsed_ms);
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
                self.bookmark_manager.record_history(
                    &db_name,
                    &bound_query,
                    elapsed_ms,
                    0,
                    Some(err.clone()),
                );
                if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
                    q.results = None;
                    q.is_executing = false;
                    q.error = Some(err.clone());
                }
                Err(err)
            }
        }
    }
}
