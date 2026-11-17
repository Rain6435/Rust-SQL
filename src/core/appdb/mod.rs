//! The application's own SQLite database: web accounts, sessions, connection grants, and the
//! audit log.
//!
//! It lives next to the configuration file (`rust-sql.json` becomes `rust-sql.db`), so test
//! suites that override the configuration path are isolated automatically. It never stores
//! database passwords, session IDs, or query text.

pub mod audit;
pub mod grants;
pub mod sessions;
pub mod users;

pub use audit::{AuditActor, AuditEvent, AuditKind, AuditRecord};
pub use grants::Grant;
pub use sessions::{NewSession, SessionInfo};
pub use users::{User, MIN_PASSWORD_LEN};

use crate::core::config::get_config_path;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Errors from the application database.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AppDbError {
    /// SQLite failed (I/O, schema, or constraint).
    #[error("Application database error: {0}")]
    Sqlite(String),
    /// The input was rejected (for example a password shorter than 12 characters).
    #[error("{0}")]
    Invalid(String),
    /// The requested record does not exist.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// Password hashing failed.
    #[error("Password hashing failed: {0}")]
    Hash(String),
}

impl From<rusqlite::Error> for AppDbError {
    fn from(e: rusqlite::Error) -> Self {
        AppDbError::Sqlite(e.to_string())
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    csrf_token TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    revoked_at TEXT
);
CREATE TABLE IF NOT EXISTS connection_grants (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    connection_id TEXT NOT NULL,
    granted_by INTEGER,
    granted_at TEXT NOT NULL,
    PRIMARY KEY (user_id, connection_id)
);
CREATE TABLE IF NOT EXISTS audit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at TEXT NOT NULL,
    event_type TEXT NOT NULL,
    outcome TEXT NOT NULL,
    user_id INTEGER,
    username TEXT,
    source_ip TEXT,
    connection_id TEXT,
    statement_type TEXT,
    row_count INTEGER,
    detail TEXT
);
";

/// Handle to the application database. One connection, serialized by a mutex.
pub struct AppDb {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl AppDb {
    /// Opens (creating if needed) the database at `path` and applies the schema.
    pub fn open(path: &Path) -> Result<Self, AppDbError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| AppDbError::Sqlite(e.to_string()))?;
            }
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        })
    }

    /// Opens the database at [`default_path`].
    pub fn open_default() -> Result<Self, AppDbError> {
        Self::open(&default_path())
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn conn(&self) -> Result<MutexGuard<'_, Connection>, AppDbError> {
        self.conn
            .lock()
            .map_err(|_| AppDbError::Sqlite("database lock poisoned".into()))
    }
}

/// Default database path: the configuration file path with a `.db` extension.
pub fn default_path() -> PathBuf {
    get_config_path().with_extension("db")
}

/// Current UTC time as RFC 3339.
pub(crate) fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| OffsetDateTime::now_utc().unix_timestamp().to_string())
}

/// Current UTC time as Unix seconds.
pub(crate) fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

#[cfg(test)]
pub(crate) fn test_db() -> (tempfile::TempDir, AppDb) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let db = AppDb::open(&dir.path().join("app.db")).expect("open app db");
    (dir, db)
}
