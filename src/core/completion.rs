//! Schema-aware SQL autocompletion from the metadata cache.
//!
//! [`complete`] looks at the word before the cursor and its context:
//! - after `FROM`, `JOIN`, `INTO`, `UPDATE`, or `TABLE` it offers table names;
//! - after `name.` it offers the columns of that table, resolving aliases from the query
//!   (`FROM users u` / `JOIN orders AS o`);
//! - elsewhere it offers columns of the tables the query references, then table names, then
//!   SQL keywords.
//!
//! Nothing is offered inside a string literal or a `--` comment. Matching is a
//! case-insensitive prefix match. Columns come only from the metadata cache; the caller loads
//! the columns of [`referenced_tables`] first.

use std::collections::HashMap;

/// Keywords offered when nothing more specific fits.
pub const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "LIKE",
    "BETWEEN",
    "ORDER BY",
    "GROUP BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "JOIN",
    "LEFT JOIN",
    "INNER JOIN",
    "ON",
    "AS",
    "DISTINCT",
    "COUNT",
    "INSERT INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE FROM",
    "CREATE TABLE",
    "ALTER TABLE",
    "DROP TABLE",
    "UNION",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "EXISTS",
    "WITH",
    "ASC",
    "DESC",
];

const TABLE_CONTEXT: &[&str] = &["FROM", "JOIN", "INTO", "UPDATE", "TABLE"];

/// Most suggestions returned at once.
pub const MAX_ITEMS: usize = 50;

/// Tables and (cached) columns of the current database.
#[derive(Debug, Clone, Default)]
pub struct CompletionSchema {
    /// Table and view names.
    pub tables: Vec<String>,
    /// Column names keyed by table name.
    pub columns: HashMap<String, Vec<String>>,
}

impl CompletionSchema {
    fn columns_of(&self, table: &str) -> &[String] {
        self.columns
            .iter()
            .find(|(t, _)| t.eq_ignore_ascii_case(table))
            .map(|(_, c)| c.as_slice())
            .unwrap_or(&[])
    }
}

/// What a suggestion is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// A column of a referenced table.
    Column,
    /// A table or view.
    Table,
    /// A SQL keyword.
    Keyword,
}

/// One suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// Text to insert.
    pub label: String,
    /// Kind of suggestion.
    pub kind: CompletionKind,
}

/// Suggestions for the word that ends at the cursor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Completions {
    /// Char index where the word being completed starts (it ends at the cursor).
    pub start: usize,
    /// The partial word already typed.
    pub prefix: String,
    /// Suggestions, most specific first.
    pub items: Vec<CompletionItem>,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `(table, alias)` pairs named after `FROM` / `JOIN` in `text` (alias may equal the table).
pub fn referenced_tables(text: &str) -> Vec<(String, String)> {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')' || c == ';')
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if !(w.eq_ignore_ascii_case("FROM") || w.eq_ignore_ascii_case("JOIN")) {
            continue;
        }
        let Some(table) = words.get(i + 1) else {
            continue;
        };
        let table = table.trim_matches(|c| c == '"' || c == '`' || c == '[' || c == ']');
        let table = table.rsplit('.').next().unwrap_or(table).to_string();
        let mut alias_idx = i + 2;
        if words
            .get(alias_idx)
            .is_some_and(|a| a.eq_ignore_ascii_case("AS"))
        {
            alias_idx += 1;
        }
        let alias = words
            .get(alias_idx)
            .filter(|a| a.chars().all(is_ident) && !is_keyword(a))
            .map(|a| a.to_string())
            .unwrap_or_else(|| table.clone());
        out.push((table, alias));
    }
    out
}

fn is_keyword(w: &str) -> bool {
    KEYWORDS
        .iter()
        .any(|k| k.split(' ').any(|p| p.eq_ignore_ascii_case(w)))
}

/// Whether the cursor sits inside a string literal or after `--` on its line.
fn in_literal_or_comment(before: &str) -> bool {
    let line = before.rsplit('\n').next().unwrap_or(before);
    let quotes = before.chars().filter(|c| *c == '\'').count();
    quotes % 2 == 1 || line.contains("--")
}

/// Suggestions for the cursor at char index `cursor` in `text`.
///
/// With `explicit` (the user asked, e.g. Ctrl+Space) an empty word still gets suggestions;
/// otherwise at least one character must be typed, except right after `name.`.
pub fn complete(
    text: &str,
    cursor: usize,
    schema: &CompletionSchema,
    explicit: bool,
) -> Completions {
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    let before: String = chars[..cursor].iter().collect();
    if in_literal_or_comment(&before) {
        return Completions::default();
    }
    let mut start = cursor;
    while start > 0 && is_ident(chars[start - 1]) {
        start -= 1;
    }
    let prefix: String = chars[start..cursor].iter().collect();
    let lower = prefix.to_lowercase();
    let matches =
        |s: &str| s.to_lowercase().starts_with(&lower) && !s.eq_ignore_ascii_case(&prefix);
    let mut items: Vec<CompletionItem> = Vec::new();
    let mut push = |label: &str, kind| {
        if matches(label) && !items.iter().any(|i: &CompletionItem| i.label == label) {
            items.push(CompletionItem {
                label: label.to_string(),
                kind,
            });
        }
    };

    if start > 0 && chars[start - 1] == '.' {
        let mut q = start - 1;
        while q > 0 && is_ident(chars[q - 1]) {
            q -= 1;
        }
        let qualifier: String = chars[q..start - 1].iter().collect();
        let refs = referenced_tables(text);
        let table = refs
            .iter()
            .find(|(_, a)| a.eq_ignore_ascii_case(&qualifier))
            .map(|(t, _)| t.clone())
            .unwrap_or(qualifier);
        for c in schema.columns_of(&table) {
            push(c, CompletionKind::Column);
        }
    } else if prefix.is_empty() && !explicit {
        return Completions::default();
    } else {
        let prev_word: String = {
            let head = before[..before.len() - prefix.len()].trim_end();
            head.rsplit(|c: char| !is_ident(c))
                .next()
                .unwrap_or("")
                .to_string()
        };
        let table_ctx = TABLE_CONTEXT
            .iter()
            .any(|k| k.eq_ignore_ascii_case(&prev_word));
        if !table_ctx {
            for (t, _) in referenced_tables(text) {
                for c in schema.columns_of(&t) {
                    push(c, CompletionKind::Column);
                }
            }
        }
        for t in &schema.tables {
            push(t, CompletionKind::Table);
        }
        if !table_ctx {
            for k in KEYWORDS {
                push(k, CompletionKind::Keyword);
            }
        }
    }
    items.truncate(MAX_ITEMS);
    Completions {
        start,
        prefix,
        items,
    }
}

#[path = "completion_apply.rs"]
mod apply;

#[cfg(test)]
#[path = "completion_tests.rs"]
mod tests;
