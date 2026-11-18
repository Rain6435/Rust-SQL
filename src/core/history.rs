//! Query history privacy policy.
//!
//! History keeps the query exactly as written, with its placeholders (`:email`, `$1`), and never
//! the parameter values, unless the user opts in with `historyStoreBoundValues`. A connection
//! marked `excludeFromHistory` records nothing at all. Note that values typed directly into the
//! SQL text (rather than passed as parameters) are part of the query and are kept.

use crate::common::types::{Config, Server};
use crate::core::bookmarks::{BookmarkManager, QueryHistoryEntry};

/// What query history may record for one execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HistoryPolicy {
    /// Record nothing (the connection is excluded from history).
    pub excluded: bool,
    /// Also keep the display-only query with parameter values filled in.
    pub store_bound_values: bool,
}

impl HistoryPolicy {
    /// The policy for `server` under `config` (defaults when either is missing).
    pub fn for_connection(config: Option<&Config>, server: Option<&Server>) -> Self {
        Self {
            excluded: server.is_some_and(Server::is_excluded_from_history),
            store_bound_values: config.is_some_and(|c| c.history_store_bound_values),
        }
    }
}

/// One finished execution, as offered to [`BookmarkManager::record_execution`].
#[derive(Debug, Clone)]
pub struct ExecutionRecord<'a> {
    /// Target database.
    pub database: &'a str,
    /// The query as written, with placeholders.
    pub sql: &'a str,
    /// The query with parameter values filled in (display only), if it had parameters.
    pub display_sql: Option<String>,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Rows returned across all statements.
    pub row_count: usize,
    /// Error message when the execution failed.
    pub error: Option<String>,
}

impl BookmarkManager {
    /// Records `record` in history as `policy` allows, returning the stored entry (if any).
    pub fn record_execution(
        &mut self,
        policy: HistoryPolicy,
        record: ExecutionRecord<'_>,
    ) -> Option<QueryHistoryEntry> {
        if policy.excluded {
            return None;
        }
        self.record_history(
            record.database,
            record.sql,
            record.duration_ms,
            record.row_count,
            record.error,
        );
        let display = record.display_sql.filter(|_| policy.store_bound_values);
        self.set_latest_display_sql(display);
        self.history.first().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(display: Option<&str>) -> ExecutionRecord<'static> {
        ExecutionRecord {
            database: "crm",
            sql: "SELECT * FROM users WHERE email = :email",
            display_sql: display.map(str::to_string),
            duration_ms: 3,
            row_count: 1,
            error: None,
        }
    }

    const BOUND: &str = "SELECT * FROM users WHERE email = 'alice@example.com'";

    #[test]
    fn default_policy_keeps_placeholders_and_drops_values() {
        let mut mgr = BookmarkManager::new();
        let entry = mgr
            .record_execution(HistoryPolicy::default(), record(Some(BOUND)))
            .unwrap();
        assert_eq!(entry.sql, "SELECT * FROM users WHERE email = :email");
        assert_eq!(entry.display_sql, None);
        let json = serde_json::to_string(&mgr.history).unwrap();
        assert!(!json.contains("alice@example.com"), "{json}");
    }

    #[test]
    fn opt_in_keeps_the_display_only_bound_query() {
        let mut mgr = BookmarkManager::new();
        let policy = HistoryPolicy {
            store_bound_values: true,
            ..Default::default()
        };
        let entry = mgr.record_execution(policy, record(Some(BOUND))).unwrap();
        assert_eq!(entry.display_sql.as_deref(), Some(BOUND));
    }

    #[test]
    fn excluded_connection_records_nothing() {
        let mut mgr = BookmarkManager::new();
        let policy = HistoryPolicy {
            excluded: true,
            store_bound_values: true,
        };
        assert!(mgr.record_execution(policy, record(Some(BOUND))).is_none());
        assert!(mgr.history.is_empty());
    }

    #[test]
    fn policy_comes_from_the_server_flag_and_the_config_option() {
        let server = Server {
            exclude_from_history: Some(true),
            ..Default::default()
        };
        let config = Config {
            history_store_bound_values: true,
            ..Default::default()
        };
        let p = HistoryPolicy::for_connection(Some(&config), Some(&server));
        assert!(p.excluded && p.store_bound_values);
        assert_eq!(
            HistoryPolicy::for_connection(None, None),
            HistoryPolicy::default()
        );
    }
}
