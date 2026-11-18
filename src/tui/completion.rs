//! Autocomplete in the TUI SQL editor, from the shared completion engine
//! ([`crate::core::completion`]) and the metadata cache.
//!
//! Suggestions appear as you type (Live Auto Complete) or on Ctrl+Space. While they are shown,
//! Up/Down pick one, Tab inserts it, and Esc hides them; otherwise Tab switches panes as usual.
//! Both Settings toggles are honored. The TUI editor types at the end of the text, so the
//! cursor is always the end.

use crate::core::completion::CompletionKind;
use crate::tui::TuiApp;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem},
};
use tokio::runtime::Runtime;

/// Most suggestions visible at once in the popup.
const VISIBLE: usize = 6;

/// Whether `key` is Ctrl+Space (some terminals report it as a NUL key).
fn is_ctrl_space(key: &KeyEvent) -> bool {
    key.code == KeyCode::Null
        || (key.code == KeyCode::Char(' ') && key.modifiers.contains(KeyModifiers::CONTROL))
}

impl TuiApp {
    /// Recomputes suggestions for the end of the current query.
    pub(crate) fn refresh_completion(&mut self, rt: &Runtime, database: &str, explicit: bool) {
        let text = self
            .store
            .current_query()
            .map(|q| q.query.clone())
            .unwrap_or_default();
        rt.block_on(self.store.load_completion_columns(database, &text));
        let c = self
            .store
            .completions(database, &text, text.chars().count(), explicit);
        self.completion = (!c.items.is_empty()).then_some(c);
        self.completion_idx = 0;
    }

    /// Handles completion keys in the SQL editor. Returns whether the key was consumed.
    pub(crate) fn handle_completion_key(
        &mut self,
        key: KeyEvent,
        rt: &Runtime,
        database: &str,
    ) -> bool {
        if is_ctrl_space(&key) {
            self.refresh_completion(rt, database, true);
            return true;
        }
        let Some(c) = self.completion.as_ref() else {
            return false;
        };
        let len = c.items.len();
        match key.code {
            KeyCode::Up => self.completion_idx = self.completion_idx.saturating_sub(1),
            KeyCode::Down if self.completion_idx + 1 < len => self.completion_idx += 1,
            KeyCode::Down => {}
            KeyCode::Esc => self.completion = None,
            KeyCode::Tab => self.accept_completion(),
            _ => return false,
        }
        true
    }

    /// Inserts the highlighted suggestion in place of the partial word.
    pub fn accept_completion(&mut self) {
        let Some(c) = self.completion.take() else {
            return;
        };
        let Some(item) = c.items.get(self.completion_idx).or(c.items.first()) else {
            return;
        };
        let text = self
            .store
            .current_query()
            .map(|q| q.query.clone())
            .unwrap_or_default();
        let (new_text, _) = c.apply(&text, &item.label);
        self.store.update_query_if_needed(&new_text, None, None);
    }

    /// Draws the suggestions popup along the bottom edge of the query box `query_area`.
    pub(crate) fn render_completion(&self, f: &mut ratatui::Frame, query_area: Rect) {
        let Some(c) = self.completion.as_ref() else {
            return;
        };
        let first = self.completion_idx.saturating_sub(VISIBLE - 1);
        let items: Vec<ListItem> = c
            .items
            .iter()
            .enumerate()
            .skip(first)
            .take(VISIBLE)
            .map(|(i, item)| {
                let tag = match item.kind {
                    CompletionKind::Column => "col",
                    CompletionKind::Table => "table",
                    CompletionKind::Keyword => "kw",
                };
                let style = if i == self.completion_idx {
                    Style::default().fg(Color::Black).bg(Color::Yellow)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{:<28} {tag}", item.label)).style(style)
            })
            .collect();
        let screen = f.area();
        let height = (items.len() as u16 + 2).min(screen.height);
        let y = (query_area.y + query_area.height.saturating_sub(1))
            .min(screen.height.saturating_sub(height));
        let width = 40.min(screen.width.saturating_sub(query_area.x + 1));
        let area = Rect::new(query_area.x + 1, y, width, height);
        f.render_widget(Clear, area);
        let title = format!(
            " Suggestions {}/{} (Tab) ",
            self.completion_idx + 1,
            c.items.len()
        );
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(title)
                .title_style(Style::default().add_modifier(Modifier::BOLD)),
        );
        f.render_widget(list, area);
    }
}
