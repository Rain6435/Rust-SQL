//! Table-level metadata fetching actions (columns, triggers, indexes) on [`AppStore`].

use crate::common::types::ListTableColumnsResult;
use crate::state::{AppStore, ColumnItem, NamedItem};

impl AppStore {
    /// Fetches columns and data types for `table` in `database` if not yet cached or invalidated.
    pub async fn fetch_table_columns_if_needed(
        &mut self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<(), String> {
        let is_fetching = self
            .columns
            .is_fetching
            .get(database)
            .and_then(|m| m.get(table))
            .copied()
            .unwrap_or(false);
        let has_cols = self
            .columns
            .columns_by_table
            .get(database)
            .and_then(|m| m.get(table))
            .is_some();
        if is_fetching || (has_cols && !self.columns.did_invalidate) {
            return Ok(());
        }

        self.columns
            .is_fetching
            .entry(database.to_string())
            .or_default()
            .insert(table.to_string(), true);
        self.columns.did_invalidate = false;
        self.columns.error = None;

        match self.db.list_table_columns(database, table, schema).await {
            Ok(cols) => {
                self.columns
                    .is_fetching
                    .entry(database.to_string())
                    .or_default()
                    .insert(table.to_string(), false);
                let mapped: Vec<ColumnItem> = cols
                    .into_iter()
                    .map(|c: ListTableColumnsResult| ColumnItem {
                        name: c.column_name,
                        data_type: c.data_type,
                    })
                    .collect();
                self.columns
                    .columns_by_table
                    .entry(database.to_string())
                    .or_default()
                    .insert(table.to_string(), mapped);
                Ok(())
            }
            Err(err) => {
                self.columns
                    .is_fetching
                    .entry(database.to_string())
                    .or_default()
                    .insert(table.to_string(), false);
                self.columns.did_invalidate = true;
                self.columns.error = Some(err.clone());
                Err(err)
            }
        }
    }

    /// Fetches triggers for `table` in `database` if not yet cached or invalidated.
    pub async fn fetch_table_triggers_if_needed(
        &mut self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<(), String> {
        let has_triggers = self
            .triggers
            .triggers_by_table
            .get(database)
            .and_then(|m| m.get(table))
            .is_some();
        if self.triggers.is_fetching || (has_triggers && !self.triggers.did_invalidate) {
            return Ok(());
        }

        self.triggers.is_fetching = true;
        self.triggers.did_invalidate = false;
        self.triggers.error = None;

        match self.db.list_table_triggers(database, table, schema).await {
            Ok(list) => {
                self.triggers.is_fetching = false;
                self.triggers
                    .triggers_by_table
                    .entry(database.to_string())
                    .or_default()
                    .insert(
                        table.to_string(),
                        list.into_iter().map(|name| NamedItem { name }).collect(),
                    );
                Ok(())
            }
            Err(err) => {
                self.triggers.is_fetching = false;
                self.triggers.did_invalidate = true;
                self.triggers.error = Some(err.clone());
                Err(err)
            }
        }
    }

    /// Fetches indexes for `table` in `database` if not yet cached or invalidated.
    pub async fn fetch_table_indexes_if_needed(
        &mut self,
        database: &str,
        table: &str,
        schema: Option<&str>,
    ) -> Result<(), String> {
        let has_indexes = self
            .indexes
            .indexes_by_table
            .get(database)
            .and_then(|m| m.get(table))
            .is_some();
        if self.indexes.is_fetching || (has_indexes && !self.indexes.did_invalidate) {
            return Ok(());
        }

        self.indexes.is_fetching = true;
        self.indexes.did_invalidate = false;
        self.indexes.error = None;

        match self.db.list_table_indexes(database, table, schema).await {
            Ok(list) => {
                self.indexes.is_fetching = false;
                self.indexes
                    .indexes_by_table
                    .entry(database.to_string())
                    .or_default()
                    .insert(
                        table.to_string(),
                        list.into_iter().map(|name| NamedItem { name }).collect(),
                    );
                Ok(())
            }
            Err(err) => {
                self.indexes.is_fetching = false;
                self.indexes.did_invalidate = true;
                self.indexes.error = Some(err.clone());
                Err(err)
            }
        }
    }
}
