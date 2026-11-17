//! Shared web server state ([`WebApp`]), per-session state ([`WebIpcState`]), and the
//! per-request [`IpcCtx`] with authorization helpers.
//!
//! Each signed-in session has its own [`DatabaseFacade`], so one user's open connection is
//! never visible to another user. Every IPC event that touches a connection re-checks the
//! grant on each request, so revoking a grant takes effect immediately.

use crate::adapters::DatabaseFacade;
use crate::common::types::HostKeyChallenge;
use crate::core::appdb::{AppDb, AuditActor, AuditEvent, AuditKind, User};
use crate::core::keys::KeyProvider;
use crate::web::rate_limit::LoginLimiter;
use crate::web::settings::WebSettings;
use crate::web::IpcResponse;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{Mutex, Semaphore};

/// State of one signed-in session.
pub struct WebIpcState {
    /// This session's database facade.
    pub db: Mutex<DatabaseFacade>,
    /// Host-key challenges issued to this session by `DB_CONNECT`; only these can be approved.
    pub pending_host_keys: Mutex<Vec<HostKeyChallenge>>,
    /// Saved connection ID this session is connected to.
    pub connected_server: StdMutex<Option<String>>,
    /// Concurrency cap for this session's IPC requests.
    pub permits: Arc<Semaphore>,
}

impl WebIpcState {
    /// Creates session state allowing `max_concurrent` simultaneous IPC requests.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            db: Mutex::new(DatabaseFacade::default()),
            pending_host_keys: Mutex::new(Vec::new()),
            connected_server: StdMutex::new(None),
            permits: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    /// The saved connection ID this session is connected to.
    pub fn connected_server(&self) -> Option<String> {
        self.connected_server
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Records (or clears) the connected saved connection ID.
    pub fn set_connected_server(&self, id: Option<String>) {
        *self
            .connected_server
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = id;
    }
}

/// Process-wide web server state.
pub struct WebApp {
    /// Accounts, sessions, grants, and audit log.
    pub db: Arc<AppDb>,
    /// Master-key source for saved credentials. Keys never come from clients.
    pub keys: Arc<dyn KeyProvider>,
    /// Validated settings.
    pub settings: WebSettings,
    /// Failed-login limiter.
    pub limiter: LoginLimiter,
    sessions: StdMutex<HashMap<String, (i64, Arc<WebIpcState>)>>,
}

impl WebApp {
    /// Creates the shared state.
    pub fn new(db: AppDb, keys: Arc<dyn KeyProvider>, settings: WebSettings) -> Self {
        Self {
            db: Arc::new(db),
            keys,
            settings,
            limiter: LoginLimiter::default(),
            sessions: StdMutex::new(HashMap::new()),
        }
    }

    /// Returns (creating if needed) the state for `user_id`'s session with `token_hash`.
    pub fn session_state(&self, token_hash: &str, user_id: i64) -> Arc<WebIpcState> {
        let mut map = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(token_hash.to_string())
            .or_insert_with(|| {
                let state = WebIpcState::new(self.settings.max_concurrent_per_session);
                (user_id, Arc::new(state))
            })
            .1
            .clone()
    }

    /// Drops the in-memory state of every session of `user_id`, closing their connections.
    pub async fn drop_user_session_states(&self, user_id: i64) {
        let hashes: Vec<String> = self
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, (uid, _))| *uid == user_id)
            .map(|(h, _)| h.clone())
            .collect();
        for h in hashes {
            self.drop_session_state(&h).await;
        }
    }

    /// Drops the state of a session that ended, closing its connections.
    pub async fn drop_session_state(&self, token_hash: &str) {
        let removed = self
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(token_hash);
        if let Some((_, state)) = removed {
            let _ = state.db.lock().await.disconnect_server().await;
        }
    }

    /// Appends an audit event; a failure is logged because the request already completed.
    pub fn audit(&self, event: AuditEvent) {
        if let Err(e) = self.db.record_audit(&event) {
            log::error!("audit log write failed for {}: {e}", event.kind.as_str());
        }
    }
}

/// Everything an IPC handler needs for one request.
pub struct IpcCtx<'a> {
    /// Shared state.
    pub app: &'a WebApp,
    /// The caller's session state.
    pub session: &'a WebIpcState,
    /// The signed-in user.
    pub user: &'a User,
    /// Client IP address.
    pub source_ip: Option<String>,
}

impl IpcCtx<'_> {
    /// The caller as an audit actor.
    pub fn actor(&self) -> AuditActor {
        AuditActor {
            user_id: Some(self.user.id),
            username: Some(self.user.username.clone()),
            source_ip: self.source_ip.clone(),
        }
    }

    /// Audits a refused request and returns the matching 403 response.
    pub fn deny(&self, what: &str, connection_id: Option<&str>) -> IpcResponse {
        let mut ev = AuditEvent::new(AuditKind::AccessDenied, &self.actor())
            .outcome("denied")
            .detail(what.to_string());
        if let Some(id) = connection_id {
            ev = ev.connection(id);
        }
        self.app.audit(ev);
        IpcResponse::forbidden(format!("Access denied: {what}"))
    }

    /// Requires the admin flag for `event`.
    pub fn require_admin(&self, event: &str) -> Result<(), IpcResponse> {
        if self.user.is_admin {
            Ok(())
        } else {
            Err(self.deny(&format!("{event} requires an administrator"), None))
        }
    }

    /// Whether the caller may use the saved connection `id` (admins may use all).
    pub fn may_use_connection(&self, id: &str) -> Result<bool, IpcResponse> {
        if self.user.is_admin {
            return Ok(true);
        }
        self.app
            .db
            .has_grant(self.user.id, id)
            .map_err(|e| IpcResponse::err(e.to_string()))
    }

    /// Requires a grant for `id`, auditing and returning 403 otherwise.
    pub fn require_grant(&self, id: &str) -> Result<(), IpcResponse> {
        if self.may_use_connection(id)? {
            Ok(())
        } else {
            Err(self.deny("no grant for this connection", Some(id)))
        }
    }

    /// Requires an open connection that the caller still holds a grant for; returns its ID.
    pub fn require_connected(&self) -> Result<String, IpcResponse> {
        let Some(id) = self.session.connected_server() else {
            return Err(IpcResponse::err(
                "Not connected: connect to a saved connection first",
            ));
        };
        self.require_grant(&id)?;
        Ok(id)
    }
}

/// Test helper: an app on a temporary database with one non-admin user.
#[cfg(test)]
pub(crate) fn test_app() -> (tempfile::TempDir, WebApp, User) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let db = AppDb::open(&dir.path().join("app.db")).expect("app db");
    let user = db
        .create_user("tester", "correct horse battery", false)
        .expect("user");
    let keys = Arc::new(crate::core::keys::InMemoryKeyProvider::new_for_tests());
    let app = WebApp::new(db, keys, WebSettings::loopback_for_tests(8080));
    (dir, app, user)
}
