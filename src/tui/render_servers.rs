//! TUI rendering for the top bar, server list, server add/edit form, server removal prompt,
//! and database selection list.

use crate::tui::{TuiApp, TuiRoute};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

impl TuiApp {
    /// Renders the complete TUI frame for the active [`TuiRoute`].
    pub fn render(&self, f: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(5),
                Constraint::Length(2),
            ])
            .split(f.area());

        let status_text = if !self.store.status.is_empty() {
            format!(" Rust-SQL | Status: {}", self.store.status)
        } else {
            " Rust-SQL".to_string()
        };
        let top_bar = Paragraph::new(status_text).style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        f.render_widget(top_bar, chunks[0]);

        match &self.route {
            TuiRoute::ServerList => self.render_server_list(f, chunks[1], chunks[2]),
            TuiRoute::ServerSave(_) => self.render_server_save(f, chunks[1], chunks[2]),
            TuiRoute::ServerRemove(id) => self.render_server_remove(f, chunks[1], chunks[2], id),
            TuiRoute::Database { database, .. } => {
                self.render_database(f, chunks[1], chunks[2], database)
            }
            TuiRoute::DatabaseList { .. } => self.render_database_list(f, chunks[1], chunks[2]),
        }
    }

    pub(crate) fn render_server_list(&self, f: &mut ratatui::Frame, area: Rect, footer: Rect) {
        let items: Vec<ListItem> = self
            .store
            .servers
            .items
            .iter()
            .enumerate()
            .map(|(i, srv)| {
                let prefix = if i == self.selected_server_idx {
                    "▶ "
                } else {
                    "  "
                };
                let host = srv
                    .host
                    .as_deref()
                    .or(srv.database.as_deref())
                    .unwrap_or("localhost");
                let line = format!("{prefix}{} [{}] ({})", srv.name, srv.client, host);
                let style = if i == self.selected_server_idx {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(line).style(style)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Servers (/server/list) "),
        );
        f.render_widget(list, area);

        let shortcuts = Paragraph::new(
            " [Enter] Connect  [a] Add Server  [e] Edit Server  [d] Remove Server  [q] Quit",
        )
        .style(Style::default().fg(Color::Cyan));
        f.render_widget(shortcuts, footer);
    }

    pub(crate) fn render_server_save(&self, f: &mut ratatui::Frame, area: Rect, footer: Rect) {
        let fields = [
            ("Name", self.form_draft.name.as_str()),
            (
                "Client (postgresql/mysql/sqlite/sqlserver/cassandra)",
                self.form_draft.client.as_str(),
            ),
            ("Host", self.form_draft.host.as_str()),
            ("Port", self.form_draft.port.as_str()),
            ("User", self.form_draft.user.as_str()),
            ("Password", self.form_draft.password.as_str()),
            ("Database", self.form_draft.database.as_str()),
        ];

        let lines: Vec<Line> = fields
            .iter()
            .enumerate()
            .map(|(idx, (label, val))| {
                let active = idx == self.form_field_idx;
                let style = if active {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let marker = if active { "▶ " } else { "  " };
                Line::from(vec![
                    Span::styled(format!("{marker}{label}: "), style),
                    Span::styled((*val).to_string(), style),
                ])
            })
            .collect();

        let block = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Save Server (/server/add) "),
        );
        f.render_widget(block, area);

        let shortcuts = Paragraph::new(" [Tab/↑/↓] Next/Prev Field  [Enter] Save  [Esc] Cancel")
            .style(Style::default().fg(Color::Cyan));
        f.render_widget(shortcuts, footer);
    }

    pub(crate) fn render_server_remove(
        &self,
        f: &mut ratatui::Frame,
        area: Rect,
        footer: Rect,
        id: &str,
    ) {
        let name = self
            .store
            .servers
            .items
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.as_str())
            .unwrap_or(id);
        let msg = Paragraph::new(format!(
            "\n  Are you sure you want to remove server '{name}'?\n\n  Press [y] to confirm or [n] / [Esc] to cancel."
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Remove Server "),
        );
        f.render_widget(Clear, area);
        f.render_widget(msg, area);

        let shortcuts = Paragraph::new(" [y] Confirm Remove  [n/Esc] Cancel")
            .style(Style::default().fg(Color::Red));
        f.render_widget(shortcuts, footer);
    }

    pub(crate) fn render_database_list(&self, f: &mut ratatui::Frame, area: Rect, footer: Rect) {
        let items: Vec<ListItem> = self
            .store
            .databases
            .items
            .iter()
            .enumerate()
            .map(|(i, db)| {
                let prefix = if i == self.selected_db_idx {
                    "▶ "
                } else {
                    "  "
                };
                let style = if i == self.selected_db_idx {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{prefix}{}", db.name)).style(style)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Database "),
        );
        f.render_widget(list, area);

        let shortcuts = Paragraph::new(" [Enter] Select Database  [Esc] Cancel")
            .style(Style::default().fg(Color::Cyan));
        f.render_widget(shortcuts, footer);
    }
}
