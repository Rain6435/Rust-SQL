//! Database adapter metadata, query execution results, schema objects, and file/menu dialog types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Static capabilities and defaults for a registered database client adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterInfo {
    /// Unique client identifier (`postgresql`, `redshift`, `mysql`, `mariadb`, `sqlserver`, `sqlite`, `cassandra`).
    pub key: String,
    /// Human-readable display name (`PostgreSQL`, `MySQL`, etc.).
    pub name: String,
    /// URI scheme protocol (`postgres`, `mysql`, `mssql`, `file`, `cassandra`, etc.).
    pub protocol: String,
    /// Default TCP port when none is specified by the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_port: Option<u32>,
    /// Default initial database name or `:memory:` path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_database: Option<String>,
    /// List of unsupported feature flags for this adapter (e.g., `"server:ssh"`, `"cancelQuery"`).
    pub disabled_features: Vec<String>,
}

/// Version metadata reported by a connected database server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterVersion {
    /// Product name (e.g., `"PostgreSQL"`, `"MySQL"`, `"SQLite"`).
    pub name: String,
    /// Numeric version string (e.g., `"14.5"`, `"8.0.32"`).
    pub version: String,
    /// Full raw version banner returned by the server.
    pub string: String,
}

impl Default for AdapterVersion {
    fn default() -> Self {
        Self {
            name: "UNKNOWN".to_string(),
            version: "0.0.0".to_string(),
            string: "UNKNOWN 0.0.0".to_string(),
        }
    }
}

/// Column header descriptor for a query result set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInfo {
    /// Column name.
    pub name: String,
}

/// Result of executing a single SQL statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRowResult {
    /// Statement command classification (`SELECT`, `INSERT`, `UPDATE`, `DELETE`, `EXPLAIN`, etc.).
    pub command: String,
    /// Returned data rows as ordered column-to-value maps.
    pub rows: Vec<serde_json::Map<String, Value>>,
    /// Column descriptors in SQL projection order.
    pub fields: Vec<FieldInfo>,
    /// Total number of rows returned for `SELECT` queries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_count: Option<usize>,
    /// Number of rows modified by `INSERT`, `UPDATE`, or `DELETE` statements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected_rows: Option<usize>,
}

/// Metadata entry for a database table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTableResult {
    /// Optional schema owning the table (e.g., `"public"`, `"dbo"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Table name.
    pub name: String,
}

/// Alias for a table metadata entry.
pub type DbTable = ListTableResult;
/// Alias for a view metadata entry.
pub type ListViewResult = ListTableResult;
/// Alias for a view metadata entry.
pub type DbView = ListViewResult;

/// Metadata entry for a stored procedure or function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRoutineResult {
    /// Optional schema owning the routine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Routine identifier.
    pub routine_name: String,
    /// Routine classification (`"FUNCTION"` or `"PROCEDURE"`).
    pub routine_type: String,
}

/// Metadata entry describing a single table column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTableColumnsResult {
    /// Column name.
    pub column_name: String,
    /// SQL data type name (e.g., `"integer"`, `"varchar"`, `"text"`).
    pub data_type: String,
}

/// Primary or foreign key constraint metadata for a table column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableKeysResult {
    /// Constrained column name.
    pub column_name: String,
    /// Key classification (`"PRIMARY KEY"` or `"FOREIGN KEY"`).
    pub key_type: String,
    /// Optional constraint identifier.
    pub constraint_name: Option<String>,
    /// Referenced parent table name for foreign keys.
    pub referenced_table: Option<String>,
}

/// File extension filter used by open/save dialogs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogFilter {
    /// Display name for the filter (e.g., `"SQL"`, `"CSV"`).
    pub name: String,
    /// Allowed file extensions without leading dots.
    pub extensions: Vec<String>,
}

/// Context menu item descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuItem {
    /// Optional item type (e.g., `"separator"`).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub item_type: Option<String>,
    /// Human-readable menu label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Associated event constant emitted when clicked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
}

/// Collection of items forming a popup context menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuOptions {
    /// Ordered list of menu items.
    pub menu_items: Vec<MenuItem>,
}

/// Metadata returned after saving a `.sql` query file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedQueryInfo {
    /// Tab title derived from the file stem.
    pub name: String,
    /// Full file path on disk.
    pub filename: String,
}

/// Metadata and contents returned after opening a `.sql` query file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedQueryInfo {
    /// Tab title derived from the file stem.
    pub name: String,
    /// Raw SQL query text read from disk.
    pub query: String,
    /// Full file path on disk.
    pub filename: String,
}
