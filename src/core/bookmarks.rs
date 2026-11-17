//! Saved SQL bookmark/snippet library and searchable query execution audit log.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Reusable SQL query bookmark with tags and optional target database client type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlBookmark {
    /// Unique UUID identifier.
    pub id: String,
    /// Human-readable title for the bookmark.
    pub title: String,
    /// SQL statement or script body.
    pub sql: String,
    /// User-assigned searchable tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional database client key (`postgresql`, `mysql`, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
}

impl SqlBookmark {
    /// Creates a new [`SqlBookmark`] with a generated UUID.
    pub fn new(title: &str, sql: &str, tags: Vec<String>, client: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title: title.trim().to_string(),
            sql: sql.to_string(),
            tags,
            client,
        }
    }
}

/// Audit log entry recording a single SQL query execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHistoryEntry {
    /// Monotonically increasing execution identifier.
    pub id: u64,
    /// Target database name.
    pub database: String,
    /// Executed SQL text.
    pub sql: String,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Total rows returned across all statements.
    pub row_count: usize,
    /// Whether the query completed without error.
    pub success: bool,
    /// Error message if execution failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Manager for saved [`SqlBookmark`] items and recent [`QueryHistoryEntry`] audit records.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkManager {
    /// Saved SQL bookmarks.
    pub bookmarks: Vec<SqlBookmark>,
    /// Recent query execution history entries (newest first, capped at 500).
    pub history: Vec<QueryHistoryEntry>,
    next_history_id: u64,
}

impl BookmarkManager {
    /// Creates an empty [`BookmarkManager`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates and stores a new [`SqlBookmark`].
    pub fn add_bookmark(
        &mut self,
        title: &str,
        sql: &str,
        tags: Vec<String>,
        client: Option<String>,
    ) -> SqlBookmark {
        let bm = SqlBookmark::new(title, sql, tags, client);
        self.bookmarks.push(bm.clone());
        bm
    }

    /// Removes a bookmark by its `id`, returning `true` if an entry was removed.
    pub fn remove_bookmark(&mut self, id: &str) -> bool {
        let before = self.bookmarks.len();
        self.bookmarks.retain(|b| b.id != id);
        self.bookmarks.len() < before
    }

    /// Filters bookmarks case-insensitively by `title`, `sql`, or `tags`.
    pub fn search_bookmarks(&self, filter: &str) -> Vec<&SqlBookmark> {
        let q = filter.trim().to_lowercase();
        if q.is_empty() {
            return self.bookmarks.iter().collect();
        }
        self.bookmarks
            .iter()
            .filter(|b| {
                b.title.to_lowercase().contains(&q)
                    || b.sql.to_lowercase().contains(&q)
                    || b.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// Appends a new [`QueryHistoryEntry`] to the front of the history log (retaining up to 500 entries).
    pub fn record_history(
        &mut self,
        database: &str,
        sql: &str,
        duration_ms: u64,
        row_count: usize,
        error: Option<String>,
    ) -> QueryHistoryEntry {
        self.next_history_id += 1;
        let entry = QueryHistoryEntry {
            id: self.next_history_id,
            database: database.to_string(),
            sql: sql.to_string(),
            duration_ms,
            row_count,
            success: error.is_none(),
            error,
        };
        self.history.insert(0, entry.clone());
        if self.history.len() > 500 {
            self.history.truncate(500);
        }
        entry
    }

    /// Filters execution history entries case-insensitively by `sql` or `database`.
    pub fn search_history(&self, filter: &str) -> Vec<&QueryHistoryEntry> {
        let q = filter.trim().to_lowercase();
        if q.is_empty() {
            return self.history.iter().collect();
        }
        self.history
            .iter()
            .filter(|h| h.sql.to_lowercase().contains(&q) || h.database.to_lowercase().contains(&q))
            .collect()
    }

    /// Clears all recorded execution history entries.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bookmark_and_history_manager() {
        let mut mgr = BookmarkManager::new();
        let bm = mgr.add_bookmark(
            "Active Users",
            "SELECT * FROM users WHERE active = 1;",
            vec!["users".to_string(), "prod".to_string()],
            Some("postgresql".to_string()),
        );
        assert_eq!(mgr.search_bookmarks("prod").len(), 1);
        assert_eq!(mgr.search_bookmarks("missing").len(), 0);
        assert!(mgr.remove_bookmark(&bm.id));
        assert!(mgr.bookmarks.is_empty());

        mgr.record_history("company", "SELECT 1;", 12, 1, None);
        mgr.record_history(
            "company",
            "SELECT * FROM bad_table;",
            5,
            0,
            Some("no such table".to_string()),
        );
        assert_eq!(mgr.history.len(), 2);
        assert!(!mgr.history[0].success);
        assert!(mgr.history[1].success);
        assert_eq!(mgr.search_history("bad_table").len(), 1);
    }
}
