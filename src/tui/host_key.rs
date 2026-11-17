//! SSH bastion host-key approval prompt for the Terminal UI.

use crate::tui::{DatabaseFocus, TuiApp, TuiRoute};
use crossterm::event::{self, KeyCode};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use tokio::runtime::Runtime;

impl TuiApp {
    /// Connects to `server_id`, then opens its database view. Shows the error in the status bar.
    pub(crate) fn connect_and_open(&mut self, server_id: &str, rt: &Runtime) {
        match rt.block_on(self.store.connect(server_id, None, false, None)) {
            Ok(()) if self.store.connections.connected => {
                let db = self.store.current_database().unwrap_or_default();
                let _ = rt.block_on(self.store.fetch_databases_if_needed());
                let _ = rt.block_on(self.store.fetch_tables_if_needed(&db));
                self.selected_table_idx = 0;
                self.db_focus = DatabaseFocus::TableList;
                self.route = TuiRoute::Database {
                    server_id: server_id.to_string(),
                    database: db,
                };
            }
            Ok(()) => {}
            Err(e) => self.store.status = e,
        }
    }

    /// Handles `y` / `n` while a host-key prompt is open. Returns `true` if the key was consumed.
    pub(crate) fn handle_host_key_key(&mut self, key: event::KeyEvent, rt: &Runtime) -> bool {
        if self.store.connections.host_key_prompt.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => match self.store.approve_host_key() {
                Ok(Some(server_id)) => self.connect_and_open(&server_id, rt),
                Ok(None) => {}
                Err(e) => self.store.status = e,
            },
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.store.reject_host_key(),
            _ => {}
        }
        true
    }

    /// Draws the host-key prompt over `area` when one is pending.
    pub(crate) fn render_host_key_prompt(&self, f: &mut ratatui::Frame, area: Rect, footer: Rect) {
        let Some(ch) = self.store.connections.host_key_prompt.as_ref() else {
            return;
        };
        let text = format!(
            "\n  {}\n\n  Fingerprint: {}\n\n  Press [y] to trust this key and connect, or [n] / [Esc] to cancel.",
            ch.prompt_text(),
            ch.fingerprint_sha256
        );
        let msg = Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Verify SSH host key "),
        );
        f.render_widget(Clear, area);
        f.render_widget(msg, area);
        let shortcuts = Paragraph::new(" [y] Trust key  [n/Esc] Cancel").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        f.render_widget(Clear, footer);
        f.render_widget(shortcuts, footer);
    }
}
