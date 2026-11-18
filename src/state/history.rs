//! Query history recording for [`AppStore`], following the privacy [`HistoryPolicy`].

use crate::core::history::{ExecutionRecord, HistoryPolicy};
use crate::state::AppStore;

impl AppStore {
    /// The history policy for the connected server under the loaded configuration.
    pub fn history_policy(&self) -> HistoryPolicy {
        HistoryPolicy::for_connection(self.config.data.as_ref(), self.connections.server.as_ref())
    }

    /// Records one finished execution in the bookmark manager's history, as `policy` allows.
    pub(crate) fn record_execution(
        &mut self,
        policy: HistoryPolicy,
        database: &str,
        sql: &str,
        display_sql: Option<String>,
        duration_ms: u64,
        outcome: Result<usize, String>,
    ) {
        let (row_count, error) = match outcome {
            Ok(rows) => (rows, None),
            Err(e) => (0, Some(e)),
        };
        self.bookmark_manager.record_execution(
            policy,
            ExecutionRecord {
                database,
                sql,
                display_sql,
                duration_ms,
                row_count,
                error,
            },
        );
    }
}
