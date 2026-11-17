//! TUI rendering for the connected database workspace (`TableList`, `QueryArea`, `QueryResults`).

use crate::common::utils::value_to_string;
use crate::tui::{DatabaseFocus, TuiApp};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table},
};
use serde_json::Value;

impl TuiApp {
    /// Renders the 3-pane database workspace view (tables list, SQL query box, and results grid).
    pub(crate) fn render_database(
        &self,
        f: &mut ratatui::Frame,
        area: Rect,
        footer: Rect,
        database: &str,
    ) {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(30), Constraint::Min(20)])
            .split(area);

        let r_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(5)])
            .split(h_chunks[1]);

        let tables = self
            .store
            .tables
            .items_by_database
            .get(database)
            .cloned()
            .unwrap_or_default();
        let table_items: Vec<ListItem> = tables
            .iter()
            .enumerate()
            .map(|(idx, t)| {
                let prefix = if idx == self.selected_table_idx {
                    "▶ "
                } else {
                    "  "
                };
                let style = if idx == self.selected_table_idx {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{prefix}{}", t.name)).style(style)
            })
            .collect();

        let tbl_border_style = if self.db_focus == DatabaseFocus::TableList {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let tbl_list = List::new(table_items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(tbl_border_style)
                .title(format!(" Tables ({database}) ")),
        );
        f.render_widget(tbl_list, h_chunks[0]);

        let query_str = self
            .store
            .current_query()
            .map(|q| q.query.as_str())
            .unwrap_or("");
        let query_border_style = if self.db_focus == DatabaseFocus::QueryArea {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let query_box = Paragraph::new(query_str).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(query_border_style)
                .title(" Query "),
        );
        f.render_widget(query_box, r_chunks[0]);

        let res_border_style = if self.db_focus == DatabaseFocus::QueryResults {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let results_block = Block::default()
            .borders(Borders::ALL)
            .border_style(res_border_style)
            .title(" Query Results ");

        if let Some(q) = self.store.current_query() {
            if let Some(ref err) = q.error {
                let err_p = Paragraph::new(err.as_str())
                    .style(Style::default().fg(Color::Red))
                    .block(results_block);
                f.render_widget(err_p, r_chunks[1]);
            } else if let Some(Some(first)) = q.results.as_ref().map(|r| r.first()) {
                if !first.fields.is_empty() {
                    let header_cells = first.fields.iter().map(|fld| {
                        Cell::from(fld.name.clone()).style(
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        )
                    });
                    let header = Row::new(header_cells).height(1);
                    let rows = first.rows.iter().map(|row_map| {
                        let cells = first.fields.iter().map(|fld| {
                            let val = row_map.get(&fld.name).unwrap_or(&Value::Null);
                            Cell::from(value_to_string(val))
                        });
                        Row::new(cells).height(1)
                    });
                    let widths: Vec<Constraint> =
                        first.fields.iter().map(|_| Constraint::Min(14)).collect();
                    let table = Table::new(rows, widths).header(header).block(results_block);
                    f.render_widget(table, r_chunks[1]);
                } else {
                    let msg = Paragraph::new(format!(
                        "Command {} executed. Affected rows: {}",
                        first.command,
                        first.affected_rows.unwrap_or(0)
                    ))
                    .block(results_block);
                    f.render_widget(msg, r_chunks[1]);
                }
            } else {
                f.render_widget(
                    Paragraph::new("No query executed yet.").block(results_block),
                    r_chunks[1],
                );
            }
        } else {
            f.render_widget(Paragraph::new("").block(results_block), r_chunks[1]);
        }

        let shortcut_text = match self.db_focus {
            DatabaseFocus::TableList => {
                " [Enter] Select  [c] Change database  [s] List servers  [Tab/C-e] Editor  [q] Quit"
            }
            DatabaseFocus::QueryArea => {
                " [C-x/F5] Execute  [C-u] Clear  [Tab/C-e] Switch Focus  [Esc] Table List"
            }
            DatabaseFocus::QueryResults => " [Tab/C-e] Editor  [Esc] Table List  [q] Quit",
        };
        f.render_widget(
            Paragraph::new(shortcut_text).style(Style::default().fg(Color::Cyan)),
            footer,
        );
    }
}
