//! Interactive Terminal User Interface (`--tui`) powered by `ratatui` and `crossterm`.

pub mod host_key;
pub mod input_database;
pub mod input_servers;
pub mod params;
pub mod render_database;
pub mod render_servers;

use crate::gui::ServerFormDraft;
use crate::state::AppStore;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

/// Active screen route in the TUI application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiRoute {
    /// Main list of saved database servers.
    ServerList,
    /// Server creation (`None`) or editing (`Some(id)`) form.
    ServerSave(Option<String>),
    /// Server deletion confirmation dialog for `id`.
    ServerRemove(String),
    /// Connected database workspace view (`TableList`, `QueryArea`, `QueryResults`).
    Database {
        /// Connected server ID.
        server_id: String,
        /// Active database name.
        database: String,
    },
    /// Database switcher list on the active server.
    DatabaseList {
        /// Connected server ID.
        server_id: String,
        /// Current database name before switching.
        database: String,
    },
}

/// Active focus pane inside [`TuiRoute::Database`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseFocus {
    /// Left-hand table list sidebar.
    TableList,
    /// Top-right SQL query editor box.
    QueryArea,
    /// Parameter values listed under the SQL (only when the query has placeholders).
    Params,
    /// Bottom-right query results table.
    QueryResults,
}

/// State container for the interactive Terminal User Interface.
pub struct TuiApp {
    /// Core application state store and database facade.
    pub store: AppStore,
    /// Current navigation route.
    pub route: TuiRoute,
    /// Highlighted server index in [`TuiRoute::ServerList`].
    pub selected_server_idx: usize,
    /// Highlighted table index in [`TuiRoute::Database`].
    pub selected_table_idx: usize,
    /// Highlighted database index in [`TuiRoute::DatabaseList`].
    pub selected_db_idx: usize,
    /// Active focus pane in [`TuiRoute::Database`].
    pub db_focus: DatabaseFocus,
    /// Highlighted parameter index in [`DatabaseFocus::Params`].
    pub selected_param_idx: usize,
    /// Editable draft fields for [`TuiRoute::ServerSave`].
    pub form_draft: ServerFormDraft,
    /// Active field index (0..7) in [`TuiRoute::ServerSave`].
    pub form_field_idx: usize,
    /// Whether the user has requested to exit the TUI loop.
    pub should_quit: bool,
}

impl Default for TuiApp {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiApp {
    /// Creates a new [`TuiApp`] and loads persistent configuration from disk.
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
            selected_param_idx: 0,
            form_draft: ServerFormDraft::from_server(None),
            form_field_idx: 0,
            should_quit: false,
        }
    }
}

/// Enters alternate-screen raw mode and runs the interactive [`TuiApp`] event loop.
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
