use crate::common::types::{Password, Server, SslConfig};
use crate::common::utils::value_to_string;
use crate::gui::ServerFormDraft;
use crate::state::AppStore;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table},
    Terminal,
};
use serde_json::Value;
use std::io;
use std::time::Duration;
use tokio::runtime::Runtime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiRoute {
    ServerList,
    ServerSave(Option<String>),
    ServerRemove(String),
    Database { server_id: String, database: String },
    DatabaseList { server_id: String, database: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseFocus {
    TableList,
    QueryArea,
    QueryResults,
}

pub struct TuiApp {
    pub store: AppStore,
    pub route: TuiRoute,
    pub selected_server_idx: usize,
    pub selected_table_idx: usize,
    pub selected_db_idx: usize,
    pub db_focus: DatabaseFocus,
    pub form_draft: ServerFormDraft,
    pub form_field_idx: usize,
    pub should_quit: bool,
}

impl TuiApp {
    pub fn new() -> Self {
        let mut store = AppStore::new();
        let _ = store.load_config();
        Self {
            store,
            route: TuiRoute::ServerList,
            selected_server_idx: 0,
            selected_table_idx: 0,
            selected_db_idx: 0,
            db_focus: DatabaseFocus::TableList,
            form_draft: ServerFormDraft::from_server(None),
            form_field_idx: 0,
            should_quit: false,
        }
    }

    pub fn render(&self, f: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(5),
                Constraint::Length(2),
            ])
            .split(f.area());

        // Top Title / Status bar
        let status_text = if !self.store.status.is_empty() {
            format!(" Rust-SQL (sqlectron-term) | Status: {}", self.store.status)
        } else {
            " Rust-SQL (sqlectron-term)".to_string()
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

    fn render_server_list(&self, f: &mut ratatui::Frame, area: Rect, footer: Rect) {
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

    fn render_server_save(&self, f: &mut ratatui::Frame, area: Rect, footer: Rect) {
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

    fn render_server_remove(&self, f: &mut ratatui::Frame, area: Rect, footer: Rect, id: &str) {
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

    fn render_database(&self, f: &mut ratatui::Frame, area: Rect, footer: Rect, database: &str) {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(30), Constraint::Min(20)])
            .split(area);

        let r_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(5)])
            .split(h_chunks[1]);

        // 1. TableList box (left)
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

        // 2. Query Textarea box (top-right)
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

        // 3. QueryResults box (bottom-right)
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

        // Context-sensitive shortcuts bar matching `sqlectron-term/src/containers/database.js`
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

    fn render_database_list(&self, f: &mut ratatui::Frame, area: Rect, footer: Rect) {
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

    pub fn handle_key(&mut self, key: event::KeyEvent, rt: &Runtime) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            self.should_quit = true;
            return;
        }

        match self.route.clone() {
            TuiRoute::ServerList => match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected_server_idx = self.selected_server_idx.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.selected_server_idx + 1 < self.store.servers.items.len() {
                        self.selected_server_idx += 1;
                    }
                }
                KeyCode::Char('a') => {
                    self.form_draft = ServerFormDraft::from_server(None);
                    self.form_field_idx = 0;
                    self.route = TuiRoute::ServerSave(None);
                }
                KeyCode::Char('e') => {
                    if let Some(srv) = self
                        .store
                        .servers
                        .items
                        .get(self.selected_server_idx)
                        .cloned()
                    {
                        let _ = self.store.start_editing_server(Some(&srv.id));
                        self.form_draft = ServerFormDraft::from_server(
                            self.store.servers.editing_server.as_ref(),
                        );
                        self.form_field_idx = 0;
                        self.route = TuiRoute::ServerSave(Some(srv.id));
                    }
                }
                KeyCode::Char('d') | KeyCode::Delete => {
                    if let Some(srv) = self.store.servers.items.get(self.selected_server_idx) {
                        self.route = TuiRoute::ServerRemove(srv.id.clone());
                    }
                }
                KeyCode::Enter => {
                    if let Some(srv) = self
                        .store
                        .servers
                        .items
                        .get(self.selected_server_idx)
                        .cloned()
                    {
                        if rt
                            .block_on(self.store.connect(&srv.id, None, false, None))
                            .is_ok()
                        {
                            let db = self.store.current_database().unwrap_or_default();
                            let _ = rt.block_on(self.store.fetch_databases_if_needed());
                            let _ = rt.block_on(self.store.fetch_tables_if_needed(&db));
                            self.selected_table_idx = 0;
                            self.db_focus = DatabaseFocus::TableList;
                            self.route = TuiRoute::Database {
                                server_id: srv.id,
                                database: db,
                            };
                        }
                    }
                }
                _ => {}
            },
            TuiRoute::ServerSave(edit_id) => match key.code {
                KeyCode::Esc => {
                    self.store.finish_editing_server();
                    self.route = TuiRoute::ServerList;
                }
                KeyCode::Tab | KeyCode::Down => {
                    self.form_field_idx = (self.form_field_idx + 1) % 7;
                }
                KeyCode::Up => {
                    self.form_field_idx = (self.form_field_idx + 6) % 7;
                }
                KeyCode::Enter => {
                    let srv = Server {
                        id: edit_id.clone().unwrap_or_default(),
                        name: self.form_draft.name.clone(),
                        client: self.form_draft.client.clone(),
                        host: if self.form_draft.host.is_empty() {
                            None
                        } else {
                            Some(self.form_draft.host.clone())
                        },
                        port: self.form_draft.port.parse::<u32>().ok(),
                        user: if self.form_draft.user.is_empty() {
                            None
                        } else {
                            Some(self.form_draft.user.clone())
                        },
                        password: if self.form_draft.password.is_empty() {
                            None
                        } else {
                            Some(Password::Plain(self.form_draft.password.clone()))
                        },
                        database: if self.form_draft.database.is_empty() {
                            None
                        } else {
                            Some(self.form_draft.database.clone())
                        },
                        ssl: Some(SslConfig::Bool(false)),
                        ..Default::default()
                    };
                    if self.store.save_server(srv, edit_id.as_deref()).is_ok() {
                        self.route = TuiRoute::ServerList;
                    }
                }
                KeyCode::Backspace => {
                    let target = match self.form_field_idx {
                        0 => &mut self.form_draft.name,
                        1 => &mut self.form_draft.client,
                        2 => &mut self.form_draft.host,
                        3 => &mut self.form_draft.port,
                        4 => &mut self.form_draft.user,
                        5 => &mut self.form_draft.password,
                        _ => &mut self.form_draft.database,
                    };
                    target.pop();
                }
                KeyCode::Char(c) => {
                    let target = match self.form_field_idx {
                        0 => &mut self.form_draft.name,
                        1 => &mut self.form_draft.client,
                        2 => &mut self.form_draft.host,
                        3 => &mut self.form_draft.port,
                        4 => &mut self.form_draft.user,
                        5 => &mut self.form_draft.password,
                        _ => &mut self.form_draft.database,
                    };
                    target.push(c);
                }
                _ => {}
            },
            TuiRoute::ServerRemove(id) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let _ = self.store.remove_server(&id);
                    self.selected_server_idx = 0;
                    self.route = TuiRoute::ServerList;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.route = TuiRoute::ServerList;
                }
                _ => {}
            },
            TuiRoute::Database {
                server_id,
                database,
            } => {
                if key.code == KeyCode::Tab
                    || (key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('e'))
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
                                    "postgresql" => {
                                        format!("select * from \"{}\" limit 1000", tbl.name)
                                    }
                                    "mysql" => format!("select * from `{}` limit 1000", tbl.name),
                                    _ => format!("select * from {} limit 1000", tbl.name),
                                };
                                self.store
                                    .update_query_if_needed(&query, None, Some(&tbl.name));
                                let _ =
                                    rt.block_on(self.store.execute_query_if_needed(&query, true));
                            }
                        }
                        _ => {}
                    },
                    DatabaseFocus::QueryArea => {
                        if (key.modifiers.contains(KeyModifiers::CONTROL)
                            && key.code == KeyCode::Char('x'))
                            || key.code == KeyCode::F(5)
                        {
                            if let Some(sql) = self.store.current_query().map(|q| q.query.clone()) {
                                let _ =
                                    rt.block_on(self.store.execute_query_if_needed(&sql, false));
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
            TuiRoute::DatabaseList {
                server_id,
                database,
            } => match key.code {
                KeyCode::Esc => {
                    self.route = TuiRoute::Database {
                        server_id,
                        database,
                    };
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected_db_idx = self.selected_db_idx.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.selected_db_idx + 1 < self.store.databases.items.len() {
                        self.selected_db_idx += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(db_item) = self
                        .store
                        .databases
                        .items
                        .get(self.selected_db_idx)
                        .cloned()
                    {
                        let _ = rt.block_on(self.store.connect(
                            &server_id,
                            Some(&db_item.name),
                            false,
                            None,
                        ));
                        let _ = rt.block_on(self.store.fetch_tables_if_needed(&db_item.name));
                        self.selected_table_idx = 0;
                        self.route = TuiRoute::Database {
                            server_id,
                            database: db_item.name,
                        };
                    }
                }
                _ => {}
            },
        }
    }
}

pub fn run_tui() -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;

    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let mut app = TuiApp::new();

    loop {
        terminal
            .draw(|f| app.render(f))
            .map_err(|e| e.to_string())?;
        if event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
                app.handle_key(key, &rt);
            }
        }
        if app.should_quit {
            break;
        }
    }

    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;
    Ok(())
}
