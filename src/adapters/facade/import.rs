//! Data import entry points on [`DatabaseConnection`] and [`DatabaseFacade`].

use crate::adapters::facade::{DatabaseConnection, DatabaseFacade};
use crate::core::errors::DbCoreError;
use crate::core::import::{run_import, ImportReport, ImportRequest};

impl DatabaseConnection {
    /// Imports rows into an existing table in one transaction (see [`crate::core::import`]).
    pub async fn import_rows(&self, req: ImportRequest<'_>) -> Result<ImportReport, DbCoreError> {
        let adapter = self.check_is_connected()?;
        run_import(adapter, &self.adapter_key, req).await
    }
}

impl DatabaseFacade {
    /// Imports rows into a table of `database` (see [`DatabaseConnection::import_rows`]).
    pub async fn import_rows(
        &self,
        database: &str,
        req: ImportRequest<'_>,
    ) -> Result<ImportReport, String> {
        self.get_db(database)?
            .import_rows(req)
            .await
            .map_err(|e| e.to_string())
    }
}
