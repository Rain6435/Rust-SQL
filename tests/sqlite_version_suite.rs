//! The linked SQLite must include the fix for CVE-2026-119375 (RUSTSEC-2026-0459): before 3.54.0,
//! opening a database file with a sibling `-wal` file of a different page size could return
//! uninitialized memory. Rust-SQL opens SQLite files chosen by the user, so it builds SQLite from
//! the patched source bundled with `libsqlite3-sys` instead of linking the system library.

#[test]
fn linked_sqlite_includes_the_wal_page_size_fix() {
    let version = rusqlite::version_number();
    assert!(
        version >= 3_054_000,
        "SQLite {} is older than 3.54.0 (RUSTSEC-2026-0459)",
        rusqlite::version()
    );
}
