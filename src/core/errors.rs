use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CANCELED_BY_USER_CODE: &str = "CANCELED_BY_USER";
pub const CANCELED_BY_USER_NAME: &str = "Query canceled by user";
pub const CANCELED_BY_USER_MESSAGE: &str = "Query canceled by user. The query process may still in the process list. But has already received the command to kill it successfully.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{message}")]
pub struct CanceledByUserError {
    pub name: String,
    pub message: String,
    pub code: String,
    #[serde(rename = "sqlectronError")]
    pub sqlectron_error: String,
}

impl Default for CanceledByUserError {
    fn default() -> Self {
        Self::new()
    }
}

impl CanceledByUserError {
    pub fn new() -> Self {
        Self {
            name: CANCELED_BY_USER_NAME.to_string(),
            message: CANCELED_BY_USER_MESSAGE.to_string(),
            code: CANCELED_BY_USER_CODE.to_string(),
            sqlectron_error: CANCELED_BY_USER_CODE.to_string(),
        }
    }
}

#[derive(Debug, Error)]
pub enum DbCoreError {
    #[error("{0}")]
    CanceledByUser(#[from] CanceledByUserError),
    #[error(
        "There is already a connection in progress for this database. Aborting this new request."
    )]
    ConnectionInProgress,
    #[error("There is no connection available.")]
    NoConnectionAvailable,
    #[error("Missing server configuration")]
    MissingServerConfig,
    #[error("Invalid SQL adapter: {0}")]
    InvalidAdapter(String),
    #[error("Unknown requested adapter: {0}")]
    UnknownAdapter(String),
    #[error("Adapter already registered with that key")]
    AdapterAlreadyRegistered,
    #[error("Query not ready to be canceled")]
    QueryNotReadyToCancel,
    #[error("\"query\" function is not implementd by cassandra client.")]
    CassandraCancelUnsupported,
    #[error("{0}")]
    Message(String),
}

impl DbCoreError {
    pub fn sqlectron_error_code(&self) -> Option<&str> {
        match self {
            DbCoreError::CanceledByUser(_) => Some(CANCELED_BY_USER_CODE),
            _ => None,
        }
    }
}
