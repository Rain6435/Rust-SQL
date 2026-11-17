//! Append-only audit log (`audit_events`).
//!
//! Records logins, failed and rate-limited logins, logouts, connection and grant changes,
//! user administration, denied access, query executions, and SSH host-key approvals and
//! deletions. Query executions record the connection, user, time, statement type, and row
//! count, never the query text. No row ever contains a password, session ID, or CSRF token.

use super::{now_rfc3339, AppDb, AppDbError};
use rusqlite::params;
use serde::Serialize;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditKind {
    /// Successful login.
    Login,
    /// Failed login (unknown user or wrong password).
    LoginFailed,
    /// Login refused by the rate limiter.
    LoginRateLimited,
    /// Logout.
    Logout,
    /// Saved connection created.
    ConnectionCreated,
    /// Saved connection changed.
    ConnectionUpdated,
    /// Saved connection deleted.
    ConnectionDeleted,
    /// Grant added.
    GrantAdded,
    /// Grant removed.
    GrantRemoved,
    /// Account created.
    UserCreated,
    /// Account changed (admin flag or password).
    UserUpdated,
    /// Account deleted.
    UserDeleted,
    /// All sessions of an account revoked.
    SessionsRevoked,
    /// A request was refused for lack of a grant or the admin flag.
    AccessDenied,
    /// A statement was executed.
    QueryExecuted,
    /// An SSH host key was approved or re-confirmed.
    HostKeyApproved,
    /// An SSH host-key pin was deleted.
    HostKeyDeleted,
}

impl AuditKind {
    /// Stable snake_case label stored in `event_type`.
    pub fn as_str(self) -> &'static str {
        match self {
            AuditKind::Login => "login",
            AuditKind::LoginFailed => "login_failed",
            AuditKind::LoginRateLimited => "login_rate_limited",
            AuditKind::Logout => "logout",
            AuditKind::ConnectionCreated => "connection_created",
            AuditKind::ConnectionUpdated => "connection_updated",
            AuditKind::ConnectionDeleted => "connection_deleted",
            AuditKind::GrantAdded => "grant_added",
            AuditKind::GrantRemoved => "grant_removed",
            AuditKind::UserCreated => "user_created",
            AuditKind::UserUpdated => "user_updated",
            AuditKind::UserDeleted => "user_deleted",
            AuditKind::SessionsRevoked => "sessions_revoked",
            AuditKind::AccessDenied => "access_denied",
            AuditKind::QueryExecuted => "query_executed",
            AuditKind::HostKeyApproved => "host_key_approved",
            AuditKind::HostKeyDeleted => "host_key_deleted",
        }
    }
}

/// Who did it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditActor {
    /// Account ID, when known.
    pub user_id: Option<i64>,
    /// Account name, or `local` for the desktop GUI, TUI, and CLI.
    pub username: Option<String>,
    /// Client IP address for web requests.
    pub source_ip: Option<String>,
}

impl AuditActor {
    /// The in-process front ends (GUI, TUI, CLI), which have no account.
    pub fn local() -> Self {
        Self {
            username: Some("local".into()),
            ..Default::default()
        }
    }
}

/// One event to append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    /// Event type.
    pub kind: AuditKind,
    /// `success`, `failure`, or `denied`.
    pub outcome: &'static str,
    /// Actor.
    pub actor: AuditActor,
    /// Saved connection ID involved, if any.
    pub connection_id: Option<String>,
    /// Statement type for query executions (for example `SELECT`).
    pub statement_type: Option<String>,
    /// Rows returned or affected for query executions.
    pub row_count: Option<i64>,
    /// Short non-secret detail (target user, host:port and fingerprint, error class).
    pub detail: Option<String>,
}

impl AuditEvent {
    /// A successful event of `kind` by `actor`.
    pub fn new(kind: AuditKind, actor: &AuditActor) -> Self {
        Self {
            kind,
            outcome: "success",
            actor: actor.clone(),
            connection_id: None,
            statement_type: None,
            row_count: None,
            detail: None,
        }
    }

    /// Sets the outcome label.
    pub fn outcome(mut self, outcome: &'static str) -> Self {
        self.outcome = outcome;
        self
    }

    /// Sets the connection ID.
    pub fn connection(mut self, id: impl Into<String>) -> Self {
        self.connection_id = Some(id.into());
        self
    }

    /// Sets the detail text.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sets statement type and row count.
    pub fn statement(mut self, statement_type: Option<String>, rows: Option<i64>) -> Self {
        self.statement_type = statement_type;
        self.row_count = rows;
        self
    }
}

/// A stored audit row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    /// Row ID.
    pub id: i64,
    /// When (RFC 3339, UTC).
    pub occurred_at: String,
    /// Event type label.
    pub event_type: String,
    /// Outcome label.
    pub outcome: String,
    /// Account ID.
    pub user_id: Option<i64>,
    /// Account name.
    pub username: Option<String>,
    /// Client IP.
    pub source_ip: Option<String>,
    /// Connection ID.
    pub connection_id: Option<String>,
    /// Statement type.
    pub statement_type: Option<String>,
    /// Row count.
    pub row_count: Option<i64>,
    /// Detail.
    pub detail: Option<String>,
}

impl AppDb {
    /// Appends `event` to the audit log.
    pub fn record_audit(&self, event: &AuditEvent) -> Result<(), AppDbError> {
        self.conn()?.execute(
            "INSERT INTO audit_events (occurred_at, event_type, outcome, user_id, username,
                 source_ip, connection_id, statement_type, row_count, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                now_rfc3339(),
                event.kind.as_str(),
                event.outcome,
                event.actor.user_id,
                event.actor.username,
                event.actor.source_ip,
                event.connection_id,
                event.statement_type,
                event.row_count,
                event.detail,
            ],
        )?;
        Ok(())
    }

    /// Returns the newest `limit` audit rows, newest first.
    pub fn list_audit(&self, limit: u32) -> Result<Vec<AuditRecord>, AppDbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, occurred_at, event_type, outcome, user_id, username, source_ip,
                    connection_id, statement_type, row_count, detail
             FROM audit_events ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok(AuditRecord {
                id: r.get(0)?,
                occurred_at: r.get(1)?,
                event_type: r.get(2)?,
                outcome: r.get(3)?,
                user_id: r.get(4)?,
                username: r.get(5)?,
                source_ip: r.get(6)?,
                connection_id: r.get(7)?,
                statement_type: r.get(8)?,
                row_count: r.get(9)?,
                detail: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

/// Appends `event` to the default application database (used by the in-process front ends).
pub fn record_to_default_db(event: &AuditEvent) -> Result<(), AppDbError> {
    AppDb::open_default()?.record_audit(event)
}
