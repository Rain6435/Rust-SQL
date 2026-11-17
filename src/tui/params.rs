//! TUI parameter editor: lists the query's placeholders under the SQL and edits their typed
//! values (`Text`, `Number`, `Boolean`, `NULL`), which are bound through prepared statements.

use crate::core::params::{extract_query_parameters, ParamKind};
use crate::tui::{DatabaseFocus, TuiApp};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use tokio::runtime::Runtime;

impl TuiApp {
    /// Placeholder names in the active query, in order of first use.
    pub fn query_param_names(&self) -> Vec<String> {
        self.store
            .current_query()
            .map(|q| extract_query_parameters(&q.query))
            .unwrap_or_default()
    }

    /// Handles keys while [`DatabaseFocus::Params`] is focused.
    ///
    /// Up/Down pick a parameter, typing edits its value, Ctrl+T cycles its type, F5 or Ctrl+X
    /// executes, and Esc returns to the SQL editor.
    pub(crate) fn handle_params_key(&mut self, key: KeyEvent, rt: &Runtime) {
        let names = self.query_param_names();
        if names.is_empty() {
            self.db_focus = DatabaseFocus::QueryArea;
            return;
        }
        self.selected_param_idx = self.selected_param_idx.min(names.len() - 1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if (ctrl && key.code == KeyCode::Char('x')) || key.code == KeyCode::F(5) {
            if let Some(sql) = self.store.current_query().map(|q| q.query.clone()) {
                let _ = rt.block_on(self.store.execute_query_if_needed(&sql, false));
            }
            return;
        }
        let entry = self
            .store
            .query_params
            .entry(names[self.selected_param_idx].clone())
            .or_default();
        match key.code {
            KeyCode::Char('t') if ctrl => entry.kind = entry.kind.next(),
            KeyCode::Esc => self.db_focus = DatabaseFocus::QueryArea,
            KeyCode::Up => self.selected_param_idx = self.selected_param_idx.saturating_sub(1),
            KeyCode::Down if self.selected_param_idx + 1 < names.len() => {
                self.selected_param_idx += 1
            }
            KeyCode::Backspace => {
                entry.text.pop();
            }
            KeyCode::Char(c) if !ctrl => entry.text.push(c),
            _ => {}
        }
    }

    /// The SQL text followed by one line per parameter, for the query box.
    pub(crate) fn query_box_text(&self) -> Text<'static> {
        let query = self
            .store
            .current_query()
            .map(|q| q.query.clone())
            .unwrap_or_default();
        let mut lines: Vec<Line<'static>> =
            query.lines().map(|l| Line::from(l.to_string())).collect();
        let names = self.query_param_names();
        if names.is_empty() {
            return Text::from(lines);
        }
        lines.push(Line::from(
            "── Parameters (Tab to edit, C-t to change type) ──",
        ));
        for (idx, name) in names.iter().enumerate() {
            let input = self
                .store
                .query_params
                .get(name)
                .cloned()
                .unwrap_or_default();
            let value = match input.kind {
                ParamKind::Null => "NULL".to_string(),
                _ => input.text,
            };
            let selected = self.db_focus == DatabaseFocus::Params && idx == self.selected_param_idx;
            let marker = if selected { ">" } else { " " };
            let style = if selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::styled(
                format!("{marker} {name} [{}] = {value}", input.kind.label()),
                style,
            ));
        }
        Text::from(lines)
    }
}

#[cfg(test)]
mod tests {
    use crate::core::params::{ParamKind, ParamValue};
    use crate::tui::{DatabaseFocus, TuiApp};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn edits_typed_parameter_values() {
        let _cfg = crate::core::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let mut app = TuiApp::new();
        app.store.add_new_query("db", None);
        app.store
            .update_query_if_needed("SELECT :id, :name -- :ignored", None, None);
        app.db_focus = DatabaseFocus::Params;
        let key = |code, m| KeyEvent::new(code, m);
        for c in ['4', '2'] {
            app.handle_params_key(key(KeyCode::Char(c), KeyModifiers::NONE), &rt);
        }
        app.handle_params_key(key(KeyCode::Char('t'), KeyModifiers::CONTROL), &rt);
        app.handle_params_key(key(KeyCode::Down, KeyModifiers::NONE), &rt);
        app.handle_params_key(key(KeyCode::Char('x'), KeyModifiers::NONE), &rt);

        assert_eq!(app.query_param_names(), vec!["id", "name"]);
        assert_eq!(app.store.query_params["id"].kind, ParamKind::Number);
        let params = app.store.params_for_query("SELECT :id, :name").unwrap();
        assert_eq!(params["id"], ParamValue::Int(42));
        assert_eq!(params["name"], ParamValue::Text("x".into()));
        let shown = app.query_box_text().to_string();
        assert!(shown.contains("id [Number] = 42"), "{shown}");
        assert!(!shown.contains("ignored ["), "{shown}");
    }
}
