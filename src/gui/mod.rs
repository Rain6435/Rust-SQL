//! Native cross-platform Desktop GUI powered by `eframe` / `egui`.

pub mod completion;
pub mod header_footer;
pub mod modal_host_keys;
pub mod modal_import;
pub mod modal_server;
pub mod modal_settings;
pub mod modals_files;
pub mod modals_tools;
pub mod query_editor;
pub mod query_results;
pub mod server_draft;
pub mod server_draft_convert;
pub mod server_management;
pub mod settings_draft;
pub mod sidebar;
pub mod sidebar_objects;

pub use server_draft::ServerFormDraft;
pub use settings_draft::SettingsFormDraft;

use crate::core::result_ops::{ColumnStats, SortDirection};
use crate::core::safety::QuerySafetyReport;
use crate::core::updater::UpdateCheckResult;
use crate::state::AppStore;
use eframe::egui;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Top-level navigation route inside the Desktop GUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveRoute {
    /// Server cards/list dashboard and connection manager.
    ServerManagement,
    /// Connected database explorer, multi-tab SQL editor, and result viewer for the given `server_id`.
    QueryBrowser(String),
}

/// Complete state of the `eframe` / `egui` native Desktop GUI application.
pub struct RustSqlGuiApp {
    /// Shared Tokio async runtime for executing database queries and SSH tunnels.
    pub rt: Arc<Runtime>,
    /// Central application state store and database facade.
    pub store: AppStore,
    /// Current screen route.
    pub route: ActiveRoute,
    /// Search filter text on the server management screen.
    pub server_filter: String,
    /// Editable draft state for the Server Add/Edit modal.
    pub server_draft: ServerFormDraft,
    /// Editable draft state for the Settings modal.
    pub settings_draft: SettingsFormDraft,
    /// Whether the server deletion confirmation dialog is open.
    pub confirm_delete_server: bool,
    /// Whether the left-hand database object sidebar is collapsed.
    pub sidebar_collapsed: bool,
    /// Search filter text for narrowing databases in the sidebar.
    pub db_search_filter: String,
    /// Per-database object search filters in the sidebar.
    pub db_obj_filters: HashMap<String, String>,
    /// Set of expanded database names in the sidebar tree.
    pub expanded_databases: HashSet<String>,
    /// Set of expanded `(database, table)` pairs in the sidebar tree.
    pub expanded_tables: HashSet<(String, String)>,
    /// Query tab ID currently being renamed inline, if any.
    pub renaming_tab_id: Option<u64>,
    /// Text buffer for inline tab renaming.
    pub rename_tab_buffer: String,
    /// Whether SQL editor text should wrap horizontally.
    pub wrap_contents: bool,
    /// Monospace font size in points for the SQL code editor.
    pub editor_font_size: f32,
    /// Cell value currently open in the content preview modal (`Plain Text` / `JSON`).
    pub preview_cell_value: Option<Value>,
    /// Active preview mode (`"plain"` or `"json"`).
    pub preview_mode: String,
    /// Whether the dialect query notes modal is open.
    pub show_client_info_modal: bool,
    /// Whether the keyboard shortcuts reference modal is open.
    pub show_shortcuts_modal: bool,
    /// Input buffer for interactive SSH private-key passphrase prompts.
    pub ssh_passphrase_input: String,
    /// Whether the trusted SSH host keys manager is open.
    pub show_host_keys_modal: bool,
    /// File path input buffer for Save/Open/Export dialogs.
    pub file_dialog_path: String,
    /// Whether the Save Query `.sql` modal is open.
    pub show_save_query_modal: bool,
    /// Whether the Open Query `.sql` modal is open.
    pub show_open_query_modal: bool,
    /// Active `(result_index, export_format)` tuple when the Export Result modal is open.
    pub show_export_file_modal: Option<(usize, String)>,
    /// Substring filter applied to result table rows.
    pub result_filter_text: String,
    /// Active `(column_name, SortDirection)` applied to result table rows.
    pub result_sort_col: Option<(String, SortDirection)>,
    /// Pending `(sql, QuerySafetyReport)` awaiting user confirmation for destructive queries.
    pub pending_safety_confirm: Option<(String, QuerySafetyReport)>,
    /// Whether the SQL Bookmarks & Execution Audit History modal is open.
    pub show_bookmarks_modal: bool,
    /// Input buffer for a new bookmark's title.
    pub bookmark_title_input: String,
    /// Input buffer for a new bookmark's comma-separated tags.
    pub bookmark_tags_input: String,
    /// Search filter inside the Bookmarks & History modal.
    pub bookmark_search_input: String,
    /// Computed column statistics displayed in the Column Stats modal.
    pub stats_modal_data: Option<Vec<ColumnStats>>,
    /// Generated Mermaid `erDiagram` displayed in the Schema ERD modal.
    pub erd_modal_diagram: Option<String>,
    /// Result of an interactive "Check for Updates" request, when the update modal is open.
    pub update_modal_state: Option<Result<UpdateCheckResult, String>>,
    /// Message displayed after an in-app self-update attempt.
    pub update_apply_message: Option<Result<String, String>>,
    /// Autocomplete suggestions shown under the SQL editor.
    pub completion: Option<crate::core::completion::Completions>,
    /// Data import modal state, while the modal is open.
    pub import_modal: Option<modal_import::ImportModalState>,
}

impl RustSqlGuiApp {
    /// Creates a new [`RustSqlGuiApp`] and loads persistent configuration from disk.
    pub fn new(rt: Arc<Runtime>) -> Self {
        let mut store = AppStore::new();
        let _ = store.load_config();
        let settings_draft = SettingsFormDraft::from_store(&store);
        Self {
            rt,
            store,
            route: ActiveRoute::ServerManagement,
            server_filter: String::new(),
            server_draft: ServerFormDraft::default(),
            settings_draft,
            confirm_delete_server: false,
            sidebar_collapsed: false,
            db_search_filter: String::new(),
            db_obj_filters: HashMap::new(),
            expanded_databases: HashSet::new(),
            expanded_tables: HashSet::new(),
            renaming_tab_id: None,
            rename_tab_buffer: String::new(),
            wrap_contents: false,
            editor_font_size: 13.0,
            preview_cell_value: None,
            preview_mode: "plain".to_string(),
            show_client_info_modal: false,
            show_shortcuts_modal: false,
            ssh_passphrase_input: String::new(),
            show_host_keys_modal: false,
            file_dialog_path: String::new(),
            show_save_query_modal: false,
            show_open_query_modal: false,
            show_export_file_modal: None,
            result_filter_text: String::new(),
            result_sort_col: None,
            pending_safety_confirm: None,
            show_bookmarks_modal: false,
            bookmark_title_input: String::new(),
            bookmark_tags_input: String::new(),
            bookmark_search_input: String::new(),
            stats_modal_data: None,
            erd_modal_diagram: None,
            update_modal_state: None,
            update_apply_message: None,
            import_modal: None,
            completion: None,
        }
    }

    fn render_modals(&mut self, ctx: &egui::Context) {
        self.render_server_modal(ctx);
        self.render_confirm_delete_modal(ctx);
        self.render_settings_modal(ctx);
        self.render_preview_modal(ctx);
        self.render_passphrase_and_info_modals(ctx);
        self.render_host_key_prompt(ctx);
        self.render_host_keys_manager(ctx);
        self.render_file_and_safety_modals(ctx);
        self.render_bookmarks_stats_erd_modals(ctx);
        self.render_import_modal(ctx);
    }
}

impl eframe::App for RustSqlGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(ref cfg) = self.store.config.data {
            if cfg.enabled_dark_theme {
                ctx.set_visuals(egui::Visuals::dark());
            } else {
                ctx.set_visuals(egui::Visuals::light());
            }
            ctx.set_pixels_per_point((cfg.zoom_factor as f32).clamp(0.4, 3.0));
        }

        self.handle_global_shortcuts(&ctx);

        egui::Panel::top("rust_sql_header")
            .exact_size(56.0)
            .show(ui, |ui| self.render_header(ui));

        egui::Panel::bottom("rust_sql_footer")
            .exact_size(28.0)
            .show(ui, |ui| self.render_footer(ui));

        egui::CentralPanel::default().show(ui, |ui| match self.route.clone() {
            ActiveRoute::ServerManagement => self.render_server_management(ui),
            ActiveRoute::QueryBrowser(ref server_id) => {
                self.render_query_browser(&ctx, ui, server_id)
            }
        });

        self.render_modals(&ctx);
    }
}

/// Launches the native cross-platform `eframe` Desktop GUI window.
pub fn run_native_gui() -> Result<(), String> {
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?,
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Rust-SQL")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 550.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Rust-SQL",
        options,
        Box::new(move |_cc| Ok(Box::new(RustSqlGuiApp::new(rt)))),
    )
    .map_err(|e| e.to_string())
}
