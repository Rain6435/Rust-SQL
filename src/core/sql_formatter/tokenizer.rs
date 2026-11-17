//! Lexical tokenizer and SQL keyword dictionary used by [`super::format_sql`].

/// Returns `true` if `upper` is a reserved SQL keyword that should be uppercased when formatting.
pub fn is_sql_keyword(upper: &str) -> bool {
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

/// Splits a raw SQL string into tokens while preserving quoted strings and punctuation.
pub fn tokenize_sql(sql: &str) -> Vec<String> {
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
