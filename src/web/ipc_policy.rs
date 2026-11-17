//! Which query IPC events need a connection grant and which need an administrator.

use crate::common::event::*;

/// Query events that use the open connection and therefore need a grant.
pub const QUERY_CONNECTION_EVENTS: &[&str] = &[
    DB_CREATE_CANCELLABLE_QUERY,
    DB_EXECUTE_CANCELLABLE_QUERY,
    DB_EXECUTE_QUERY,
    DB_GET_QUERY_SELECT_TOP,
    DB_GET_TABLE_CREATE_SCRIPT,
    DB_GET_TABLE_SELECT_SCRIPT,
    DB_GET_TABLE_INSERT_SCRIPT,
    DB_GET_TABLE_UPDATE_SCRIPT,
    DB_GET_TABLE_DELETE_SCRIPT,
    DB_GET_VIEW_CREATE_SCRIPT,
    DB_GET_ROUTINE_CREATE_SCRIPT,
    DB_TRUNCATE_ALL_TABLES,
    DB_GET_TABLE_COLUMN_NAMES,
];

/// Query events that touch the server's file system or process-wide limits.
pub const QUERY_ADMIN_EVENTS: &[&str] = &[
    DB_SET_SELECT_LIMIT,
    DB_EXPORT_QUERY_RESULT_TO_FILE,
    DB_QUERY_SAVE,
    DB_QUERY_OPEN,
];
