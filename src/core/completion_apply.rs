//! Inserting a chosen suggestion: the partial word is replaced by the label.
//!
//! Shared by the desktop, terminal, and web editors so they all edit text the same way.

use super::Completions;

impl Completions {
    /// `text` with the partial word replaced by `label`, and the char index just after it.
    pub fn apply(&self, text: &str, label: &str) -> (String, usize) {
        let chars: Vec<char> = text.chars().collect();
        let end = (self.start + self.prefix.chars().count()).min(chars.len());
        let start = self.start.min(end);
        let mut out: String = chars[..start].iter().collect();
        out.push_str(label);
        out.extend(&chars[end..]);
        (out, start + label.chars().count())
    }
}

#[cfg(test)]
mod tests {
    use crate::core::completion::{complete, CompletionSchema};

    #[test]
    fn replaces_the_partial_word_and_keeps_the_rest() {
        let schema = CompletionSchema {
            tables: vec!["customers".into()],
            ..Default::default()
        };
        let text = "SELECT * FROM cu WHERE é = 1";
        let c = complete(text, 16, &schema, false);
        let (out, cursor) = c.apply(text, "customers");
        assert_eq!(out, "SELECT * FROM customers WHERE é = 1");
        assert_eq!(cursor, 23);
        // A label after a multi-byte character lands on char, not byte, boundaries.
        let text = "SELECT ééé, cu";
        let c = complete(text, text.chars().count(), &schema, false);
        assert_eq!(c.apply(text, "customers").0, "SELECT ééé, customers");
    }
}
