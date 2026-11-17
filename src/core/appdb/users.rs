//! Web UI accounts with Argon2id password hashes.
//!
//! The hash string is in PHC format, so it carries its own salt and parameters. A login for an
//! unknown user still runs one Argon2id verification against a dummy hash, so unknown users
//! and wrong passwords cost the same time and return the same result. Argon2 compares the
//! derived hash in constant time.

use super::{now_rfc3339, AppDb, AppDbError};
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;
use std::sync::OnceLock;

/// Minimum password length, in characters.
pub const MIN_PASSWORD_LEN: usize = 12;

/// A web UI account (never includes the password hash).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// Database ID.
    pub id: i64,
    /// Unique, case-insensitive login name.
    pub username: String,
    /// Whether the user manages users, grants, connections, and settings.
    pub is_admin: bool,
    /// Creation time (RFC 3339, UTC).
    pub created_at: String,
}

fn user_from_row(row: &Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get(0)?,
        username: row.get(1)?,
        is_admin: row.get::<_, i64>(2)? != 0,
        created_at: row.get(3)?,
    })
}

/// Validates a username: 1 to 64 characters from `A-Z a-z 0-9 . _ - @`.
pub fn validate_username(username: &str) -> Result<(), AppDbError> {
    let ok_chars = username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@'));
    if username.is_empty() || username.len() > 64 || !ok_chars {
        return Err(AppDbError::Invalid(
            "Username must be 1-64 characters: letters, digits, '.', '_', '-' or '@'".into(),
        ));
    }
    Ok(())
}

/// Validates the password policy (at least [`MIN_PASSWORD_LEN`] characters).
pub fn validate_password(password: &str) -> Result<(), AppDbError> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AppDbError::Invalid(format!(
            "Password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    Ok(())
}

/// Hashes `password` with Argon2id and a random salt, returning the PHC string.
pub fn hash_password(password: &str) -> Result<String, AppDbError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| AppDbError::Hash(e.to_string()))
}

fn verify_hash(password: &str, hash: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), hash)
        .is_ok()
}

fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY
        .get_or_init(|| hash_password("dummy-password-for-timing-equalization").unwrap_or_default())
}

const USER_COLUMNS: &str = "id, username, is_admin, created_at";

impl AppDb {
    /// Creates a user after validating the username and password policy.
    pub fn create_user(
        &self,
        username: &str,
        password: &str,
        is_admin: bool,
    ) -> Result<User, AppDbError> {
        validate_username(username)?;
        validate_password(password)?;
        let hash = hash_password(password)?;
        let conn = self.conn()?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE username = ?1)",
            [username],
            |r| r.get(0),
        )?;
        if exists {
            return Err(AppDbError::Invalid(format!(
                "User '{username}' already exists"
            )));
        }
        conn.execute(
            "INSERT INTO users (username, password_hash, is_admin, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![username, hash, is_admin as i64, now_rfc3339()],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        self.user_by_id(id)?.ok_or(AppDbError::NotFound("User"))
    }

    /// Verifies a login. Returns `None` for an unknown user *and* for a wrong password.
    pub fn verify_login(&self, username: &str, password: &str) -> Result<Option<User>, AppDbError> {
        let row: Option<(User, String)> = {
            let conn = self.conn()?;
            conn.query_row(
                &format!("SELECT {USER_COLUMNS}, password_hash FROM users WHERE username = ?1"),
                [username],
                |r| Ok((user_from_row(r)?, r.get::<_, String>(4)?)),
            )
            .optional()?
        };
        match row {
            Some((user, hash)) => Ok(verify_hash(password, &hash).then_some(user)),
            None => {
                let _ = verify_hash(password, dummy_hash());
                Ok(None)
            }
        }
    }

    /// Looks up a user by ID.
    pub fn user_by_id(&self, id: i64) -> Result<Option<User>, AppDbError> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                &format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?1"),
                [id],
                user_from_row,
            )
            .optional()?)
    }

    /// Lists all users ordered by username.
    pub fn list_users(&self) -> Result<Vec<User>, AppDbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {USER_COLUMNS} FROM users ORDER BY username"
        ))?;
        let rows = stmt.query_map([], user_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Number of accounts.
    pub fn user_count(&self) -> Result<i64, AppDbError> {
        let conn = self.conn()?;
        Ok(conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?)
    }

    fn admin_count(&self) -> Result<i64, AppDbError> {
        let conn = self.conn()?;
        Ok(
            conn.query_row("SELECT COUNT(*) FROM users WHERE is_admin = 1", [], |r| {
                r.get(0)
            })?,
        )
    }

    fn ensure_not_last_admin(&self, id: i64) -> Result<(), AppDbError> {
        let is_admin = self
            .user_by_id(id)?
            .ok_or(AppDbError::NotFound("User"))?
            .is_admin;
        if is_admin && self.admin_count()? <= 1 {
            return Err(AppDbError::Invalid(
                "The last administrator cannot be removed or demoted".into(),
            ));
        }
        Ok(())
    }

    /// Grants or removes the admin flag. Refuses to demote the last admin.
    pub fn set_admin(&self, id: i64, is_admin: bool) -> Result<(), AppDbError> {
        if !is_admin {
            self.ensure_not_last_admin(id)?;
        }
        let n = self.conn()?.execute(
            "UPDATE users SET is_admin = ?1 WHERE id = ?2",
            params![is_admin as i64, id],
        )?;
        if n == 0 {
            return Err(AppDbError::NotFound("User"));
        }
        Ok(())
    }

    /// Replaces a user's password and revokes all of that user's sessions.
    pub fn set_password(&self, id: i64, password: &str) -> Result<(), AppDbError> {
        validate_password(password)?;
        let hash = hash_password(password)?;
        let n = self.conn()?.execute(
            "UPDATE users SET password_hash = ?1 WHERE id = ?2",
            params![hash, id],
        )?;
        if n == 0 {
            return Err(AppDbError::NotFound("User"));
        }
        self.revoke_user_sessions(id)?;
        Ok(())
    }

    /// Deletes a user with their sessions and grants. Refuses to delete the last admin.
    pub fn delete_user(&self, id: i64) -> Result<(), AppDbError> {
        self.ensure_not_last_admin(id)?;
        let n = self
            .conn()?
            .execute("DELETE FROM users WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(AppDbError::NotFound("User"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "users_tests.rs"]
mod tests;
