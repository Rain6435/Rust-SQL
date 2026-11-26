//! State slice structures for SQL query tabs, execution results, and cached DDL/DML object scripts.

use crate::common::types::QueryRowResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents an open SQL editor tab and its execution state/results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryTab {
    /// Unique numeric tab ID.
    pub id: u64,
    /// Target database name for this tab.
    pub database: String,
    /// Display title of the tab.
    pub name: String,
    /// Optional file path if the tab is backed by a saved `.sql` file.
    pub filename: Option<String>,
    /// Whether a query is currently executing in this tab.
    pub is_executing: bool,
    /// Whether the last executed query was an auto-generated default `SELECT TOP` query.
    pub is_default_select: bool,
    /// Whether the tab's state was invalidated.
    pub did_invalidate: bool,
    /// Full SQL text in the editor buffer.
    pub query: String,
    /// Currently highlighted/selected SQL text in the editor, if any.
    pub selected_query: String,
    /// Per-tab history of executed SQL statements.
    pub query_history: Vec<String>,
    /// Statement result sets from the most recent execution.
    pub results: Option<std::sync::Arc<Vec<QueryRowResult>>>,
    /// Error message from the most recent execution, if it failed.
    pub error: Option<String>,
    /// Whether the most recent clipboard copy succeeded.
    pub copied: Option<bool>,
    /// Whether the most recent file save succeeded.
    pub saved: Option<bool>,
    /// Whether cancellation has been requested for the active query.
    pub is_canceling: bool,
    /// Number of result rows displayed per page.
    pub result_items_per_page: u32,
    /// Elapsed execution time in milliseconds for the last run.
    pub execution_time: Option<u64>,
    /// Paging state when the last result was fetched page by page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<TabPage>,
}

/// Paging state of a tab whose result is a single `SELECT` fetched page by page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabPage {
    /// The query as written (each page wraps it, see [`crate::core::paging`]).
    pub query: String,
    /// Parameter values the first page ran with, reused for later pages.
    #[serde(skip)]
    pub params: crate::core::params::QueryParams,
    /// Rows per page.
    pub page_size: u32,
    /// The next page, or `None` when every row is loaded.
    pub next: Option<crate::core::paging::PageCursor>,
}

/// State slice managing all open SQL query tabs and editor preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryState {
    /// Monotonically increasing counter for assigning new tab IDs.
    pub last_created_id: u64,
    /// ID of the currently active query tab.
    pub current_query_id: Option<u64>,
    /// Ordered list of open query tab IDs.
    pub query_ids: Vec<u64>,
    /// Map of tab ID to [`QueryTab`].
    pub queries_by_id: HashMap<u64, QueryTab>,
    /// Default pagination size for result tables.
    pub result_items_per_page: u32,
    /// Whether SQL auto-completion is enabled.
    pub enabled_auto_complete: bool,
    /// Whether live as-you-type auto-completion is enabled.
    pub enabled_live_auto_complete: bool,
}

impl Default for QueryState {
    fn default() -> Self {
        Self {
            last_created_id: 0,
            current_query_id: None,
            query_ids: Vec::new(),
            queries_by_id: HashMap::new(),
            result_items_per_page: 100,
            enabled_auto_complete: true,
            enabled_live_auto_complete: true,
        }
    }
}

/// Type of SQL script action requested for a database object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScriptActionType {
    /// `CREATE` DDL script.
    #[serde(rename = "CREATE")]
    Create,
    /// `SELECT` DML template script.
    #[serde(rename = "SELECT")]
    Select,
    /// `INSERT` DML template script.
    #[serde(rename = "INSERT")]
    Insert,
    /// `UPDATE` DML template script.
    #[serde(rename = "UPDATE")]
    Update,
    /// `DELETE` DML template script.
    #[serde(rename = "DELETE")]
    Delete,
}

/// Kind of database object for which a script is generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DbObjectType {
    /// User table.
    Table,
    /// Database view.
    View,
    /// User-defined function.
    Function,
    /// Stored procedure.
    Procedure,
}

impl DbObjectType {
    /// Returns the string label (`"Table"`, `"View"`, `"Function"`, `"Procedure"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            DbObjectType::Table => "Table",
            DbObjectType::View => "View",
            DbObjectType::Function => "Function",
            DbObjectType::Procedure => "Procedure",
        }
    }
}

/// Cached scripts for a single database object across [`ScriptActionType`] variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectScripts {
    /// Kind of database object.
    pub object_type: DbObjectType,
    /// Cached `CREATE` script.
    pub create: Option<String>,
    /// Cached `SELECT` script.
    pub select: Option<String>,
    /// Cached `INSERT` script.
    pub insert: Option<String>,
    /// Cached `UPDATE` script.
    pub update: Option<String>,
    /// Cached `DELETE` script.
    pub delete: Option<String>,
}

impl ObjectScripts {
    /// Creates an empty [`ObjectScripts`] container for `object_type`.
    pub fn new(object_type: DbObjectType) -> Self {
        Self {
            object_type,
            create: None,
            select: None,
            insert: None,
            update: None,
            delete: None,
        }
    }

    /// Retrieves the cached script for `action`, if present.
    pub fn get(&self, action: ScriptActionType) -> Option<&str> {
        match action {
            ScriptActionType::Create => self.create.as_deref(),
            ScriptActionType::Select => self.select.as_deref(),
            ScriptActionType::Insert => self.insert.as_deref(),
            ScriptActionType::Update => self.update.as_deref(),
            ScriptActionType::Delete => self.delete.as_deref(),
        }
    }

    /// Stores `script` under `action`.
    pub fn set(&mut self, action: ScriptActionType, script: String) {
        match action {
            ScriptActionType::Create => self.create = Some(script),
            ScriptActionType::Select => self.select = Some(script),
            ScriptActionType::Insert => self.insert = Some(script),
            ScriptActionType::Update => self.update = Some(script),
            ScriptActionType::Delete => self.delete = Some(script),
        }
    }
}

/// State slice caching generated SQL scripts keyed by database and object name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScriptState {
    /// Error from the last script generation request.
    pub error: Option<String>,
    /// Whether a script is currently being generated.
    pub is_fetching: bool,
    /// Whether cached scripts are stale.
    pub did_invalidate: bool,
    /// Cached scripts grouped by database name and object name.
    pub scripts_by_object: HashMap<String, HashMap<String, ObjectScripts>>,
}
