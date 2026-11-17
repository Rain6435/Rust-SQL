//! Login sessions: random 256-bit IDs of which only the SHA-256 hash is stored.
//!
//! Each session has an expiry, a revocation timestamp, and its own CSRF token. A database leak
//! therefore exposes no usable session ID.

use super::{now_rfc3339, now_unix, users::User, AppDb, AppDbError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::Rng;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::time::Duration;

/// A session that was just created. `token` goes into the cookie and is not stored.
#[derive(Debug, Clone)]
pub struct NewSession {
    /// The session ID for the cookie (base64url of 32 random bytes).
    pub token: String,
    /// Per-session CSRF token the client must echo in `X-CSRF-Token`.
    pub csrf_token: String,
    /// Expiry as Unix seconds.
    pub expires_at: i64,
}

/// A valid (unexpired, unrevoked) session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    /// SHA-256 hex of the session ID: the server-side key.
    pub token_hash: String,
    /// The signed-in user.
    pub user: User,
    /// Per-session CSRF token.
    pub csrf_token: String,
    /// Expiry as Unix seconds.
    pub expires_at: i64,
}

/// Generates a random 256-bit value with the CSPRNG, base64url-encoded.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// SHA-256 hex digest of a session ID.
pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl AppDb {
    /// Creates a session for `user_id` lasting `ttl`.
    pub fn create_session(&self, user_id: i64, ttl: Duration) -> Result<NewSession, AppDbError> {
        let token = random_token();
        let csrf_token = random_token();
        let ttl_secs = i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX / 2);
        let expires_at = now_unix().saturating_add(ttl_secs);
        self.conn()?.execute(
            "INSERT INTO sessions (token_hash, user_id, csrf_token, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                hash_token(&token),
                user_id,
                csrf_token,
                now_rfc3339(),
                expires_at
            ],
        )?;
        Ok(NewSession {
            token,
            csrf_token,
            expires_at,
        })
    }

    /// Returns the session for `token` if it exists, is unexpired, and is not revoked.
    pub fn lookup_session(&self, token: &str) -> Result<Option<SessionInfo>, AppDbError> {
        let token_hash = hash_token(token);
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT s.token_hash, s.csrf_token, s.expires_at,
                        u.id, u.username, u.is_admin, u.created_at
                 FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1 AND s.revoked_at IS NULL AND s.expires_at > ?2",
                params![token_hash, now_unix()],
                |r| {
                    Ok(SessionInfo {
                        token_hash: r.get(0)?,
                        csrf_token: r.get(1)?,
                        expires_at: r.get(2)?,
                        user: User {
                            id: r.get(3)?,
                            username: r.get(4)?,
                            is_admin: r.get::<_, i64>(5)? != 0,
                            created_at: r.get(6)?,
                        },
                    })
                },
            )
            .optional()?)
    }

    /// Revokes one session by its hash. Returns whether a live session was revoked.
    pub fn revoke_session(&self, token_hash: &str) -> Result<bool, AppDbError> {
        let n = self.conn()?.execute(
            "UPDATE sessions SET revoked_at = ?1 WHERE token_hash = ?2 AND revoked_at IS NULL",
            params![now_rfc3339(), token_hash],
        )?;
        Ok(n > 0)
    }

    /// Revokes every session of `user_id`. Returns how many were revoked.
    pub fn revoke_user_sessions(&self, user_id: i64) -> Result<usize, AppDbError> {
        Ok(self.conn()?.execute(
            "UPDATE sessions SET revoked_at = ?1 WHERE user_id = ?2 AND revoked_at IS NULL",
            params![now_rfc3339(), user_id],
        )?)
    }

    /// Deletes expired and revoked sessions. Returns how many rows were removed.
    pub fn purge_sessions(&self) -> Result<usize, AppDbError> {
        Ok(self.conn()?.execute(
            "DELETE FROM sessions WHERE revoked_at IS NOT NULL OR expires_at <= ?1",
            [now_unix()],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::appdb::test_db;

    #[test]
    fn only_the_hash_of_the_session_id_is_stored() {
        let (_d, db) = test_db();
        let u = db
            .create_user("alice", "correct horse battery", false)
            .unwrap();
        let s = db.create_session(u.id, Duration::from_secs(60)).unwrap();
        assert_eq!(URL_SAFE_NO_PAD.decode(&s.token).unwrap().len(), 32);
        let stored: String = db
            .conn()
            .unwrap()
            .query_row("SELECT token_hash FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, hash_token(&s.token));
        assert_ne!(stored, s.token);
        let info = db.lookup_session(&s.token).unwrap().unwrap();
        assert_eq!(info.user.id, u.id);
        assert_eq!(info.csrf_token, s.csrf_token);
    }

    #[test]
    fn expired_and_revoked_sessions_are_rejected() {
        let (_d, db) = test_db();
        let u = db
            .create_user("alice", "correct horse battery", false)
            .unwrap();
        let expired = db.create_session(u.id, Duration::ZERO).unwrap();
        assert!(db.lookup_session(&expired.token).unwrap().is_none());

        let live = db.create_session(u.id, Duration::from_secs(60)).unwrap();
        assert!(db.revoke_session(&hash_token(&live.token)).unwrap());
        assert!(db.lookup_session(&live.token).unwrap().is_none());

        let other = db.create_session(u.id, Duration::from_secs(60)).unwrap();
        db.set_password(u.id, "another long password").unwrap();
        assert!(db.lookup_session(&other.token).unwrap().is_none());
        assert!(db.purge_sessions().unwrap() >= 3);
    }

    #[test]
    fn tokens_are_unique() {
        assert_ne!(random_token(), random_token());
    }
}
