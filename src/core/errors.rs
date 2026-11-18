//! Core database error types, user query cancellation errors, and error code constants.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error code emitted when an in-flight SQL query is canceled by the user.
pub const CANCELED_BY_USER_CODE: &str = "CANCELED_BY_USER";
/// Human-readable error title for user-canceled queries.
pub const CANCELED_BY_USER_NAME: &str = "Query canceled by user";
/// Detailed explanation returned when a running query is interrupted by the user.
pub const CANCELED_BY_USER_MESSAGE: &str = "Query canceled by user. The query process may still in the process list. But has already received the command to kill it successfully.";

/// Error returned when a cancellable query is aborted by the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{message}")]
pub struct CanceledByUserError {
    /// Short error title.
    pub name: String,
    /// Detailed cancellation explanation.
    pub message: String,
    /// Machine-readable error code (`CANCELED_BY_USER`).
    pub code: String,
    /// Serialized error code field.
    #[serde(rename = "errorCode")]
    pub error_code: String,
}

impl Default for CanceledByUserError {
    fn default() -> Self {
        Self::new()
    }
}

impl CanceledByUserError {
    /// Constructs a new [`CanceledByUserError`] with standard messages and codes.
    pub fn new() -> Self {
        Self {
            name: CANCELED_BY_USER_NAME.to_string(),
            message: CANCELED_BY_USER_MESSAGE.to_string(),
            code: CANCELED_BY_USER_CODE.to_string(),
            error_code: CANCELED_BY_USER_CODE.to_string(),
        }
    }
}

/// Unified error enumeration for database adapter and session operations.
#[derive(Debug, Error)]
pub enum DbCoreError {
    /// Query was canceled by the user.
    #[error("{0}")]
    CanceledByUser(#[from] CanceledByUserError),
    /// A connection attempt is already in progress for this database.
    #[error(
        "There is already a connection in progress for this database. Aborting this new request."
    )]
    ConnectionInProgress,
    /// Attempted to run a query or metadata command before connecting.
    #[error("There is no connection available.")]
    NoConnectionAvailable,
    /// Server configuration lacked a client or adapter key.
    #[error("Missing server configuration")]
    MissingServerConfig,
    /// Requested SQL adapter key is not recognized.
    #[error("Invalid SQL adapter: {0}")]
    InvalidAdapter(String),
    /// Requested adapter key was not found in the adapter factory registry.
    #[error("Unknown requested adapter: {0}")]
    UnknownAdapter(String),
    /// Attempted to register a custom adapter with a duplicate key.
    #[error("Adapter already registered with that key")]
    AdapterAlreadyRegistered,
    /// Attempted to cancel a query before its backend handle or PID was initialized.
    #[error("Query not ready to be canceled")]
    QueryNotReadyToCancel,
    /// Cassandra adapter does not support cancellable queries.
    #[error("\"query\" function is not implementd by cassandra client.")]
    CassandraCancelUnsupported,
    /// No live driver exists for this dialect and emulation was not explicitly enabled.
    #[error(
        "Live connections for {0} are not implemented. Rust-SQL can only emulate {0} with an \
         in-memory engine whose results are not from your server. Start Rust-SQL with --emulate, \
         or set \"emulateUnsupportedDialects\": true in the config file, to opt in to emulation."
    )]
    LiveConnectionNotImplemented(String),
    /// A live connection attempt failed. Never contains the password.
    #[error("Could not connect to {dialect} at {host}:{port} (database \"{database}\"): {reason}")]
    ConnectionFailed {
        /// Dialect display name (`PostgreSQL`, `Redshift`).
        dialect: String,
        /// Host that was dialed.
        host: String,
        /// Port that was dialed.
        port: u32,
        /// Database name that was requested.
        database: String,
        /// Driver error with any password occurrence redacted.
        reason: String,
    },
    /// The SSH tunnel could not be opened (host key, authentication, or bastion error).
    #[error("{0}")]
    Tunnel(#[from] crate::core::tunnel::TunnelError),
    /// A statement ran longer than the connection's statement timeout and was stopped.
    #[error("Statement timed out: {0}")]
    StatementTimeout(String),
    /// General driver or execution error message.
    #[error("{0}")]
    Message(String),
}

impl DbCoreError {
    /// Returns the machine-readable error code if this error is a [`CanceledByUserError`].
    pub fn error_code(&self) -> Option<&str> {
        match self {
            DbCoreError::CanceledByUser(_) => Some(CANCELED_BY_USER_CODE),
            _ => None,
        }
    }
}
