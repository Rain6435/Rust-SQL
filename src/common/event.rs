//! Application IPC and menu event channel constants.
//!
//! These constants define the canonical event names shared between the state store,
//! the native desktop menu bar, and the Web IPC server.

/// Prepare and sanitize the configuration file on disk.
pub const CONFIG_PREPARE: &str = "CONFIG_PREPARE";
/// Retrieve the resolved absolute path of `rust-sql.json`.
pub const CONFIG_PATH: &str = "CONFIG_PATH";
/// Read the raw configuration file from disk.
pub const CONFIG_GET: &str = "CONFIG_GET";
/// Read the full merged runtime configuration asynchronously.
pub const CONFIG_GET_FULL: &str = "CONFIG_GET_FULL";
/// Read the full merged runtime configuration synchronously.
pub const CONFIG_GET_FULL_SYNC: &str = "CONFIG_GET_FULL_SYNC";
/// Overwrite the entire configuration file on disk.
pub const CONFIG_SAVE: &str = "CONFIG_SAVE";
/// Merge and persist user settings (`BaseConfig`) into the configuration file.
pub const CONFIG_SAVE_SETTINS: &str = "CONFIG_SAVE_SETTINS";

/// List all configured database servers.
pub const SERVERS_GET_ALL: &str = "SERVERS_GET_ALL";
/// Validate, encrypt secrets, and add a new database server.
pub const SERVERS_ADD: &str = "SERVERS_ADD";
/// Validate, encrypt secrets, and update an existing database server.
pub const SERVERS_UPDATE: &str = "SERVERS_UPDATE";
/// Add a new server or update an existing server if `id` is populated.
pub const SERVERS_ADD_OR_UPDATE: &str = "SERVERS_ADD_OR_UPDATE";
/// Delete a configured server by its UUID.
pub const SERVERS_REMOVE_BY_ID: &str = "SERVERS_REMOVE_BY_ID";
/// Decrypt the database and SSH passwords of a `Server` using the runtime secret.
pub const SERVERS_DECRYPT_SECRETS: &str = "SERVERS_DECRYPT_SECRETS";

/// Log an error emitted by the UI renderer.
pub const LOGGER_ERROR: &str = "LOGGER_ERROR";

/// Retrieve metadata for all supported database client adapters.
pub const DB_GET_CLIENTS_SYNC: &str = "DB_GET_CLIENTS_SYNC";
/// Establish a connection to a server and target database.
pub const DB_CONNECT: &str = "DB_CONNECT";
/// Returns `{ "mode": "live" | "emulated", "banner": string | null }` for a connected database.
pub const DB_GET_CONNECTION_MODE: &str = "DB_GET_CONNECTION_MODE";
/// Disconnect a single database connection within the active server session.
pub const DB_DISCONNECT_DATABASE: &str = "DB_DISCONNECT_DATABASE";
/// Disconnect the entire active server session and any SSH tunnels.
pub const DB_DISCONNECT_SERVER: &str = "DB_DISCONNECT_SERVER";
/// List all databases available on the connected server.
pub const DB_LIST_DATABASES: &str = "DB_LIST_DATABASES";
/// List all schemas within a database.
pub const DB_LIST_SCHEMAS: &str = "DB_LIST_SCHEMAS";
/// List all tables within a database.
pub const DB_LIST_TABLES: &str = "DB_LIST_TABLES";
/// List all views within a database.
pub const DB_LIST_VIEWS: &str = "DB_LIST_VIEWS";
/// List all stored functions and procedures within a database.
pub const DB_LIST_ROUTINES: &str = "DB_LIST_ROUTINES";
/// List all columns and their data types for a specific table.
pub const DB_LIST_TABLE_COLUMNS: &str = "DB_LIST_TABLE_COLUMNS";
/// List all triggers attached to a specific table.
pub const DB_LIST_TABLE_TRIGGERS: &str = "DB_LIST_TABLE_TRIGGERS";
/// List all indexes defined on a specific table.
pub const DB_LIST_TABLE_INDEXES: &str = "DB_LIST_TABLE_INDEXES";
/// List all foreign-key referenced table names for a specific table.
pub const DB_GET_TABLE_REFERENCES: &str = "DB_GET_TABLE_REFERENCES";
/// List primary and foreign key constraints for a specific table.
pub const DB_GET_TABLE_KEYS: &str = "DB_GET_TABLE_KEYS";
/// Register a cancellable query handle identified by `queryId`.
pub const DB_CREATE_CANCELLABLE_QUERY: &str = "DB_CREATE_CANCELLABLE_QUERY";
/// Cancel an in-flight cancellable query by `queryId`.
pub const DB_CANCEL_CANCELLABLE_QUERY: &str = "DB_CANCEL_CANCELLABLE_QUERY";
/// Execute a previously registered cancellable query by `queryId`.
pub const DB_EXECUTE_CANCELLABLE_QUERY: &str = "DB_EXECUTE_CANCELLABLE_QUERY";
/// Execute one or more SQL statements directly against a database.
pub const DB_EXECUTE_QUERY: &str = "DB_EXECUTE_QUERY";
/// Build the dialect-specific `SELECT TOP / LIMIT` query string for a table.
pub const DB_GET_QUERY_SELECT_TOP: &str = "DB_GET_QUERY_SELECT_TOP";
/// Generate the `CREATE TABLE` DDL script for a table.
pub const DB_GET_TABLE_CREATE_SCRIPT: &str = "DB_GET_TABLE_CREATE_SCRIPT";
/// Generate a `SELECT` template script listing all columns of a table.
pub const DB_GET_TABLE_SELECT_SCRIPT: &str = "DB_GET_TABLE_SELECT_SCRIPT";
/// Generate an `INSERT INTO` template script for a table.
pub const DB_GET_TABLE_INSERT_SCRIPT: &str = "DB_GET_TABLE_INSERT_SCRIPT";
/// Generate an `UPDATE` template script for a table.
pub const DB_GET_TABLE_UPDATE_SCRIPT: &str = "DB_GET_TABLE_UPDATE_SCRIPT";
/// Generate a `DELETE FROM` template script for a table.
pub const DB_GET_TABLE_DELETE_SCRIPT: &str = "DB_GET_TABLE_DELETE_SCRIPT";
/// Generate the `CREATE VIEW` DDL script for a view.
pub const DB_GET_VIEW_CREATE_SCRIPT: &str = "DB_GET_VIEW_CREATE_SCRIPT";
/// Generate the `CREATE FUNCTION / PROCEDURE` DDL script for a routine.
pub const DB_GET_ROUTINE_CREATE_SCRIPT: &str = "DB_GET_ROUTINE_CREATE_SCRIPT";
/// Truncate or delete all rows across all tables in a database/schema.
pub const DB_TRUNCATE_ALL_TABLES: &str = "DB_TRUNCATE_ALL_TABLES";
/// Retrieve just the column names for a specific table.
pub const DB_GET_TABLE_COLUMN_NAMES: &str = "DB_GET_TABLE_COLUMN_NAMES";
/// Override the default row limit used by `Select Top N` queries.
pub const DB_SET_SELECT_LIMIT: &str = "DB_SET_SELECT_LIMIT";
/// Export query result rows to a file on disk.
pub const DB_EXPORT_QUERY_RESULT_TO_FILE: &str = "DB_EXPORT_QUERY_RESULT_TO_FILE";
/// Export query result rows to the system clipboard.
pub const DB_EXPORT_QUERY_RESULT_TO_CLIPBOARD: &str = "DB_EXPORT_QUERY_RESULT_TO_CLIPBOARD";
/// Save a SQL query tab's contents to a `.sql` file.
pub const DB_QUERY_SAVE: &str = "DB_QUERY_SAVE";
/// Load a `.sql` file from disk into the active query tab.
pub const DB_QUERY_OPEN: &str = "DB_QUERY_OPEN";

/// Show a native file save dialog.
pub const BROWSER_DIALOG_SHOW_SAVE_DIALOG: &str = "BROWSER_DIALOG_SHOW_SAVE_DIALOG";
/// Show a native file open dialog.
pub const BROWSER_DIALOG_SHOW_OPEN_DIALOG: &str = "BROWSER_DIALOG_SHOW_OPEN_DIALOG";
/// Write raw data to a file path.
pub const BROWSER_FS_SAVE_FILE: &str = "BROWSER_FS_SAVE_FILE";
/// Read raw text from a file path.
pub const BROWSER_FS_OPEN_FILE: &str = "BROWSER_FS_OPEN_FILE";

/// Build a context menu instance.
pub const BROWSER_MENU_CTX_MENU_BUILD: &str = "BROWSER_MENU_CTX_MENU_BUILD";
/// Display a previously built context menu at `(x, y)`.
pub const BROWSER_MENU_CTX_MENU_POPUP: &str = "BROWSER_MENU_CTX_MENU_POPUP";
/// Menu event: Refresh metadata for the selected database.
pub const BROWSER_MENU_REFRESH_DATABASE: &str = "BROWSER_MENU_REFRESH_DATABASE";
/// Menu event: Open a new query tab for the selected database.
pub const BROWSER_MENU_OPEN_TAB: &str = "BROWSER_MENU_OPEN_TAB";
/// Menu event: Execute the default `SELECT TOP N` query for a table or view.
pub const BROWSER_MENU_EXECUTE_DEFAULT_QUERY: &str = "BROWSER_MENU_EXECUTE_DEFAULT_QUERY";
/// Menu event: Generate `CREATE` statement for a database object.
pub const BROWSER_MENU_GET_SQL_SCRIPT_CREATE_TABLE: &str =
    "BROWSER_MENU_GET_SQL_SCRIPT_CREATE_TABLE";
/// Menu event: Generate `SELECT` statement for a table.
pub const BROWSER_MENU_GET_SQL_SCRIPT_SELECT: &str = "BROWSER_MENU_GET_SQL_SCRIPT_SELECT";
/// Menu event: Generate `INSERT` statement for a table.
pub const BROWSER_MENU_GET_SQL_SCRIPT_INSERT: &str = "BROWSER_MENU_GET_SQL_SCRIPT_INSERT";
/// Menu event: Generate `UPDATE` statement for a table.
pub const BROWSER_MENU_GET_SQL_SCRIPT_UPDATE: &str = "BROWSER_MENU_GET_SQL_SCRIPT_UPDATE";
/// Menu event: Generate `DELETE` statement for a table.
pub const BROWSER_MENU_GET_SQL_SCRIPT_DELETE: &str = "BROWSER_MENU_GET_SQL_SCRIPT_DELETE";
/// Menu event: Open the cell content preview modal.
pub const BROWSER_MENU_OPEN_PREVIEW: &str = "BROWSER_MENU_OPEN_PREVIEW";

/// Menu event: Increase UI zoom factor (+20%).
pub const BROWSER_MENU_ZOOM_IN: &str = "BROWSER_MENU_ZOOM_IN";
/// Menu event: Decrease UI zoom factor (-20%).
pub const BROWSER_MENU_ZOOM_OUT: &str = "BROWSER_MENU_ZOOM_OUT";
/// Menu event: Reset UI zoom factor to 100%.
pub const BROWSER_MENU_ZOOM_RESET: &str = "BROWSER_MENU_ZOOM_RESET";

/// Menu event: Execute the active SQL query.
pub const BROWSER_MENU_QUERY_EXECUTE: &str = "BROWSER_MENU_QUERY_EXECUTE";
/// Menu event: Open a new SQL query tab.
pub const BROWSER_MENU_QUERY_TAB_NEW: &str = "BROWSER_MENU_QUERY_TAB_NEW";
/// Menu event: Close the active SQL query tab.
pub const BROWSER_MENU_QUERY_TAB_CLOSE: &str = "BROWSER_MENU_QUERY_TAB_CLOSE";
/// Menu event: Save the active SQL query.
pub const BROWSER_MENU_QUERY_SAVE: &str = "BROWSER_MENU_QUERY_SAVE";
/// Menu event: Save the active SQL query under a new filename.
pub const BROWSER_MENU_QUERY_SAVE_AS: &str = "BROWSER_MENU_QUERY_SAVE_AS";
/// Menu event: Open a SQL query file from disk.
pub const BROWSER_MENU_QUERY_OPEN: &str = "BROWSER_MENU_QUERY_OPEN";
/// Menu event: Focus the SQL query editor.
pub const BROWSER_MENU_QUERY_FOCUS: &str = "BROWSER_MENU_QUERY_FOCUS";
/// Menu event: Focus the sidebar database search input.
pub const BROWSER_MENU_TOGGLE_DB_SEARCH: &str = "BROWSER_MENU_TOGGLE_DB_SEARCH";
/// Menu event: Focus the sidebar database objects search input.
pub const BROWSER_MENU_TOGGLE_DB_OBJS_SEARCH: &str = "BROWSER_MENU_TOGGLE_DB_OBJS_SEARCH";
/// Menu event: Format/beautify the active SQL query.
pub const BROWSER_MENU_EDITOR_FORMAT: &str = "BROWSER_MENU_EDITOR_FORMAT";

/// Open an external URL in the user's default browser.
pub const BROWSER_SHELL_OPEN: &str = "BROWSER_SHELL_OPEN";
/// Write text to the system clipboard.
pub const BROWSER_CLIPBOARD_WRITE: &str = "BROWSER_CLIPBOARD_WRITE";
/// Set the UI zoom factor.
pub const BROWSER_WEB_FRAME_SET_ZOOM_FACTOR: &str = "BROWSER_WEB_FRAME_SET_ZOOM_FACTOR";

/// Trigger an application update check.
pub const UPDATE_CHECK: &str = "UPDATE_CHECK";
/// Notification emitted when a newer application version is available.
pub const UPDATE_AVAILABLE: &str = "UPDATE_AVAILABLE";
