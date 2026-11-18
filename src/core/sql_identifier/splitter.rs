//! Lexer-based SQL script splitter handling quotes, dollar-quoting, comments, and `BEGIN ... END` blocks.

/// Splits `sql` into `(start_byte, end_byte, slice)` statement spans terminated by top-level semicolons.
pub fn split_sql_statements(sql: &str) -> Vec<(usize, usize, &str)> {
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut statements = Vec::new();
    let mut stmt_start = 0;
    let mut i = 0;

    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backtick = false;
    let mut in_bracket = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut dollar_tag: Option<&str> = None;
    let mut paren_depth: usize = 0;
    let mut begin_depth: usize = 0;

    while i < len {
        let c = bytes[i];
        let next = if i + 1 < len {
            Some(bytes[i + 1])
        } else {
            None
        };

        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if c == b'*' && next == Some(b'/') {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(tag) = dollar_tag {
            if bytes[i..].starts_with(tag.as_bytes()) {
                dollar_tag = None;
                i += tag.len();
                continue;
            }
            i += 1;
            continue;
        }
        if in_single_quote {
            if c == b'\\' && next.is_some() {
                i += 2;
                continue;
            }
            if c == b'\'' {
                if next == Some(b'\'') {
                    i += 2;
                    continue;
                }
                in_single_quote = false;
            }
            i += 1;
            continue;
        }
        if in_double_quote {
            if c == b'"' {
                if next == Some(b'"') {
                    i += 2;
                    continue;
                }
                in_double_quote = false;
            }
            i += 1;
            continue;
        }
        if in_backtick {
            if c == b'`' {
                if next == Some(b'`') {
                    i += 2;
                    continue;
                }
                in_backtick = false;
            }
            i += 1;
            continue;
        }
        if in_bracket {
            if c == b']' {
                in_bracket = false;
            }
            i += 1;
            continue;
        }

        if c == b'-' && next == Some(b'-') {
            in_line_comment = true;
            i += 2;
            continue;
        }
        if c == b'/' && next == Some(b'*') {
            in_block_comment = true;
            i += 2;
            continue;
        }
        if c == b'$' {
            if let Some(tag) = parse_dollar_tag(&sql[i..]) {
                dollar_tag = Some(tag);
                i += tag.len();
                continue;
            }
        }

        match c {
            b'\'' => in_single_quote = true,
            b'"' => in_double_quote = true,
            b'`' => in_backtick = true,
            b'[' => in_bracket = true,
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            _ if c.is_ascii_alphabetic() => {
                let prev_is_word =
                    i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
                if !prev_is_word {
                    let word_end = sql[i..]
                        .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                        .map(|off| i + off)
                        .unwrap_or(len);
                    let word = &sql[i..word_end];
                    if word.eq_ignore_ascii_case("BEGIN") {
                        let after = sql[word_end..].trim_start();
                        let is_tx = after.starts_with(';')
                            || after.is_empty()
                            || after
                                .split_whitespace()
                                .next()
                                .map(|w| {
                                    matches!(
                                        w.trim_end_matches(';').to_uppercase().as_str(),
                                        "TRANSACTION"
                                            | "WORK"
                                            | "DEFERRED"
                                            | "IMMEDIATE"
                                            | "EXCLUSIVE"
                                    )
                                })
                                .unwrap_or(false);
                        if !is_tx {
                            begin_depth += 1;
                        }
                    } else if word.eq_ignore_ascii_case("END") && begin_depth > 0 {
                        begin_depth = begin_depth.saturating_sub(1);
                    }
                    i = word_end;
                    continue;
                }
            }
            b';' if paren_depth == 0 && begin_depth == 0 => {
                let slice = &sql[stmt_start..=i];
                if !slice.trim().is_empty() {
                    statements.push((stmt_start, i + 1, slice));
                }
                stmt_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }

    if stmt_start < len {
        let slice = &sql[stmt_start..len];
        if !slice.trim().is_empty() {
            statements.push((stmt_start, len, slice));
        }
    }
    statements
}

/// Parses a PostgreSQL dollar-quote delimiter (`$$` or `$tag$`) starting at the beginning of `s`.
pub fn parse_dollar_tag(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'$') {
        return None;
    }
    for (idx, &b) in bytes[1..].iter().enumerate() {
        if b == b'$' {
            return Some(&s[..=idx + 1]);
        }
        if !(b.is_ascii_alphanumeric() || b == b'_') {
            return None;
        }
    }
    None
}

/// Strips `--` line comments and `/* ... */` block comments outside of quoted strings.
pub fn strip_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < len {
        let c = bytes[i];
        let next = if i + 1 < len {
            Some(bytes[i + 1])
        } else {
            None
        };
        if !in_single && !in_double {
            if c == b'-' && next == Some(b'-') {
                i += 2;
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if c == b'/' && next == Some(b'*') {
                i += 2;
                while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(len);
                out.push(' ');
                continue;
            }
        }
        if c == b'\'' && !in_double {
            in_single = !in_single;
        } else if c == b'"' && !in_single {
            in_double = !in_double;
        }
        out.push(c as char);
        i += 1;
    }
    out
}
