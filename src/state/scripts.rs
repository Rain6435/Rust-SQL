//! SQL DDL/DML script generation, default SELECT execution, query cancellation, safety checks,
//! and Mermaid Entity-Relationship Diagram (ERD) actions on [`AppStore`].

use crate::common::types::{ListTableColumnsResult, QueryRowResult, TableKeysResult};
use crate::core::result_ops::generate_mermaid_erd;
use crate::core::safety::{analyze_script, QuerySafetyReport, SafetyAnalysis, SqlDialect};
use crate::state::{AppStore, DbObjectType, ObjectScripts, ScriptActionType};
use std::collections::HashMap;

impl AppStore {
    /// Evaluates `query_text` against the destructive-query and read-only safety analyzer.
    pub fn check_query_safety(&self, query_text: &str) -> QuerySafetyReport {
        self.analyze_safety(query_text).to_report()
    }

    /// Structured safety analysis of `query_text`, parsed with the connected server's dialect.
    pub fn analyze_safety(&self, query_text: &str) -> SafetyAnalysis {
        let dialect = self
            .db
            .adapter_key()
            .map(SqlDialect::from_adapter_key)
            .unwrap_or(SqlDialect::Generic);
        analyze_script(query_text, dialect, self.read_only_mode)
    }

    /// Generates and executes a default `SELECT TOP N` query for `table` in `database`.
    pub async fn execute_default_select_query_if_needed(
        &mut self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<QueryRowResult>, String> {
        let query_default_select = self
            .db
            .get_query_select_top(database, table, schema, None)
            .await?;
        let need_new_query = match self.current_query() {
            Some(current) => {
                if current.is_executing {
                    return Ok(Vec::new());
                }
                current.database != database
                    || (current.query != query_default_select && !current.query.trim().is_empty())
            }
            None => true,
        };
        if need_new_query {
            self.add_new_query(database, Some(table));
        }
        self.update_query_if_needed(&query_default_select, Some(""), Some(table));
        self.execute_query_if_needed(&query_default_select, true)
            .await
    }

    /// Cancels an in-flight query registered under `query_id`.
    pub async fn cancel_query(&mut self, query_id: u64) -> Result<(), String> {
        if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
            q.is_canceling = true;
        }
        let res = self.db.cancel_cancellable_query(query_id).await;
        if let Some(q) = self.queries.queries_by_id.get_mut(&query_id) {
            q.is_canceling = false;
            if let Err(ref e) = res {
                q.is_executing = false;
                q.results = None;
                q.error = Some(e.clone());
            }
        }
        res
    }

    /// Generates a Mermaid `erDiagram` specification for all tables and foreign keys in `database`.
    pub async fn generate_database_erd(&mut self, database: &str) -> Result<String, String> {
        self.fetch_tables_if_needed(database).await?;
        let tables = self
            .tables
            .items_by_database
            .get(database)
            .cloned()
            .unwrap_or_default();

        let mut cols_map: HashMap<String, Vec<ListTableColumnsResult>> = HashMap::new();
        let mut keys_map: HashMap<String, Vec<TableKeysResult>> = HashMap::new();

        for tbl in tables {
            let cols = self
                .db
                .list_table_columns(database, &tbl.name, tbl.schema.as_deref())
                .await
                .unwrap_or_default();
            let keys = self
                .db
                .get_table_keys(database, &tbl.name, tbl.schema.as_deref())
                .await
                .unwrap_or_default();
            cols_map.insert(tbl.name.clone(), cols);
            keys_map.insert(tbl.name, keys);
        }

        Ok(generate_mermaid_erd(&cols_map, &keys_map))
    }

    /// Generates (or retrieves from cache) a DDL/DML script for `item` and appends it to the active query tab.
    pub async fn get_sql_script_if_needed(
        &mut self,
        database: &str,
        item: &str,
        action_type: ScriptActionType,
        object_type: DbObjectType,
        schema: Option<&str>,
    ) -> Result<String, String> {
        if !self.sqlscripts.did_invalidate {
            if let Some(cached) = self
                .sqlscripts
                .scripts_by_object
                .get(database)
                .and_then(|m| m.get(item))
                .and_then(|s| s.get(action_type))
            {
                let script = cached.to_string();
                self.append_query(&script);
                return Ok(script);
            }
        }

        self.sqlscripts.is_fetching = true;
        self.sqlscripts.did_invalidate = false;
        self.sqlscripts.error = None;

        let script_res: Result<String, String> = match (action_type, object_type) {
            (ScriptActionType::Create, DbObjectType::Table) => self
                .db
                .get_table_create_script(database, item, schema)
                .await
                .map(|v| v.into_iter().next().unwrap_or_default()),
            (ScriptActionType::Create, DbObjectType::View) => self
                .db
                .get_view_create_script(database, item, schema)
                .await
                .map(|v| v.into_iter().next().unwrap_or_default()),
            (ScriptActionType::Create, DbObjectType::Function | DbObjectType::Procedure) => self
                .db
                .get_routine_create_script(database, item, object_type.as_str(), schema)
                .await
                .map(|v| v.into_iter().next().unwrap_or_default()),
            (ScriptActionType::Select, _) => {
                self.db
                    .get_table_select_script(database, item, schema)
                    .await
            }
            (ScriptActionType::Insert, _) => {
                self.db
                    .get_table_insert_script(database, item, schema)
                    .await
            }
            (ScriptActionType::Update, _) => {
                self.db
                    .get_table_update_script(database, item, schema)
                    .await
            }
            (ScriptActionType::Delete, _) => {
                self.db
                    .get_table_delete_script(database, item, schema)
                    .await
            }
        };

        match script_res {
            Ok(script) => {
                self.sqlscripts.is_fetching = false;
                let entry = self
                    .sqlscripts
                    .scripts_by_object
                    .entry(database.to_string())
                    .or_default()
                    .entry(item.to_string())
                    .or_insert_with(|| ObjectScripts::new(object_type));
                entry.set(action_type, script.clone());
                self.append_query(&script);
                Ok(script)
            }
            Err(err) => {
                self.sqlscripts.is_fetching = false;
                self.sqlscripts.did_invalidate = true;
                self.sqlscripts.error = Some(err.clone());
                Err(err)
            }
        }
    }
}
