//! Query execution, DDL/DML script generation, and file/clipboard export methods for [`DatabaseFacade`].

use crate::adapters::facade::{set_select_limit, DatabaseFacade};
use crate::common::types::{OpenedQueryInfo, QueryRowResult, SavedQueryInfo};
use crate::core::export::{
    export_query_result_to_clipboard, export_query_result_to_file_path, open_query_from_file,
    save_query_to_file,
};
use serde_json::{Map, Value};

impl DatabaseFacade {
    /// Registers a cancellable query under `query_id`.
    pub fn create_cancellable_query(
        &mut self,
        database: &str,
        query_id: u64,
        query_text: &str,
    ) -> Result<(), String> {
        let q = self
            .get_db(database)?
            .query(query_text)
            .map_err(|e| e.to_string())?;
        self.cancellable_queries.insert(query_id, q);
        Ok(())
    }

    /// Cancels and removes a registered cancellable query.
    pub async fn cancel_cancellable_query(&mut self, query_id: u64) -> Result<(), String> {
        if let Some(q) = self.cancellable_queries.remove(&query_id) {
            q.cancel().await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Executes and removes a registered cancellable query.
    pub async fn execute_cancellable_query(
        &mut self,
        query_id: u64,
    ) -> Result<Vec<QueryRowResult>, String> {
        let Some(q) = self.cancellable_queries.remove(&query_id) else {
            return Ok(Vec::new());
        };
        q.execute().await.map_err(|e| e.to_string())
    }

    /// Executes `query_text` on `database`.
    pub async fn execute_query(
        &self,
        database: &str,
        query_text: &str,
    ) -> Result<Vec<QueryRowResult>, String> {
        self.get_db(database)?
            .execute_query(query_text)
            .await
            .map_err(|e| e.to_string())
    }

    /// Generates a `SELECT TOP` / `LIMIT` query for `table` in `database`.
    pub async fn get_query_select_top(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
        limit: Option<u32>,
    ) -> Result<String, String> {
        self.get_db(database)?
            .get_query_select_top(table, schema, limit)
            .await
            .map_err(|e| e.to_string())
    }

    /// Retrieves `CREATE TABLE` DDL script(s) for `table` in `database`.
    pub async fn get_table_create_script(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .get_table_create_script(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Generates a `SELECT` template script for `table` in `database`.
    pub async fn get_table_select_script(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, String> {
        self.get_db(database)?
            .get_table_select_script(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Generates an `INSERT` template script for `table` in `database`.
    pub async fn get_table_insert_script(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, String> {
        self.get_db(database)?
            .get_table_insert_script(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Generates an `UPDATE` template script for `table` in `database`.
    pub async fn get_table_update_script(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, String> {
        self.get_db(database)?
            .get_table_update_script(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Generates a `DELETE` template script for `table` in `database`.
    pub async fn get_table_delete_script(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, String> {
        self.get_db(database)?
            .get_table_delete_script(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Retrieves `CREATE VIEW` DDL script(s) for `view` in `database`.
    pub async fn get_view_create_script(
        &self,
        database: &str,
        view: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .get_view_create_script(view, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Retrieves `CREATE FUNCTION` / `CREATE PROCEDURE` DDL script(s) for `routine` in `database`.
    pub async fn get_routine_create_script(
        &self,
        database: &str,
        routine: &str,
        routine_type: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .get_routine_create_script(routine, routine_type, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Truncates all user tables in `database`.
    pub async fn truncate_all_tables(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<(), String> {
        self.get_db(database)?
            .truncate_all_tables(schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Returns column names for `table` in `database`.
    pub async fn get_table_column_names(
        &self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, String> {
        self.get_db(database)?
            .get_table_column_names(table, schema)
            .await
            .map_err(|e| e.to_string())
    }

    /// Sets the process-wide `SELECT` row limit.
    pub fn set_select_limit(&self, limit: u32) {
        set_select_limit(limit);
    }

    /// Exports `rows` to `filename` in `export_type` format.
    pub fn export_query_result_to_file(
        &self,
        rows: &[Map<String, Value>],
        export_type: &str,
        delimiter: Option<&str>,
        filename: &str,
    ) -> Result<String, String> {
        export_query_result_to_file_path(rows, export_type, delimiter, filename)
    }

    /// Formats `rows` for clipboard export in `export_type` format.
    pub fn export_query_result_to_clipboard(
        &self,
        rows: &[Map<String, Value>],
        export_type: &str,
        delimiter: Option<&str>,
    ) -> Result<String, String> {
        export_query_result_to_clipboard(rows, export_type, delimiter)
    }

    /// Saves `query` text to `filename`.
    pub fn save_query(&self, filename: &str, query: &str) -> Result<SavedQueryInfo, String> {
        save_query_to_file(filename, query)
    }

    /// Reads a SQL query from `filename`.
    pub fn open_query(&self, filename: &str) -> Result<OpenedQueryInfo, String> {
        open_query_from_file(filename)
    }
}
