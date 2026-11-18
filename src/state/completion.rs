//! Autocompletion for the SQL editors, fed by the metadata cache (see [`crate::core::completion`]).

use crate::core::completion::{complete, referenced_tables, CompletionSchema, Completions};
use crate::state::AppStore;

impl AppStore {
    /// Tables and cached columns of `database`, for completion.
    pub fn completion_schema(&self, database: &str) -> CompletionSchema {
        let tables = self
            .tables
            .items_by_database
            .get(database)
            .map(|t| t.iter().map(|t| t.name.clone()).collect())
            .unwrap_or_default();
        let columns = self
            .columns
            .columns_by_table
            .get(database)
            .map(|m| {
                m.iter()
                    .map(|(t, cols)| (t.clone(), cols.iter().map(|c| c.name.clone()).collect()))
                    .collect()
            })
            .unwrap_or_default();
        CompletionSchema { tables, columns }
    }

    /// Loads (into the metadata cache) the columns of known tables that `text` references.
    /// Failures are ignored: completion then simply has fewer columns.
    pub async fn load_completion_columns(&mut self, database: &str, text: &str) {
        let known = self
            .tables
            .items_by_database
            .get(database)
            .cloned()
            .unwrap_or_default();
        for (name, _) in referenced_tables(text) {
            let Some(t) = known.iter().find(|t| t.name.eq_ignore_ascii_case(&name)) else {
                continue;
            };
            let cached = self
                .columns
                .columns_by_table
                .get(database)
                .is_some_and(|m| m.contains_key(&t.name));
            if !cached {
                let (table, schema) = (t.name.clone(), t.schema.clone());
                let _ = self
                    .fetch_table_columns_if_needed(database, &table, schema.as_deref())
                    .await;
            }
        }
    }

    /// Suggestions for the cursor (a char index) in `text`, or none when autocomplete is off.
    /// `explicit` is a user request (Ctrl+Space); otherwise live autocomplete must be on.
    pub fn completions(
        &self,
        database: &str,
        text: &str,
        cursor: usize,
        explicit: bool,
    ) -> Completions {
        let q = &self.queries;
        if !q.enabled_auto_complete || (!explicit && !q.enabled_live_auto_complete) {
            return Completions::default();
        }
        complete(text, cursor, &self.completion_schema(database), explicit)
    }
}
