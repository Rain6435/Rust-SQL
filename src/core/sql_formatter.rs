/// Multi-dialect SQL formatter (`Mod-i` / `BROWSER_MENU_EDITOR_FORMAT`).
pub fn format_sql(query: &str, client: Option<&str>) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let _dialect = match client.unwrap_or("sql") {
        "cassandra" | "sqlite" => "sql",
        "sqlserver" => "tsql",
        other => other,
    };

    let major_clauses = [
        "SELECT",
        "FROM",
        "WHERE",
        "LEFT JOIN",
        "RIGHT JOIN",
        "INNER JOIN",
        "OUTER JOIN",
        "FULL JOIN",
        "CROSS JOIN",
        "JOIN",
        "ON",
        "GROUP BY",
        "ORDER BY",
        "HAVING",
        "LIMIT",
        "OFFSET",
        "UNION ALL",
        "UNION",
        "INSERT INTO",
        "VALUES",
        "UPDATE",
        "SET",
        "DELETE FROM",
        "CREATE TABLE",
        "ALTER TABLE",
        "DROP TABLE",
    ];

    let tokens = tokenize_sql(trimmed);
    let mut out = String::new();
    let mut indent: usize = 0;
    let mut i = 0;
    let mut line_start = true;

    while i < tokens.len() {
        let tok = &tokens[i];
        if tok == ";" {
            out = out.trim_end().to_string();
            out.push_str(";\n\n");
            line_start = true;
            indent = 0;
            i += 1;
            continue;
        }
        if tok == "," {
            out = out.trim_end().to_string();
            out.push_str(",\n");
            for _ in 0..(indent + 1) {
                out.push_str("  ");
            }
            line_start = false;
            i += 1;
            continue;
        }
        if tok == "(" {
            out.push_str(" (");
            indent += 1;
            line_start = false;
            i += 1;
            continue;
        }
        if tok == ")" {
            indent = indent.saturating_sub(1);
            out = out.trim_end().to_string();
            out.push(')');
            line_start = false;
            i += 1;
            continue;
        }

        // Check two-word major clauses
        let upper = tok.to_uppercase();
        let next_upper = tokens.get(i + 1).map(|s| s.to_uppercase());
        let two_word = next_upper.as_ref().map(|n| format!("{upper} {n}"));

        if let Some(ref tw) = two_word {
            if major_clauses.contains(&tw.as_str()) {
                if !out.is_empty() && !out.ends_with('\n') {
                    out = out.trim_end().to_string();
                    out.push('\n');
                }
                for _ in 0..indent {
                    out.push_str("  ");
                }
                out.push_str(tw);
                out.push('\n');
                for _ in 0..(indent + 1) {
                    out.push_str("  ");
                }
                line_start = false;
                i += 2;
                continue;
            }
        }

        if major_clauses.contains(&upper.as_str()) {
            if !out.is_empty() && !out.ends_with('\n') {
                out = out.trim_end().to_string();
                out.push('\n');
            }
            for _ in 0..indent {
                out.push_str("  ");
            }
            out.push_str(&upper);
            out.push('\n');
            for _ in 0..(indent + 1) {
                out.push_str("  ");
            }
            line_start = false;
            i += 1;
            continue;
        }

        if matches!(upper.as_str(), "AND" | "OR") {
            if !out.is_empty() && !out.ends_with('\n') {
                out = out.trim_end().to_string();
                out.push('\n');
            }
            for _ in 0..(indent + 1) {
                out.push_str("  ");
            }
            out.push_str(&upper);
            out.push(' ');
            line_start = false;
            i += 1;
            continue;
        }

        let formatted_tok = if is_sql_keyword(&upper) {
            upper
        } else {
            tok.clone()
        };

        if !line_start && !out.ends_with(' ') && !out.ends_with('\n') && !out.ends_with('(') {
            out.push(' ');
        }
        out.push_str(&formatted_tok);
        line_start = false;
        i += 1;
    }

    out.trim().to_string()
}

fn is_sql_keyword(upper: &str) -> bool {
    matches!(
        upper,
        "SELECT"
            | "FROM"
            | "WHERE"
            | "AND"
            | "OR"
            | "NOT"
            | "NULL"
            | "IS"
            | "IN"
            | "AS"
            | "ON"
            | "JOIN"
            | "LEFT"
            | "RIGHT"
            | "INNER"
            | "OUTER"
            | "FULL"
            | "CROSS"
            | "GROUP"
            | "BY"
            | "ORDER"
            | "ASC"
            | "DESC"
            | "HAVING"
            | "LIMIT"
            | "OFFSET"
            | "TOP"
            | "DISTINCT"
            | "ALL"
            | "UNION"
            | "INSERT"
            | "INTO"
            | "VALUES"
            | "UPDATE"
            | "SET"
            | "DELETE"
            | "CREATE"
            | "TABLE"
            | "VIEW"
            | "INDEX"
            | "TRIGGER"
            | "PROCEDURE"
            | "FUNCTION"
            | "DATABASE"
            | "SCHEMA"
            | "DROP"
            | "ALTER"
            | "TRUNCATE"
            | "PRIMARY"
            | "KEY"
            | "FOREIGN"
            | "REFERENCES"
            | "DEFAULT"
            | "CASE"
            | "WHEN"
            | "THEN"
            | "ELSE"
            | "END"
            | "EXISTS"
            | "BETWEEN"
            | "LIKE"
            | "COUNT"
            | "SUM"
            | "AVG"
            | "MIN"
            | "MAX"
    )
}

fn tokenize_sql(sql: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = sql.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if matches!(c, ',' | ';' | '(' | ')') {
            tokens.push(c.to_string());
            i += 1;
            continue;
        }
        if matches!(c, '\'' | '"' | '`') {
            let quote = c;
            let mut s = String::new();
            s.push(quote);
            i += 1;
            while i < len {
                s.push(chars[i]);
                if chars[i] == quote {
                    if i + 1 < len && chars[i + 1] == quote {
                        s.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            tokens.push(s);
            continue;
        }
        let mut word = String::new();
        while i < len && !chars[i].is_whitespace() && !matches!(chars[i], ',' | ';' | '(' | ')') {
            word.push(chars[i]);
            i += 1;
        }
        tokens.push(word);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_sql() {
        let formatted = format_sql(
            "select id, username from users where id = 1 and active = 1;",
            Some("postgresql"),
        );
        assert!(formatted.contains("SELECT"));
        assert!(formatted.contains("FROM"));
        assert!(formatted.contains("WHERE"));
        assert!(formatted.ends_with(';'));
    }
}
