//! SQL script generation and table truncation helpers for [`DatabaseConnection`].

use crate::adapters::facade::DatabaseConnection;
use crate::core::errors::DbCoreError;

impl DatabaseConnection {
    pub(crate) fn resolve_schema(&self, schema: Option<&str>) -> String {
        match schema.filter(|s| !s.is_empty()) {
            Some(s) => format!("{}.", self.wrap(s)),
            None => String::new(),
        }
    }

    /// Returns only the column names for `table`.
    pub async fn get_table_column_names(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        let cols = self.list_table_columns(table, schema).await?;
        Ok(cols.into_iter().map(|c| c.column_name).collect())
    }

    /// Retrieves `CREATE TABLE` DDL script(s) for `table`.
    pub async fn get_table_create_script(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .get_table_create_script(table, schema)
            .await
    }

    /// Generates `SELECT <col1>, <col2> FROM [schema.]<table>;`.
    pub async fn get_table_select_script(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        let column_names = self.get_table_column_names(table, schema).await?;
        let wrapped_cols = column_names
            .iter()
            .map(|c| self.wrap(c))
            .collect::<Vec<_>>()
            .join(", ");
        let schema_selection = self.resolve_schema(schema);
        Ok(format!(
            "SELECT {} FROM {}{};",
            wrapped_cols,
            schema_selection,
            self.wrap(table)
        ))
    }

    /// Generates `INSERT INTO [schema.]<table> (<col1>, <col2>)\n VALUES (?, ?);`.
    pub async fn get_table_insert_script(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        let column_names = self.get_table_column_names(table, schema).await?;
        let wrapped_cols = column_names
            .iter()
            .map(|c| self.wrap(c))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = vec!["?"; column_names.len()].join(", ");
        let schema_selection = self.resolve_schema(schema);
        Ok(format!(
            "INSERT INTO {}{} ({})\n VALUES ({});",
            schema_selection,
            self.wrap(table),
            wrapped_cols,
            placeholders
        ))
    }

    /// Generates `UPDATE [schema.]<table>\n SET <col1>=?, <col2>=?\n WHERE <condition>;`.
    pub async fn get_table_update_script(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        let column_names = self.get_table_column_names(table, schema).await?;
        let set_cols = column_names
            .iter()
            .map(|c| format!("{}=?", self.wrap(c)))
            .collect::<Vec<_>>()
            .join(", ");
        let schema_selection = self.resolve_schema(schema);
        Ok(format!(
            "UPDATE {}{}\n SET {}\n WHERE <condition>;",
            schema_selection,
            self.wrap(table),
            set_cols
        ))
    }

    /// Generates `DELETE FROM [schema.]<table> WHERE <condition>;`.
    pub async fn get_table_delete_script(
        &self,
        table: &str,
        schema: Option<&str>,
    ) -> Result<String, DbCoreError> {
        let schema_selection = self.resolve_schema(schema);
        Ok(format!(
            "DELETE FROM {}{} WHERE <condition>;",
            schema_selection,
            self.wrap(table)
        ))
    }

    /// Retrieves `CREATE VIEW` DDL script(s) for `view`.
    pub async fn get_view_create_script(
        &self,
        view: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .get_view_create_script(view, schema)
            .await
    }

    /// Retrieves `CREATE FUNCTION` / `CREATE PROCEDURE` DDL script(s) for `routine`.
    pub async fn get_routine_create_script(
        &self,
        routine: &str,
        routine_type: &str,
        schema: Option<&str>,
    ) -> Result<Vec<String>, DbCoreError> {
        self.check_is_connected()?
            .get_routine_create_script(routine, routine_type, schema)
            .await
    }

    /// Truncates or deletes all rows from all user tables in `schema`.
    pub async fn truncate_all_tables(&self, schema: Option<&str>) -> Result<(), DbCoreError> {
        self.check_is_connected()?.truncate_all_tables(schema).await
    }
}
