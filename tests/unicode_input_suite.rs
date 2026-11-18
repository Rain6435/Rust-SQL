//! Text scanners must not panic on user input: every function that walks SQL typed by a user
//! (splitting, classification, safety analysis, parameter scanning and binding, formatting,
//! paging, autocompletion) runs over scripts mixing multi-byte characters with quotes,
//! comments, dollar-quoting and placeholders, and over every truncation of those scripts.

use rust_sql::core::completion::{complete, referenced_tables, CompletionSchema};
use rust_sql::core::paging::pageable_select;
use rust_sql::core::params::{
    bind_for_display, extract_query_parameters, prepare_statement, scan_placeholders, ParamValue,
    PlaceholderStyle, QueryParams,
};
use rust_sql::core::safety::{analyze_query_safety, analyze_script, SqlDialect};
use rust_sql::core::sql_formatter::format_sql;
use rust_sql::core::sql_identifier::identify;
use rust_sql::core::sql_identifier::splitter::{split_sql_statements, strip_comments};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Scripts with a multi-byte character right after each construct a scanner treats specially.
const CORPUS: &[&str] = &[
    "SELECT $$é$$;",
    "SELECT $tag$日本語$tag$; SELECT 1",
    "DO $$ BEGIN RAISE NOTICE 'ü'; END $$;",
    "SELECT 'a\\é' AS x;",
    "SELECT 'it''s ß' -- commentaire é\nFROM t;",
    "SELECT \"çol\" FROM `tåble` WHERE [naïve] = 1;",
    "/* ✓ */ DELETE FROM t WHERE id = :ïd;",
    "UPDATE t SET a = @é, b = $1, c = :name€ WHERE x = ?",
    "SELECT 1 -- 😀",
    "SELECT '😀' FROM 😀 WHERE 😀 = $😀",
    "INSERT INTO t VALUES ('é'), (\"ü\"); ;; é",
    "BEGIN; SELECT 'é'; END;",
    "SELECT :a::text, @@version, '€' FROM t",
    "é",
    "$",
    "'",
    "--",
    "/*",
];

fn params() -> QueryParams {
    let mut p = QueryParams::new();
    for name in ["name", "a", "ïd", "é", "$1", "1"] {
        p.insert(name.to_string(), ParamValue::Text("vé'\\--".to_string()));
    }
    p
}

/// Every prefix of `s` that ends on a character boundary, plus `s` itself.
fn truncations(s: &str) -> impl Iterator<Item = &str> {
    s.char_indices()
        .map(move |(i, _)| &s[..i])
        .chain(std::iter::once(s))
}

fn exercise(sql: &str) {
    let _ = split_sql_statements(sql);
    let _ = strip_comments(sql);
    let _ = identify(sql, false);
    let _ = identify(sql, true);
    let _ = analyze_query_safety(sql, true);
    for dialect in [
        SqlDialect::Generic,
        SqlDialect::PostgreSql,
        SqlDialect::MySql,
        SqlDialect::MsSql,
        SqlDialect::Sqlite,
    ] {
        let _ = analyze_script(sql, dialect, true);
    }
    let _ = scan_placeholders(sql);
    let _ = extract_query_parameters(sql);
    let p = params();
    let _ = prepare_statement(sql, &p, PlaceholderStyle::Question);
    let _ = prepare_statement(sql, &p, PlaceholderStyle::Dollar);
    let _ = bind_for_display(sql, &p, "mysql");
    let _ = bind_for_display(sql, &p, "postgresql");
    let _ = format_sql(sql, Some("postgresql"));
    let _ = pageable_select(sql, "postgresql");
    let _ = referenced_tables(sql);
    let schema = CompletionSchema::default();
    for cursor in 0..=sql.chars().count() + 1 {
        let _ = complete(sql, cursor, &schema, true);
    }
}

#[test]
fn text_scanners_never_panic_on_multibyte_or_truncated_input() {
    let mut panics = Vec::new();
    for script in CORPUS {
        for sql in truncations(script) {
            if catch_unwind(AssertUnwindSafe(|| exercise(sql))).is_err() {
                panics.push(sql.to_string());
            }
        }
    }
    assert!(panics.is_empty(), "scanners panicked on: {panics:#?}");
}

#[test]
fn dollar_quoted_body_with_multibyte_text_is_one_statement() {
    let sql = "SELECT $$é; ü$$; SELECT 2";
    let spans = split_sql_statements(sql);
    let texts: Vec<&str> = spans.iter().map(|(_, _, s)| s.trim()).collect();
    assert_eq!(texts, vec!["SELECT $$é; ü$$;", "SELECT 2"]);
}
