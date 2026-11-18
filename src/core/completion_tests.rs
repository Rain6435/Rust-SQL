//! Tests for [`super`]: context-aware suggestions and alias resolution.

use super::*;

fn schema() -> CompletionSchema {
    CompletionSchema {
        tables: vec!["users".into(), "orders".into(), "user_roles".into()],
        columns: HashMap::from([
            (
                "users".to_string(),
                vec!["id".into(), "email".into(), "username".into()],
            ),
            (
                "orders".to_string(),
                vec!["id".into(), "user_id".into(), "total".into()],
            ),
        ]),
    }
}

fn labels(text: &str) -> Vec<String> {
    complete(text, text.chars().count(), &schema(), false)
        .items
        .into_iter()
        .map(|i| i.label)
        .collect()
}

#[test]
fn after_from_only_tables_are_offered() {
    assert_eq!(labels("SELECT * FROM us"), ["users", "user_roles"]);
}

#[test]
fn alias_dot_offers_that_tables_columns() {
    let text = "SELECT o.";
    let c = complete(
        &format!("{text} FROM orders o"),
        text.len(),
        &schema(),
        false,
    );
    let got: Vec<_> = c.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(got, ["id", "user_id", "total"]);
    assert_eq!(c.start, text.len());
}

#[test]
fn columns_of_referenced_tables_come_before_tables_and_keywords() {
    let text = "SELECT u";
    let full = format!("{text} FROM users JOIN orders AS o ON o.user_id = users.id");
    let c = complete(&full, text.len(), &schema(), false);
    let got: Vec<_> = c.items.iter().map(|i| (i.label.as_str(), i.kind)).collect();
    assert_eq!(got[0], ("username", CompletionKind::Column));
    assert!(got.contains(&("user_id", CompletionKind::Column)));
    assert!(got.contains(&("users", CompletionKind::Table)));
    assert!(got.contains(&("UNION", CompletionKind::Keyword)));
    assert!(got.contains(&("UPDATE", CompletionKind::Keyword)));
}

#[test]
fn nothing_inside_strings_or_comments_or_without_a_prefix() {
    assert!(labels("SELECT 'us").is_empty());
    assert!(labels("SELECT 1 -- us").is_empty());
    assert!(labels("SELECT ").is_empty());
    let explicit = complete("SELECT * FROM ", 14, &schema(), true);
    assert_eq!(explicit.items.len(), 3);
}

#[test]
fn aliases_and_qualified_names_are_resolved() {
    let refs = referenced_tables("select * from public.users u join orders on true");
    assert_eq!(
        refs,
        vec![
            ("users".to_string(), "u".to_string()),
            ("orders".to_string(), "orders".to_string()),
        ]
    );
}
