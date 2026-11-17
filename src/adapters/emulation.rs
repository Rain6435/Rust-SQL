//! Process-wide opt-in for the in-memory dialect emulator.
//!
//! Only PostgreSQL, Redshift, and SQLite have live drivers. MySQL, MariaDB, SQL Server, and
//! Cassandra can only be emulated, and only after the user opts in with `--emulate` or the
//! `emulateUnsupportedDialects` config setting. Without the opt-in, connecting to those
//! dialects fails with [`DbCoreError::LiveConnectionNotImplemented`](crate::core::errors::DbCoreError).

use std::sync::atomic::{AtomicBool, Ordering};

static EMULATION_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enables or disables emulation for dialects that have no live driver.
pub fn set_emulation_enabled(enabled: bool) {
    EMULATION_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Returns whether the user has opted in to emulation for dialects without a live driver.
pub fn emulation_enabled() -> bool {
    EMULATION_ENABLED.load(Ordering::SeqCst)
}

/// Returns whether `adapter_key` connects to a real server (never to the emulator).
pub fn has_live_driver(adapter_key: &str) -> bool {
    matches!(adapter_key, "postgresql" | "redshift" | "sqlite")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_postgres_redshift_and_sqlite_are_live() {
        for key in ["postgresql", "redshift", "sqlite"] {
            assert!(has_live_driver(key), "{key} should be live");
        }
        for key in ["mysql", "mariadb", "sqlserver", "cassandra", "custom"] {
            assert!(!has_live_driver(key), "{key} should not be live");
        }
    }
}
