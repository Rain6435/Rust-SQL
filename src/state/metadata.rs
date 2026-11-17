//! Database-level metadata fetching actions (databases, schemas, tables, views, routines) on [`AppStore`].

use crate::state::{AppStore, NamedItem, RoutineItem};

impl AppStore {
    /// Marks all schema, table, column, trigger, index, view, routine, and script caches as stale.
    pub fn refresh_database(&mut self, _database: &str) {
        self.schemas.did_invalidate = true;
        self.tables.did_invalidate = true;
        self.columns.did_invalidate = true;
        self.triggers.did_invalidate = true;
        self.indexes.did_invalidate = true;
        self.views.did_invalidate = true;
        self.routines.did_invalidate = true;
        self.sqlscripts.did_invalidate = true;
    }

    /// Fetches the list of databases on the connected server if the cache is invalidated.
    pub async fn fetch_databases_if_needed(&mut self) -> Result<(), String> {
        if self.databases.is_fetching || !self.databases.did_invalidate {
            return Ok(());
        }
        let Some(db_name) = self.current_database() else {
            return Ok(());
        };
        let filter = self
            .connections
            .server
            .as_ref()
            .and_then(|s| s.filter.as_ref())
            .map(|f| f.to_database_filter());

        self.databases.is_fetching = true;
        self.databases.did_invalidate = false;
        self.databases.error = None;

        match self.db.list_databases(&db_name, filter.as_ref()).await {
            Ok(list) => {
                self.databases.is_fetching = false;
                self.databases.items = list.into_iter().map(|name| NamedItem { name }).collect();
                Ok(())
            }
            Err(err) => {
                self.databases.is_fetching = false;
                self.databases.did_invalidate = true;
                self.databases.error = Some(err.clone());
                Err(err)
            }
        }
    }

    /// Fetches schemas for `database` if not yet cached or invalidated.
    pub async fn fetch_schemas_if_needed(&mut self, database: &str) -> Result<(), String> {
        let should_fetch = !self.schemas.is_fetching
            && (!self.schemas.items_by_database.contains_key(database)
                || self.schemas.did_invalidate);
        if !should_fetch {
            return Ok(());
        }
        self.schemas.is_fetching = true;
        self.schemas.did_invalidate = false;
        self.schemas.error = None;

        match self.db.list_schemas(database, None).await {
            Ok(list) => {
                self.schemas.is_fetching = false;
                self.schemas.items_by_database.insert(
                    database.to_string(),
                    list.into_iter().map(|name| NamedItem { name }).collect(),
                );
                Ok(())
            }
            Err(err) => {
                self.schemas.is_fetching = false;
                self.schemas.did_invalidate = true;
                self.schemas.error = Some(err.clone());
                Err(err)
            }
        }
    }

    /// Fetches tables for `database` if not yet cached or invalidated.
    pub async fn fetch_tables_if_needed(&mut self, database: &str) -> Result<(), String> {
        let should_fetch = !self.tables.is_fetching
            && (!self.tables.items_by_database.contains_key(database)
                || self.tables.did_invalidate);
        if !should_fetch {
            return Ok(());
        }
        let filter = self
            .connections
            .server
            .as_ref()
            .and_then(|s| s.filter.as_ref())
            .map(|f| f.to_schema_filter());

        self.tables.is_fetching = true;
        self.tables.did_invalidate = false;
        self.tables.error = None;
        self.status = "Loading list of tables...".to_string();

        match self.db.list_tables(database, filter.as_ref()).await {
            Ok(list) => {
                self.tables.is_fetching = false;
                self.tables
                    .items_by_database
                    .insert(database.to_string(), list);
                self.status.clear();
                Ok(())
            }
            Err(err) => {
                self.tables.is_fetching = false;
                self.tables.did_invalidate = true;
                self.tables.error = Some(err.clone());
                self.status.clear();
                Err(err)
            }
        }
    }

    /// Fetches views for `database` if not yet cached or invalidated.
    pub async fn fetch_views_if_needed(&mut self, database: &str) -> Result<(), String> {
        let should_fetch = !self.views.is_fetching
            && (!self.views.views_by_database.contains_key(database) || self.views.did_invalidate);
        if !should_fetch {
            return Ok(());
        }
        let filter = self
            .connections
            .server
            .as_ref()
            .and_then(|s| s.filter.as_ref())
            .map(|f| f.to_schema_filter());

        self.views.is_fetching = true;
        self.views.did_invalidate = false;
        self.views.error = None;

        match self.db.list_views(database, filter.as_ref()).await {
            Ok(list) => {
                self.views.is_fetching = false;
                self.views
                    .views_by_database
                    .insert(database.to_string(), list);
                Ok(())
            }
            Err(err) => {
                self.views.is_fetching = false;
                self.views.did_invalidate = true;
                self.views.error = Some(err.clone());
                Err(err)
            }
        }
    }

    /// Fetches functions and stored procedures for `database` if not yet cached or invalidated.
    pub async fn fetch_routines_if_needed(&mut self, database: &str) -> Result<(), String> {
        let should_fetch = !self.routines.is_fetching
            && (!self.routines.functions_by_database.contains_key(database)
                || !self.routines.procedures_by_database.contains_key(database)
                || self.routines.did_invalidate);
        if !should_fetch {
            return Ok(());
        }
        let filter = self
            .connections
            .server
            .as_ref()
            .and_then(|s| s.filter.as_ref())
            .map(|f| f.to_schema_filter());

        self.routines.is_fetching = true;
        self.routines.did_invalidate = false;
        self.routines.error = None;

        match self.db.list_routines(database, filter.as_ref()).await {
            Ok(list) => {
                self.routines.is_fetching = false;
                let mut funcs = Vec::new();
                let mut procs = Vec::new();
                for r in list {
                    let item = RoutineItem {
                        schema: r.schema,
                        name: r.routine_name,
                        routine_definition: None,
                    };
                    if r.routine_type == "FUNCTION" {
                        funcs.push(item);
                    } else if r.routine_type == "PROCEDURE" {
                        procs.push(item);
                    }
                }
                self.routines
                    .functions_by_database
                    .insert(database.to_string(), funcs);
                self.routines
                    .procedures_by_database
                    .insert(database.to_string(), procs);
                Ok(())
            }
            Err(err) => {
                self.routines.is_fetching = false;
                self.routines.did_invalidate = true;
                self.routines.error = Some(err.clone());
                Err(err)
            }
        }
    }
}
