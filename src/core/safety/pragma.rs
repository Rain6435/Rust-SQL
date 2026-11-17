//! SQLite `PRAGMA` handling: which pragmas only read, and parsing bare-identifier arguments.

use crate::core::safety::ast::Verdict;
use crate::core::sql_identifier::StatementKind as K;
use sqlparser::ast::ObjectName;
use sqlparser::parser::Parser;

/// Pragmas that only read, when called without an argument.
const READ_PRAGMAS: &[&str] = &[
    "application_id",
    "auto_vacuum",
    "cache_size",
    "collation_list",
    "compile_options",
    "data_version",
    "database_list",
    "encoding",
    "foreign_keys",
    "freelist_count",
    "function_list",
    "journal_mode",
    "module_list",
    "page_count",
    "page_size",
    "pragma_list",
    "schema_version",
    "synchronous",
    "table_list",
    "user_version",
];

/// Introspection pragmas whose argument names an object or a limit and never changes state.
const READ_PRAGMAS_WITH_ARG: &[&str] = &[
    "foreign_key_check",
    "foreign_key_list",
    "index_info",
    "index_list",
    "index_xinfo",
    "integrity_check",
    "quick_check",
    "table_info",
    "table_list",
    "table_xinfo",
];

pub(crate) fn pragma_verdict(name: &ObjectName, has_value: bool, is_eq: bool) -> Verdict {
    let full = name.to_string();
    let bare = full
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .trim_matches(|c| c == '"' || c == '`' || c == '[' || c == ']')
        .to_ascii_lowercase();
    let read_only = !is_eq
        && if has_value {
            READ_PRAGMAS_WITH_ARG.contains(&bare.as_str())
        } else {
            READ_PRAGMAS.contains(&bare.as_str()) || READ_PRAGMAS_WITH_ARG.contains(&bare.as_str())
        };
    if read_only {
        Verdict::read(K::Pragma)
    } else {
        Verdict::write(K::Pragma)
    }
}

/// Parses one statement. `sqlparser` accepts only literal pragma arguments, so a bare
/// identifier in `PRAGMA table_info(users)` is retried as a string literal.
pub(crate) fn parse_statement(
    dialect: &dyn sqlparser::dialect::Dialect,
    raw: &str,
) -> Result<Vec<sqlparser::ast::Statement>, sqlparser::parser::ParserError> {
    let first = Parser::parse_sql(dialect, raw);
    if first.is_ok() {
        return first;
    }
    static PRAGMA_IDENT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = PRAGMA_IDENT.get_or_init(|| {
        regex::Regex::new(
            r"(?is)^(\s*PRAGMA\s+[A-Za-z_][\w.]*\s*\(\s*)([A-Za-z_]\w*)(\s*\)\s*;?\s*)$",
        )
        .expect("static regex")
    });
    match re.captures(raw.trim()) {
        Some(c) => Parser::parse_sql(dialect, &format!("{}'{}'{}", &c[1], &c[2], &c[3])),
        None => first,
    }
}
