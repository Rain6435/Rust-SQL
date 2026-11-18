//! Placeholder scanning shared by parameter extraction, prepared-statement rewriting, and display.
//!
//! One scanner decides where placeholders are, so the parameters the editor shows, the SQL sent
//! to the driver, and the display-only text in history all agree. Placeholders inside string
//! literals, quoted identifiers, `--` comments, and `/* */` comments are ignored, as are
//! PostgreSQL `::type` casts and `@@system` variables.

/// One placeholder occurrence in a SQL string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholder {
    /// Byte offset of the sigil (`:`, `@`, or `$`).
    pub start: usize,
    /// Byte offset just past the placeholder.
    pub end: usize,
    /// Parameter name: `name` for `:name` / `@name`, `$1` for positional `$1`.
    pub name: String,
}

impl Placeholder {
    /// The placeholder exactly as written in the SQL text.
    pub fn token<'a>(&self, sql: &'a str) -> &'a str {
        &sql[self.start..self.end]
    }
}

/// Lists every placeholder in `sql`, in order, outside literals and comments.
pub fn scan_placeholders(sql: &str) -> Vec<Placeholder> {
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    let mut quote: Option<u8> = None;
    let mut i = 0;

    while i < len {
        let c = bytes[i];
        let next = bytes.get(i + 1).copied();

        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match (c, next) {
            (b'-', Some(b'-')) => {
                i = sql[i..].find('\n').map(|p| i + p + 1).unwrap_or(len);
                continue;
            }
            (b'/', Some(b'*')) => {
                i = sql[i + 2..]
                    .find("*/")
                    .map(|p| i + 2 + p + 2)
                    .unwrap_or(len);
                continue;
            }
            (b'\'' | b'"' | b'`', _) => {
                quote = Some(c);
                i += 1;
                continue;
            }
            (b':', Some(b':')) | (b'@', Some(b'@')) => {
                i += 2;
                continue;
            }
            (b':' | b'@', Some(n)) if n.is_ascii_alphabetic() || n == b'_' => {
                let end = word_end(bytes, i + 1, |b| b.is_ascii_alphanumeric() || b == b'_');
                out.push(Placeholder {
                    start: i,
                    end,
                    name: sql[i + 1..end].to_string(),
                });
                i = end;
                continue;
            }
            (b'$', Some(n)) if n.is_ascii_digit() => {
                let end = word_end(bytes, i + 1, |b| b.is_ascii_digit());
                out.push(Placeholder {
                    start: i,
                    end,
                    name: sql[i..end].to_string(),
                });
                i = end;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn word_end(bytes: &[u8], mut end: usize, keep: impl Fn(u8) -> bool) -> usize {
    while end < bytes.len() && keep(bytes[end]) {
        end += 1;
    }
    end
}

/// Extracts all unique named (`:param_name`, `@param_name`) and positional (`$1`) parameters
/// outside of string literals and comments, skipping PostgreSQL `::type` casts and `@@system` variables.
pub fn extract_query_parameters(sql: &str) -> Vec<String> {
    let mut params: Vec<String> = Vec::new();
    for p in scan_placeholders(sql) {
        if !params.contains(&p.name) {
            params.push(p.name);
        }
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_named_positional_and_skips_casts() {
        let sql = "SELECT * FROM users WHERE username = :username AND role_id = $1 AND active = @active AND createdat::date > '2020-01-01' AND @@version IS NOT NULL";
        assert_eq!(
            extract_query_parameters(sql),
            vec!["username", "$1", "active"]
        );
    }

    #[test]
    fn ignores_literals_identifiers_and_comments() {
        let sql = "SELECT ':a', \"@b\", `:c` -- :d\n/* :e $1 */ FROM t WHERE x = :f";
        assert_eq!(extract_query_parameters(sql), vec!["f"]);
    }

    #[test]
    fn unterminated_comment_hides_the_rest() {
        assert!(extract_query_parameters("SELECT 1 /* :a").is_empty());
        assert_eq!(extract_query_parameters("SELECT :a -- :b"), vec!["a"]);
    }

    #[test]
    fn placeholder_offsets_cover_the_token() {
        let sql = "SELECT :id, $12";
        let found = scan_placeholders(sql);
        assert_eq!(found[0].token(sql), ":id");
        assert_eq!(found[1].token(sql), "$12");
        assert_eq!(found[1].name, "$12");
    }
}
