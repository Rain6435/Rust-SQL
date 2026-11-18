//! SQLite connection opening, Read-Only sessions, and shared-connection helpers.
//!
//! In Read-Only mode a file database is opened with `SQLITE_OPEN_READ_ONLY`, so SQLite itself
//! rejects writes. An in-memory database cannot be reopened without losing its contents, so it
//! uses `PRAGMA query_only = ON` instead.

use crate::adapters::sqlite::timeout::apply_busy_timeout;
use crate::adapters::sqlite::{map_sqlite_err, SqliteAdapter};
use crate::core::errors::DbCoreError;
use rusqlite::{Connection, OpenFlags};
use std::sync::atomic::Ordering;

/// Opens `path` (or an in-memory database for `":memory:"`), read-only when `read_only` is set.
pub fn open_sqlite(path: &str, read_only: bool) -> Result<Connection, DbCoreError> {
    if path == ":memory:" {
        let conn = Connection::open_in_memory().map_err(map_sqlite_err)?;
        set_query_only(&conn, read_only)?;
        return Ok(conn);
    }
    if read_only {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        Connection::open_with_flags(path, flags).map_err(map_sqlite_err)
    } else {
        Connection::open(path).map_err(map_sqlite_err)
    }
}

/// Turns SQLite's `query_only` guard on or off for `conn`.
pub(crate) fn set_query_only(conn: &Connection, on: bool) -> Result<(), DbCoreError> {
    conn.pragma_update(None, "query_only", on)
        .map_err(map_sqlite_err)
}

impl SqliteAdapter {
    /// Runs `f` on the shared connection, opening it first if needed.
    pub(crate) fn open_or_with_conn<F, T>(&self, f: F) -> Result<T, DbCoreError>
    where
        F: FnOnce(&Connection) -> Result<T, DbCoreError>,
    {
        let mut guard = self
            .shared_conn
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        if guard.is_none() {
            let conn = open_sqlite(&self.db_path, self.read_only.load(Ordering::SeqCst))?;
            apply_busy_timeout(&conn, self.statement_timeout)?;
            *guard = Some(conn);
        }
        match guard.as_ref() {
            Some(conn) => f(conn),
            None => Err(DbCoreError::NoConnectionAvailable),
        }
    }

    /// Runs `sql` and collects column `col_idx` of every row as text.
    pub(crate) fn query_strings(
        &self,
        sql: &str,
        col_idx: usize,
    ) -> Result<Vec<String>, DbCoreError> {
        self.open_or_with_conn(|conn| {
            let mut stmt = conn.prepare(sql).map_err(map_sqlite_err)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(col_idx))
                .map_err(map_sqlite_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(map_sqlite_err)
        })
    }

    /// Puts the session into (or out of) Read-Only mode.
    ///
    /// A file database is reopened with the matching flags, which drops any open transaction.
    pub(crate) fn apply_read_only(&self, read_only: bool) -> Result<(), DbCoreError> {
        let mut guard = self
            .shared_conn
            .lock()
            .map_err(|e| DbCoreError::Message(e.to_string()))?;
        self.read_only.store(read_only, Ordering::SeqCst);
        match guard.as_ref() {
            Some(conn) if self.db_path == ":memory:" => set_query_only(conn, read_only),
            Some(_) => {
                *guard = None;
                let conn = open_sqlite(&self.db_path, read_only)?;
                apply_busy_timeout(&conn, self.statement_timeout)?;
                *guard = Some(conn);
                Ok(())
            }
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::DatabaseAdapter;
    use crate::common::types::Server;

    fn file_adapter(dir: &tempfile::TempDir) -> SqliteAdapter {
        let path = dir.path().join("ro.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1);")
            .unwrap();
        SqliteAdapter::new(&Server::default(), path.to_str())
    }

    #[tokio::test]
    async fn read_only_file_session_rejects_writes_at_the_driver() {
        let dir = tempfile::tempdir().unwrap();
        let mut adapter = file_adapter(&dir);
        adapter.connect().await.unwrap();
        adapter.set_read_only(true).await.unwrap();

        let err = adapter.execute_query("DELETE FROM t").await.unwrap_err();
        assert!(err.to_string().to_lowercase().contains("readonly"), "{err}");
        let q = adapter.query("INSERT INTO t VALUES (2)").unwrap();
        assert!(q.execute().await.is_err());
        let rows = adapter.execute_query("SELECT id FROM t").await.unwrap();
        assert_eq!(rows[0].rows.len(), 1);

        adapter.set_read_only(false).await.unwrap();
        adapter.execute_query("DELETE FROM t").await.unwrap();
    }

    #[tokio::test]
    async fn read_only_applies_when_connecting_after_the_flag_is_set() {
        let dir = tempfile::tempdir().unwrap();
        let mut adapter = file_adapter(&dir);
        adapter.set_read_only(true).await.unwrap();
        adapter.connect().await.unwrap();
        assert!(adapter.execute_query("DROP TABLE t").await.is_err());
    }

    #[tokio::test]
    async fn read_only_memory_session_keeps_data_and_rejects_writes() {
        let mut adapter = SqliteAdapter::new(&Server::default(), Some(":memory:"));
        adapter.connect().await.unwrap();
        adapter
            .execute_query("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1);")
            .await
            .unwrap();
        adapter.set_read_only(true).await.unwrap();
        assert!(adapter.execute_query("DELETE FROM t").await.is_err());
        let rows = adapter.execute_query("SELECT id FROM t").await.unwrap();
        assert_eq!(rows[0].rows.len(), 1);
    }
}
