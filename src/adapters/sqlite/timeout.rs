//! Per-statement timeouts for SQLite (and the SQLite-backed emulator).
//!
//! SQLite has no server-side statement timeout. A [`StatementWatchdog`] starts a thread that
//! calls `sqlite3_interrupt` through the connection's [`InterruptHandle`] once the deadline
//! passes. The interrupted statement fails with `SQLITE_INTERRUPT`, which
//! [`StatementWatchdog::finish`] turns into [`DbCoreError::StatementTimeout`] when the watchdog
//! fired (and leaves a user cancel as a cancel). Lock waits are bounded by `busy_timeout`.

use crate::core::errors::DbCoreError;
use rusqlite::{Connection, InterruptHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Interrupts a running SQLite statement when its timeout passes.
pub struct StatementWatchdog {
    stop: Option<mpsc::Sender<()>>,
    fired: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    timeout: Duration,
}

impl StatementWatchdog {
    /// Starts a watchdog for `conn`, or returns `None` when there is no timeout.
    pub fn start(conn: &Connection, timeout: Option<Duration>) -> Option<Self> {
        let timeout = timeout?;
        Some(Self::with_handle(conn.get_interrupt_handle(), timeout))
    }

    fn with_handle(handle: InterruptHandle, timeout: Duration) -> Self {
        let (tx, rx) = mpsc::channel::<()>();
        let fired = Arc::new(AtomicBool::new(false));
        let flag = fired.clone();
        let thread = std::thread::spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(timeout) {
                flag.store(true, Ordering::SeqCst);
                handle.interrupt();
            }
        });
        Self {
            stop: Some(tx),
            fired,
            thread: Some(thread),
            timeout,
        }
    }

    /// Whether the deadline passed and the statement was interrupted.
    pub fn fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }

    /// Stops the watchdog and maps an interrupt it caused to [`DbCoreError::StatementTimeout`].
    pub fn finish<T>(mut self, result: Result<T, DbCoreError>) -> Result<T, DbCoreError> {
        self.stop_thread();
        match result {
            Err(DbCoreError::CanceledByUser(_)) if self.fired() => Err(timeout_error(self.timeout)),
            other => other,
        }
    }

    fn stop_thread(&mut self) {
        self.stop.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for StatementWatchdog {
    fn drop(&mut self) {
        self.stop_thread();
    }
}

/// The error reported when a statement exceeds `timeout`.
pub fn timeout_error(timeout: Duration) -> DbCoreError {
    DbCoreError::StatementTimeout(format!(
        "it ran longer than the {}s statement timeout set for this connection",
        timeout.as_secs()
    ))
}

/// Applies the connection's timeout to lock waits (`busy_timeout`).
pub fn apply_busy_timeout(conn: &Connection, timeout: Option<Duration>) -> Result<(), DbCoreError> {
    match timeout {
        Some(t) => conn
            .busy_timeout(t)
            .map_err(|e| DbCoreError::Message(format!("Could not set busy_timeout: {e}"))),
        None => Ok(()),
    }
}

/// Runs `f` on `conn` under a watchdog for `timeout` (no watchdog when `None`).
pub fn run_with_timeout<T>(
    conn: &Connection,
    timeout: Option<Duration>,
    f: impl FnOnce(&Connection) -> Result<T, DbCoreError>,
) -> Result<T, DbCoreError> {
    match StatementWatchdog::start(conn, timeout) {
        Some(dog) => {
            let result = f(conn);
            dog.finish(result)
        }
        None => f(conn),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite::map_sqlite_err;

    const SLOW: &str = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) \
                        SELECT count(*) FROM c";

    #[test]
    fn a_slow_statement_is_interrupted_as_a_timeout() {
        let conn = Connection::open_in_memory().unwrap();
        let started = std::time::Instant::now();
        let err = run_with_timeout(&conn, Some(Duration::from_millis(200)), |c| {
            c.query_row(SLOW, [], |r| r.get::<_, i64>(0))
                .map_err(map_sqlite_err)
        })
        .unwrap_err();
        assert!(matches!(err, DbCoreError::StatementTimeout(_)), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_fast_statement_is_not_affected() {
        let conn = Connection::open_in_memory().unwrap();
        let v = run_with_timeout(&conn, Some(Duration::from_secs(5)), |c| {
            c.query_row("SELECT 41 + 1", [], |r| r.get::<_, i64>(0))
                .map_err(map_sqlite_err)
        })
        .unwrap();
        assert_eq!(v, 42);
        // The connection stays usable after the watchdog stops.
        let again: i64 = conn.query_row("SELECT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(again, 1);
    }

    #[test]
    fn a_user_cancel_is_not_reported_as_a_timeout() {
        let conn = Connection::open_in_memory().unwrap();
        let dog = StatementWatchdog::start(&conn, Some(Duration::from_secs(30))).unwrap();
        let canceled: Result<(), DbCoreError> = Err(DbCoreError::CanceledByUser(
            crate::core::errors::CanceledByUserError::new(),
        ));
        assert!(matches!(
            dog.finish(canceled),
            Err(DbCoreError::CanceledByUser(_))
        ));
    }
}
