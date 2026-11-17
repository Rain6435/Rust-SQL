//! Keyboard input handling for the connected database workspace (`TableList`, `QueryArea`, `QueryResults`).

use crate::tui::{DatabaseFocus, TuiApp, TuiRoute};
use crossterm::event::{self, KeyCode, KeyModifiers};
use tokio::runtime::Runtime;

impl TuiApp {
    /// Handles keyboard navigation and SQL execution inside [`TuiRoute::Database`].
    pub(crate) fn handle_database_key(
        &mut self,
        key: event::KeyEvent,
        rt: &Runtime,
        server_id: String,
        database: String,
    ) {
        if key.code == KeyCode::Tab
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('e'))
        {
            self.db_focus = match self.db_focus {
                DatabaseFocus::TableList => DatabaseFocus::QueryArea,
                DatabaseFocus::QueryArea => DatabaseFocus::QueryResults,
                DatabaseFocus::QueryResults => DatabaseFocus::TableList,
            };
            return;
        }

        match self.db_focus {
            DatabaseFocus::TableList => match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Char('s') => {
                    rt.block_on(self.store.disconnect());
                    self.route = TuiRoute::ServerList;
                }
                KeyCode::Char('c') => {
                    let _ = rt.block_on(self.store.fetch_databases_if_needed());
                    self.selected_db_idx = 0;
                    self.route = TuiRoute::DatabaseList {
                        server_id,
                        database,
                    };
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected_table_idx = self.selected_table_idx.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = self
                        .store
                        .tables
                        .items_by_database
                        .get(&database)
                        .map(|v| v.len())
                        .unwrap_or(0);
                    if self.selected_table_idx + 1 < len {
                        self.selected_table_idx += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(tbl) = self
                        .store
                        .tables
                        .items_by_database
                        .get(&database)
                        .and_then(|v| v.get(self.selected_table_idx))
                        .cloned()
                    {
                        let client = self
                            .store
                            .connections
                            .server
                            .as_ref()
                            .map(|s| s.client.as_str())
                            .unwrap_or("");
                        let query = match client {
                            "postgresql" => format!("select * from \"{}\" limit 1000", tbl.name),
                            "mysql" => format!("select * from `{}` limit 1000", tbl.name),
                            _ => format!("select * from {} limit 1000", tbl.name),
                        };
                        self.store
                            .update_query_if_needed(&query, None, Some(&tbl.name));
                        let _ = rt.block_on(self.store.execute_query_if_needed(&query, true));
                    }
                }
                _ => {}
            },
            DatabaseFocus::QueryArea => {
                if (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('x'))
                    || key.code == KeyCode::F(5)
                {
                    if let Some(sql) = self.store.current_query().map(|q| q.query.clone()) {
                        let _ = rt.block_on(self.store.execute_query_if_needed(&sql, false));
                    }
                    return;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && (key.code == KeyCode::Char('u') || key.code == KeyCode::Char('c'))
                {
                    self.store.update_query_if_needed("", None, None);
                    return;
                }
                match key.code {
                    KeyCode::Esc => self.db_focus = DatabaseFocus::TableList,
                    KeyCode::Backspace => {
                        if let Some(q) = self.store.current_query_mut() {
                            q.query.pop();
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(q) = self.store.current_query_mut() {
                            q.query.push('\n');
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Some(q) = self.store.current_query_mut() {
                            q.query.push(c);
                        }
                    }
                    _ => {}
                }
            }
            DatabaseFocus::QueryResults => match key.code {
                KeyCode::Esc => self.db_focus = DatabaseFocus::TableList,
                KeyCode::Char('q') => self.should_quit = true,
                _ => {}
            },
        }
    }
}
