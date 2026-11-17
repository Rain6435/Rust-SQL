//! Connection grants: which users may see and use which saved connections.

use super::{now_rfc3339, AppDb, AppDbError};
use rusqlite::params;
use serde::Serialize;

/// One user-to-connection grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    /// Grantee user ID.
    pub user_id: i64,
    /// Grantee username.
    pub username: String,
    /// Saved connection (server profile) ID.
    pub connection_id: String,
    /// Admin who granted it, if known.
    pub granted_by: Option<i64>,
    /// When it was granted (RFC 3339, UTC).
    pub granted_at: String,
}

impl AppDb {
    /// Grants `user_id` access to `connection_id`. Returns false if it already existed.
    pub fn add_grant(
        &self,
        user_id: i64,
        connection_id: &str,
        granted_by: Option<i64>,
    ) -> Result<bool, AppDbError> {
        if self.user_by_id(user_id)?.is_none() {
            return Err(AppDbError::NotFound("User"));
        }
        let n = self.conn()?.execute(
            "INSERT OR IGNORE INTO connection_grants (user_id, connection_id, granted_by, granted_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![user_id, connection_id, granted_by, now_rfc3339()],
        )?;
        Ok(n > 0)
    }

    /// Removes a grant. Returns whether one was removed.
    pub fn remove_grant(&self, user_id: i64, connection_id: &str) -> Result<bool, AppDbError> {
        let n = self.conn()?.execute(
            "DELETE FROM connection_grants WHERE user_id = ?1 AND connection_id = ?2",
            params![user_id, connection_id],
        )?;
        Ok(n > 0)
    }

    /// Removes every grant for a connection (used when the connection is deleted).
    pub fn remove_grants_for_connection(&self, connection_id: &str) -> Result<usize, AppDbError> {
        Ok(self.conn()?.execute(
            "DELETE FROM connection_grants WHERE connection_id = ?1",
            [connection_id],
        )?)
    }

    /// Whether `user_id` holds a grant for `connection_id`.
    pub fn has_grant(&self, user_id: i64, connection_id: &str) -> Result<bool, AppDbError> {
        Ok(self.conn()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM connection_grants WHERE user_id = ?1 AND connection_id = ?2)",
            params![user_id, connection_id],
            |r| r.get(0),
        )?)
    }

    /// Lists grants, optionally for one user.
    pub fn list_grants(&self, user_id: Option<i64>) -> Result<Vec<Grant>, AppDbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT g.user_id, u.username, g.connection_id, g.granted_by, g.granted_at
             FROM connection_grants g JOIN users u ON u.id = g.user_id
             WHERE ?1 IS NULL OR g.user_id = ?1
             ORDER BY u.username, g.connection_id",
        )?;
        let rows = stmt.query_map([user_id], |r| {
            Ok(Grant {
                user_id: r.get(0)?,
                username: r.get(1)?,
                connection_id: r.get(2)?,
                granted_by: r.get(3)?,
                granted_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use crate::core::appdb::test_db;

    #[test]
    fn grants_are_per_user_and_per_connection() {
        let (_d, db) = test_db();
        let a = db
            .create_user("alice", "correct horse battery", false)
            .unwrap();
        let b = db
            .create_user("bob", "correct horse battery", false)
            .unwrap();
        assert!(db.add_grant(a.id, "conn-1", None).unwrap());
        assert!(!db.add_grant(a.id, "conn-1", None).unwrap());
        assert!(db.has_grant(a.id, "conn-1").unwrap());
        assert!(!db.has_grant(b.id, "conn-1").unwrap());
        assert!(!db.has_grant(a.id, "conn-2").unwrap());
        assert_eq!(db.list_grants(Some(a.id)).unwrap().len(), 1);
        assert!(db.add_grant(999, "conn-1", None).is_err());

        db.delete_user(a.id).unwrap();
        assert!(db.list_grants(None).unwrap().is_empty());
        db.add_grant(b.id, "conn-1", None).unwrap();
        assert_eq!(db.remove_grants_for_connection("conn-1").unwrap(), 1);
    }
}
