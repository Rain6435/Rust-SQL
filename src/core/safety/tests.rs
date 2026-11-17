//! Tests for the safety analyzer: known bypasses, fail-closed parsing, and Read-Only coverage.

use super::*;

fn risk(sql: &str, read_only: bool) -> RiskLevel {
    analyze_query_safety(sql, read_only).overall_risk
}

// Bypasses that main classified as Safe before this module parsed statements structurally.

#[test]
fn commented_out_where_on_delete_is_critical() {
    let r = analyze_query_safety("DELETE FROM users -- WHERE id = 1", false);
    assert_eq!(r.overall_risk, RiskLevel::Critical);
    assert!(r.requires_confirmation);
    let r = analyze_query_safety("DELETE FROM users /* WHERE id = 1 */;", false);
    assert_eq!(r.overall_risk, RiskLevel::Critical);
}

#[test]
fn subquery_where_on_update_is_critical() {
    let r = analyze_query_safety("UPDATE users SET flag = (SELECT 1 WHERE 1=1)", false);
    assert_eq!(r.overall_risk, RiskLevel::Critical);
    assert!(r.requires_confirmation);
    assert_eq!(r.findings[0].statement_type, "UPDATE");
}

#[test]
fn unparseable_statement_fails_closed() {
    for read_only in [true, false] {
        let r = analyze_query_safety("SELEC * FRM users", read_only);
        assert_eq!(r.overall_risk, RiskLevel::Critical, "read_only={read_only}");
        assert!(!r.is_read_only_safe);
        assert!(r.requires_confirmation);
        assert!(r.findings[0].reason.contains("parsing failed"));
    }
}

#[test]
fn with_delete_without_predicate_is_critical() {
    let sql = "WITH x AS (SELECT 1) DELETE FROM users";
    let a = analyze_script(sql, SqlDialect::PostgreSql, false);
    assert_eq!(a.overall_risk(), RiskLevel::Critical);
    assert!(!a.is_read_only_safe());
    assert_eq!(a.statements[0].kind, StatementKind::Delete);
    assert_eq!(risk(sql, true), RiskLevel::Critical);
}

#[test]
fn data_modifying_cte_is_a_modification() {
    let sql = "WITH gone AS (DELETE FROM users WHERE id = 1 RETURNING *) SELECT * FROM gone";
    let a = analyze_script(sql, SqlDialect::PostgreSql, true);
    assert!(a.is_blocked());
    assert_eq!(a.overall_risk(), RiskLevel::Critical);
}

// Safe cases stay Safe.

#[test]
fn safe_statements_stay_safe() {
    for sql in [
        "SELECT * FROM users WHERE id = 1;",
        "DELETE FROM users WHERE id = 5;",
        "UPDATE users SET name = 'x' WHERE id = 5;",
        "UPDATE users SET flag = (SELECT 1 WHERE 1=1) WHERE id = 5;",
        "INSERT INTO users (name) VALUES ('a');",
        "-- only a comment",
        "",
    ] {
        assert_eq!(risk(sql, false), RiskLevel::Safe, "{sql}");
    }
    let r = analyze_query_safety("SELECT * FROM users WHERE id = 1;", true);
    assert!(r.is_read_only_safe);
    assert!(!r.requires_confirmation);
    assert!(!analyze_query_safety("DELETE FROM users WHERE id = 5;", false).is_read_only_safe);
}

#[test]
fn destructive_ddl_is_flagged() {
    assert_eq!(risk("DROP TABLE users", false), RiskLevel::Critical);
    assert_eq!(risk("TRUNCATE TABLE users", false), RiskLevel::Critical);
    assert_eq!(risk("DROP SCHEMA app CASCADE", false), RiskLevel::Critical);
    assert_eq!(risk("DROP VIEW v", false), RiskLevel::Warning);
    assert_eq!(
        risk("ALTER TABLE users ADD COLUMN x INT", false),
        RiskLevel::Warning
    );
    assert_eq!(risk("DELETE FROM users", false), RiskLevel::Critical);
}

#[test]
fn only_the_risky_statement_in_a_script_is_reported() {
    let r = analyze_query_safety("SELECT 1; DELETE FROM users; SELECT 2;", false);
    assert_eq!(r.overall_risk, RiskLevel::Critical);
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].statement_index, 1);
}

// Read-Only mode: everything not positively read-only is blocked.

fn assert_blocked(dialect: SqlDialect, sql: &str) {
    let a = analyze_script(sql, dialect, true);
    assert!(a.is_blocked(), "{dialect:?}: {sql} should be blocked");
    assert_eq!(a.overall_risk(), RiskLevel::Critical, "{sql}");
    assert!(a.requires_confirmation());
    assert!(a.blocked_reason().is_some(), "{sql}");
}

fn assert_allowed(dialect: SqlDialect, sql: &str) {
    let a = analyze_script(sql, dialect, true);
    assert!(
        !a.is_blocked(),
        "{dialect:?}: {sql} should be allowed: {a:?}"
    );
    assert_eq!(a.overall_risk(), RiskLevel::Safe, "{sql}");
}

#[test]
fn read_only_blocks_modifications_listed_in_the_brief() {
    use SqlDialect::*;
    assert_blocked(
        PostgreSql,
        "WITH x AS (SELECT 1) DELETE FROM users WHERE id = 1",
    );
    assert_blocked(
        PostgreSql,
        "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE",
    );
    assert_blocked(MySql, "REPLACE INTO t VALUES (1)");
    assert_blocked(Sqlite, "REPLACE INTO t VALUES (1)");
    assert_blocked(PostgreSql, "CALL wipe_everything()");
    assert_blocked(PostgreSql, "COPY t FROM '/tmp/x.csv'");
    assert_blocked(PostgreSql, "COPY t TO STDOUT");
    assert_blocked(PostgreSql, "GRANT ALL ON t TO bob");
    assert_blocked(PostgreSql, "REVOKE ALL ON t FROM bob");
    assert_blocked(PostgreSql, "SELECT * INTO new_t FROM t");
    assert_blocked(MsSql, "SELECT * INTO new_t FROM t");
}

#[test]
fn read_only_blocks_session_escapes_and_side_channels() {
    use SqlDialect::*;
    assert_blocked(PostgreSql, "SET default_transaction_read_only = off");
    assert_blocked(
        PostgreSql,
        "SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE",
    );
    assert_blocked(PostgreSql, "BEGIN READ WRITE");
    assert_blocked(PostgreSql, "SELECT * FROM t FOR UPDATE");
    assert_blocked(PostgreSql, "EXPLAIN ANALYZE DELETE FROM t");
    assert_blocked(Sqlite, "PRAGMA query_only = 0");
    assert_blocked(Sqlite, "PRAGMA journal_mode(WAL)");
    assert_blocked(Sqlite, "PRAGMA wal_checkpoint");
    assert_blocked(PostgreSql, "VACUUM");
    assert_blocked(PostgreSql, "LISTEN chan");
}

#[test]
fn read_only_allows_recognized_reads() {
    use SqlDialect::*;
    assert_allowed(PostgreSql, "SELECT 1; SELECT * FROM t WHERE a = 'x;y'");
    assert_allowed(PostgreSql, "WITH x AS (SELECT 1) SELECT * FROM x");
    assert_allowed(PostgreSql, "SELECT 1 UNION ALL SELECT 2");
    assert_allowed(PostgreSql, "EXPLAIN DELETE FROM t");
    assert_allowed(PostgreSql, "EXPLAIN ANALYZE SELECT 1");
    assert_allowed(PostgreSql, "BEGIN; SELECT 1; COMMIT;");
    assert_allowed(MySql, "SHOW TABLES");
    assert_allowed(Sqlite, "PRAGMA table_info(users)");
    assert_allowed(Sqlite, "PRAGMA user_version");
    assert_allowed(Generic, "VALUES (1), (2)");
}

#[test]
fn analysis_keeps_statement_spans_and_dialect() {
    let sql = "SELECT 1;\nDELETE FROM t";
    let a = analyze_script(sql, SqlDialect::Sqlite, false);
    assert_eq!(a.statements.len(), 2);
    let (s, e) = a.statements[1].span;
    assert_eq!(sql[s..e].trim(), "DELETE FROM t");
    assert_eq!(a.dialect, SqlDialect::Sqlite);
    assert_eq!(SqlDialect::from_adapter_key("mariadb"), SqlDialect::MySql);
    assert_eq!(
        SqlDialect::from_adapter_key("cassandra"),
        SqlDialect::Generic
    );
}
