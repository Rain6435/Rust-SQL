use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlBookmark {
    pub id: String,
    pub title: String,
    pub sql: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
}

impl SqlBookmark {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHistoryEntry {
    pub id: u64,
    pub database: String,
    pub sql: String,
    pub duration_ms: u64,
    pub row_count: usize,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkManager {
    pub bookmarks: Vec<SqlBookmark>,
    pub history: Vec<QueryHistoryEntry>,
    next_history_id: u64,
}

impl BookmarkManager {
    pub fn new() -> Self {
        Self::default()
    }

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

    pub fn remove_bookmark(&mut self, id: &str) -> bool {
        let before = self.bookmarks.len();
        self.bookmarks.retain(|b| b.id != id);
        self.bookmarks.len() < before
    }

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
