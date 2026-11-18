//! Query dispatch for [`DialectAdapter`]: runs a parameterized query on the live PostgreSQL /
//! Redshift server or on the opted-in emulator, binding parameters through prepared statements.

use crate::adapters::dialect_adapters::seed::Backend;
use crate::adapters::dialect_adapters::{
    execute_dialect_query_internal, execute_live_query_with_params, DialectAdapter,
    DialectCancellableQuery, PgCancellableQuery,
};
use crate::adapters::CancellableQuery;
use crate::common::types::QueryRowResult;
use crate::core::errors::DbCoreError;
use crate::core::params::QueryParams;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

impl DialectAdapter {
    /// Builds a cancellable handle that binds `params` on the live server or the emulator.
    pub(crate) fn cancellable_query(
        &self,
        query_text: &str,
        params: &QueryParams,
    ) -> Result<Arc<dyn CancellableQuery>, DbCoreError> {
        if self.adapter_key == "cassandra" {
            return Err(DbCoreError::CassandraCancelUnsupported);
        }
        let canceled = Arc::new(AtomicBool::new(false));
        Ok(match self.backend()? {
            Backend::Live(client) => Arc::new(PgCancellableQuery {
                client: client.clone(),
                query_text: query_text.to_string(),
                params: params.clone(),
                canceled,
            }),
            Backend::Emulated(e) => Arc::new(DialectCancellableQuery {
                adapter_key: self.adapter_key.clone(),
                emulated_conn: e.clone(),
                extra_databases: self.extra_databases.clone(),
                query_text: query_text.to_string(),
                params: params.clone(),
                canceled,
            }),
        })
    }

    /// Runs `query_text` to completion, binding `params` through prepared statements.
    pub(crate) async fn run_query(
        &self,
        query_text: &str,
        params: &QueryParams,
    ) -> Result<Vec<QueryRowResult>, DbCoreError> {
        match self.backend()? {
            Backend::Live(c) => execute_live_query_with_params(c, query_text, params).await,
            Backend::Emulated(e) => execute_dialect_query_internal(
                &self.adapter_key,
                e,
                &self.extra_databases,
                query_text,
                params,
            ),
        }
    }
}
