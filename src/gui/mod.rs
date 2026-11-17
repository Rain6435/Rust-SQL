use crate::adapters::CLIENTS;
use crate::common::types::{
    BaseConfig, FilterClause, FilterList, LogOptions, Password, Server, ServerFilter, SshConfig,
    SslConfig,
};
use crate::common::utils::{escape_regexp_string, value_to_string};
use crate::core::connection_string::{build_connection_uri, parse_connection_uri};
use crate::core::export::write_clipboard_text;
use crate::core::params::extract_query_parameters;
use crate::core::result_ops::{
    compute_column_stats, filter_result_rows, sort_result_rows, ColumnStats, SortDirection,
};
use crate::core::safety::QuerySafetyReport;
use crate::core::sql_formatter::format_sql;
use crate::state::{AppStore, DbObjectType, ScriptActionType};
use eframe::egui::{self, Color32, FontId, Key, KeyboardShortcut, Modifiers, RichText, Stroke};
use egui_extras::{Column, TableBuilder};
use regex::RegexBuilder;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::runtime::Runtime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveRoute {
    ServerManagement,
    QueryBrowser(String),
}

#[derive(Debug, Clone, Default)]
pub struct ServerFormDraft {
    pub id: String,
    pub name: String,
    pub client: String,
    pub ssl: bool,
    pub host: String,
    pub port: String,
    pub socket_path: String,
    pub domain: String,
    pub user: String,
    pub password: String,
    pub show_plain_password: bool,
    pub database: String,
    pub schema: String,
    pub conn_uri: String,
    pub ssh_enabled: bool,
    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_user: String,
    pub ssh_password: String,
    pub ssh_private_key: String,
    pub ssh_private_key_with_passphrase: bool,
    pub ssh_use_agent: bool,
    pub filter_enabled: bool,
    pub filter_db_only: String,
    pub filter_db_ignore: String,
    pub filter_schema_only: String,
    pub filter_schema_ignore: String,
}

impl ServerFormDraft {
    pub fn from_server(srv: Option<&Server>) -> Self {
        let Some(s) = srv else {
            return Self {
                client: "postgresql".to_string(),
                port: "5432".to_string(),
                ssh_port: "22".to_string(),
                ..Default::default()
            };
        };

        let (db_only, db_ignore, schema_only, schema_ignore, filter_enabled) =
            if let Some(ref f) = s.filter {
                let (dbo, dbi) = match &f.database {
                    Some(FilterClause::List(l)) => (
                        l.only.as_ref().map(|v| v.join("\n")).unwrap_or_default(),
                        l.ignore.as_ref().map(|v| v.join("\n")).unwrap_or_default(),
                    ),
                    Some(FilterClause::Exact(e)) => (e.clone(), String::new()),
                    None => (String::new(), String::new()),
                };
                let (so, si) = match &f.schema {
                    Some(FilterClause::List(l)) => (
                        l.only.as_ref().map(|v| v.join("\n")).unwrap_or_default(),
                        l.ignore.as_ref().map(|v| v.join("\n")).unwrap_or_default(),
                    ),
                    Some(FilterClause::Exact(e)) => (e.clone(), String::new()),
                    None => (String::new(), String::new()),
                };
                (dbo, dbi, so, si, true)
            } else {
                (
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    false,
                )
            };

        let (
            ssh_enabled,
            ssh_host,
            ssh_port,
            ssh_user,
            ssh_password,
            ssh_private_key,
            ssh_passphrase_flag,
            ssh_use_agent,
        ) = if let Some(ref ssh) = s.ssh {
            (
                true,
                ssh.host.clone(),
                ssh.port.to_string(),
                ssh.user.clone(),
                ssh.password
                    .as_ref()
                    .and_then(|p| p.as_plain())
                    .unwrap_or("")
                    .to_string(),
                ssh.private_key.clone().unwrap_or_default(),
                ssh.private_key_with_passphrase.unwrap_or(false),
                ssh.use_agent.unwrap_or(false),
            )
        } else {
            (
                false,
                String::new(),
                "22".to_string(),
                String::new(),
                String::new(),
                String::new(),
                false,
                false,
            )
        };

        let mut draft = Self {
            id: s.id.clone(),
            name: s.name.clone(),
            client: s.client.clone(),
            ssl: s.ssl.as_ref().map(|ssl| ssl.is_enabled()).unwrap_or(false),
            host: s.host.clone().unwrap_or_default(),
            port: s.port.map(|p| p.to_string()).unwrap_or_default(),
            socket_path: s.socket_path.clone().unwrap_or_default(),
            domain: s.domain.clone().unwrap_or_default(),
            user: s.user.clone().unwrap_or_default(),
            password: s
                .password
                .as_ref()
                .and_then(|p| p.as_plain())
                .unwrap_or("")
                .to_string(),
            show_plain_password: false,
            database: s.database.clone().unwrap_or_default(),
            schema: s.schema.clone().unwrap_or_default(),
            conn_uri: String::new(),
            ssh_enabled,
            ssh_host,
            ssh_port,
            ssh_user,
            ssh_password,
            ssh_private_key,
            ssh_private_key_with_passphrase: ssh_passphrase_flag,
            ssh_use_agent,
            filter_enabled,
            filter_db_only: db_only,
            filter_db_ignore: db_ignore,
            filter_schema_only: schema_only,
            filter_schema_ignore: schema_ignore,
        };
        draft.conn_uri = build_connection_uri(&draft.to_server(), false);
        draft
    }

    pub fn to_server(&self) -> Server {
        let disabled: HashSet<String> = CLIENTS
            .iter()
            .find(|c| c.key == self.client)
            .map(|c| c.disabled_features.iter().cloned().collect())
            .unwrap_or_default();

        let opt_str = |s: &str, feat: &str| -> Option<String> {
            if disabled.contains(feat) {
                return None;
            }
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        };

        let port = if disabled.contains("server:port") || !self.socket_path.trim().is_empty() {
            None
        } else {
            self.port.trim().parse::<u32>().ok().or_else(|| {
                if !self.host.trim().is_empty() {
                    CLIENTS
                        .iter()
                        .find(|c| c.key == self.client)
                        .and_then(|c| c.default_port)
                } else {
                    None
                }
            })
        };

        let ssh = if self.ssh_enabled && !disabled.contains("server:ssh") {
            Some(SshConfig {
                host: self.ssh_host.trim().to_string(),
                port: self.ssh_port.trim().parse::<u32>().unwrap_or(22),
                user: self.ssh_user.trim().to_string(),
                password: if self.ssh_password.trim().is_empty()
                    || !self.ssh_private_key.trim().is_empty()
                    || self.ssh_use_agent
                {
                    None
                } else {
                    Some(Password::Plain(self.ssh_password.trim().to_string()))
                },
                passphrase: None,
                private_key: if self.ssh_private_key.trim().is_empty() || self.ssh_use_agent {
                    None
                } else {
                    Some(self.ssh_private_key.trim().to_string())
                },
                private_key_with_passphrase: Some(self.ssh_private_key_with_passphrase),
                use_agent: Some(self.ssh_use_agent),
            })
        } else {
            None
        };

        let parse_lines = |s: &str| -> Option<Vec<String>> {
            let v: Vec<String> = s
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        };

        let filter = if self.filter_enabled {
            let db_only = parse_lines(&self.filter_db_only);
            let db_ignore = parse_lines(&self.filter_db_ignore);
            let sc_only = parse_lines(&self.filter_schema_only);
            let sc_ignore = parse_lines(&self.filter_schema_ignore);
            Some(ServerFilter {
                database: if db_only.is_some() || db_ignore.is_some() {
                    Some(FilterClause::List(FilterList {
                        only: db_only,
                        ignore: db_ignore,
                    }))
                } else {
                    None
                },
                schema: if sc_only.is_some() || sc_ignore.is_some() {
                    Some(FilterClause::List(FilterList {
                        only: sc_only,
                        ignore: sc_ignore,
                    }))
                } else {
                    None
                },
            })
        } else {
            None
        };

        Server {
            id: self.id.clone(),
            name: self.name.trim().to_string(),
            client: self.client.trim().to_string(),
            adapter: None,
            host: if !self.socket_path.trim().is_empty() {
                None
            } else {
                opt_str(&self.host, "server:host")
            },
            socket_path: if !self.host.trim().is_empty() {
                None
            } else {
                opt_str(&self.socket_path, "server:socketPath")
            },
            port,
            local_host: None,
            local_port: None,
            user: opt_str(&self.user, "server:user"),
            password: opt_str(&self.password, "server:password").map(Password::Plain),
            application_name: None,
            domain: opt_str(&self.domain, "server:domain"),
            ssh,
            ssl: if disabled.contains("server:ssl") {
                None
            } else {
                Some(SslConfig::Bool(self.ssl))
            },
            encrypted: None,
            database: {
                let d = self.database.trim();
                if d.is_empty() {
                    None
                } else {
                    Some(d.to_string())
                }
            },
            schema: opt_str(&self.schema, "server:schema"),
            filter,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SettingsFormDraft {
    pub zoom_factor: f64,
    pub limit_query_default_select_top: String,
    pub enabled_dark_theme: bool,
    pub disabled_open_animation: bool,
    pub connections_as_list: bool,
    pub custom_font: String,
    pub enabled_auto_complete: bool,
    pub enabled_live_auto_complete: bool,
    pub csv_delimiter: String,
    pub tab_delimited: bool,
    pub log_console: bool,
    pub log_file: bool,
    pub log_path: String,
    pub log_level: String,
}

impl SettingsFormDraft {
    pub fn from_store(store: &AppStore) -> Self {
        let cfg = store.config.data.clone().unwrap_or_default();
        let delim = cfg.csv_delimiter.clone();
        Self {
            zoom_factor: cfg.zoom_factor,
            limit_query_default_select_top: cfg.limit_query_default_select_top.to_string(),
            enabled_dark_theme: cfg.enabled_dark_theme,
            disabled_open_animation: cfg.disabled_open_animation,
            connections_as_list: cfg.connections_as_list,
            custom_font: cfg.custom_font,
            enabled_auto_complete: cfg.enabled_auto_complete,
            enabled_live_auto_complete: cfg.enabled_live_auto_complete,
            tab_delimited: delim == "  " || delim == "\t",
            csv_delimiter: delim,
            log_console: cfg.log.console,
            log_file: cfg.log.file,
            log_path: cfg.log.path,
            log_level: cfg.log.level,
        }
    }

    pub fn to_base_config(&self) -> BaseConfig {
        BaseConfig {
            zoom_factor: Some(self.zoom_factor),
            limit_query_default_select_top: self
                .limit_query_default_select_top
                .trim()
                .parse::<u32>()
                .ok(),
            enabled_auto_complete: Some(self.enabled_auto_complete),
            enabled_live_auto_complete: Some(self.enabled_live_auto_complete),
            enabled_dark_theme: Some(self.enabled_dark_theme),
            disabled_open_animation: Some(self.disabled_open_animation),
            csv_delimiter: Some(if self.tab_delimited {
                "  ".to_string()
            } else if self.csv_delimiter.is_empty() {
                ",".to_string()
            } else {
                self.csv_delimiter.clone()
            }),
            connections_as_list: Some(self.connections_as_list),
            custom_font: Some(if self.custom_font.trim().is_empty() {
                "Lato".to_string()
            } else {
                self.custom_font.trim().to_string()
            }),
            log: Some(LogOptions {
                console: self.log_console,
                file: self.log_file,
                path: self.log_path.trim().to_string(),
                level: self.log_level.clone(),
            }),
        }
    }
}

pub struct RustSqlGuiApp {
    pub rt: Arc<Runtime>,
    pub store: AppStore,
    pub route: ActiveRoute,
    pub server_filter: String,
    pub server_draft: ServerFormDraft,
    pub settings_draft: SettingsFormDraft,
    pub confirm_delete_server: bool,
    pub sidebar_collapsed: bool,
    pub db_search_filter: String,
    pub db_obj_filters: HashMap<String, String>,
    pub expanded_databases: HashSet<String>,
    pub expanded_tables: HashSet<(String, String)>,
    pub renaming_tab_id: Option<u64>,
    pub rename_tab_buffer: String,
    pub wrap_contents: bool,
    pub editor_font_size: f32,
    pub preview_cell_value: Option<Value>,
    pub preview_mode: String,
    pub show_client_info_modal: bool,
    pub show_shortcuts_modal: bool,
    pub ssh_passphrase_input: String,
    pub file_dialog_path: String,
    pub show_save_query_modal: bool,
    pub show_open_query_modal: bool,
    pub show_export_file_modal: Option<(usize, String)>,
    pub result_filter_text: String,
    pub result_sort_col: Option<(String, SortDirection)>,
    pub pending_safety_confirm: Option<(String, QuerySafetyReport)>,
    pub show_bookmarks_modal: bool,
    pub bookmark_title_input: String,
    pub bookmark_tags_input: String,
    pub bookmark_search_input: String,
    pub stats_modal_data: Option<Vec<ColumnStats>>,
    pub erd_modal_diagram: Option<String>,
}

impl RustSqlGuiApp {
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
        }
    }

    fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        let ctrl_q = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
        let ctrl_t = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);
        let ctrl_w = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
        let ctrl_enter = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
        let ctrl_r = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);
        let ctrl_i = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
        let ctrl_s = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

        ctx.input_mut(|i| {
            if i.consume_shortcut(&ctrl_q) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if matches!(self.route, ActiveRoute::QueryBrowser(_)) {
                if i.consume_shortcut(&ctrl_t) {
                    if let Some(db) = self.store.current_database() {
                        self.store.new_query(&db);
                    }
                }
                if i.consume_shortcut(&ctrl_w) {
                    if let Some(id) = self.store.queries.current_query_id {
                        self.store.remove_query(id);
                    }
                }
                if i.consume_shortcut(&ctrl_enter) || i.consume_shortcut(&ctrl_r) {
                    self.trigger_execute_current_query();
                }
                if i.consume_shortcut(&ctrl_i) {
                    self.format_current_query();
                }
                if i.consume_shortcut(&ctrl_s) {
                    self.show_save_query_modal = true;
                }
            }
        });
    }

    fn trigger_execute_current_query(&mut self) {
        let Some(q) = self.store.current_query() else {
            return;
        };
        let sql = if !q.selected_query.trim().is_empty() {
            q.selected_query.clone()
        } else {
            q.query.clone()
        };
        let safety = self.store.check_query_safety(&sql);
        if !self.store.read_only_mode && safety.requires_confirmation {
            self.pending_safety_confirm = Some((sql, safety));
            return;
        }
        self.execute_sql_confirmed(&sql);
    }

    fn execute_sql_confirmed(&mut self, sql: &str) {
        let rt = self.rt.clone();
        let _ = rt.block_on(self.store.execute_query_if_needed(sql, false));
        if let Some(db) = self.store.current_database() {
            let _ = rt.block_on(self.store.fetch_databases_if_needed());
            let _ = rt.block_on(self.store.fetch_tables_if_needed(&db));
        }
    }

    fn format_current_query(&mut self) {
        let client = self
            .store
            .connections
            .server
            .as_ref()
            .map(|s| s.client.clone());
        if let Some(q) = self.store.current_query_mut() {
            q.query = format_sql(&q.query, client.as_deref());
        }
    }

    fn connect_to_server(
        &mut self,
        server_id: &str,
        db_name: Option<&str>,
        passphrase: Option<String>,
    ) {
        let rt = self.rt.clone();
        if rt
            .block_on(self.store.connect(server_id, db_name, false, passphrase))
            .is_ok()
        {
            self.route = ActiveRoute::QueryBrowser(server_id.to_string());
            if let Some(db) = self.store.current_database() {
                self.expanded_databases.insert(db.clone());
                let _ = rt.block_on(self.store.fetch_databases_if_needed());
                let _ = rt.block_on(self.store.fetch_schemas_if_needed(&db));
                let _ = rt.block_on(self.store.fetch_tables_if_needed(&db));
                let _ = rt.block_on(self.store.fetch_views_if_needed(&db));
                let _ = rt.block_on(self.store.fetch_routines_if_needed(&db));
            }
        }
    }

    fn render_header(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Tab (Ctrl+T)").clicked() {
                    if let Some(db) = self.store.current_database() {
                        self.store.new_query(&db);
                    }
                    ui.close_menu();
                }
                if ui.button("Close Tab (Ctrl+W)").clicked() {
                    if let Some(id) = self.store.queries.current_query_id {
                        self.store.remove_query(id);
                    }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Save Query (Ctrl+S)").clicked() {
                    self.show_save_query_modal = true;
                    ui.close_menu();
                }
                if ui.button("Open Query (Ctrl+O)").clicked() {
                    self.show_open_query_modal = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Quit (Ctrl+Q)").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Query", |ui| {
                if ui.button("Execute (Ctrl+Enter / Ctrl+R)").clicked() {
                    self.trigger_execute_current_query();
                    ui.close_menu();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Beautify Query (Ctrl+I)").clicked() {
                    self.format_current_query();
                    ui.close_menu();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Zoom In (Ctrl+=)").clicked() {
                    self.settings_draft.zoom_factor =
                        (self.settings_draft.zoom_factor + 0.2).min(3.0);
                    let base = self.settings_draft.to_base_config();
                    let _ = self.store.save_config(&base);
                    ui.close_menu();
                }
                if ui.button("Zoom Out (Ctrl+-)").clicked() {
                    self.settings_draft.zoom_factor =
                        (self.settings_draft.zoom_factor - 0.2).max(0.4);
                    let base = self.settings_draft.to_base_config();
                    let _ = self.store.save_config(&base);
                    ui.close_menu();
                }
                if ui.button("Reset Zoom (Ctrl+0)").clicked() {
                    self.settings_draft.zoom_factor = 1.0;
                    let base = self.settings_draft.to_base_config();
                    let _ = self.store.save_config(&base);
                    ui.close_menu();
                }
            });
            ui.menu_button("Find", |ui| {
                if ui.button("Search databases (Shift+Ctrl+9)").clicked() {
                    self.sidebar_collapsed = false;
                    ui.close_menu();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard Shortcuts").clicked() {
                    self.show_shortcuts_modal = true;
                    ui.close_menu();
                }
                ui.hyperlink_to(
                    "Report Issue",
                    "https://github.com/Rain6435/Rust-SQL/issues",
                );
                ui.hyperlink_to("About Rust-SQL", "https://github.com/Rain6435/Rust-SQL");
            });
        });

        ui.horizontal(|ui| {
            ui.heading(RichText::new("⚡ Rust-SQL").strong());
            ui.separator();

            let current_route = self.route.clone();
            match current_route {
                ActiveRoute::ServerManagement => {
                    ui.label(RichText::new("🖥 servers").strong());
                }
                ActiveRoute::QueryBrowser(ref server_id) => {
                    let srv_name = self
                        .store
                        .connections
                        .server
                        .as_ref()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| server_id.clone());
                    let db_name = self.store.current_database().unwrap_or_default();
                    ui.label(RichText::new(format!("🖥 {srv_name}")).strong());
                    ui.label("›");
                    ui.label(RichText::new(format!("🗄 {db_name}")).strong());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("⏻ Close connection").clicked() {
                            let rt = self.rt.clone();
                            rt.block_on(self.store.disconnect());
                            self.route = ActiveRoute::ServerManagement;
                        }
                        if ui.button("🔌 Reconnect").clicked() {
                            let sid = server_id.clone();
                            let db = self.store.current_database().unwrap_or_default();
                            let rt = self.rt.clone();
                            let _ = rt.block_on(self.store.reconnect(&sid, &db));
                        }
                    });
                }
            }
        });
    }

    fn render_footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if !self.store.status.is_empty() {
                ui.label(RichText::new(&self.store.status).italics());
            } else {
                ui.label(RichText::new("Ready").weak());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("⌨ Shortcuts").clicked() {
                    self.show_shortcuts_modal = true;
                }
                ui.hyperlink_to("GitHub", "https://github.com/Rain6435/Rust-SQL");
                if let Some(ref cfg) = self.store.config.data {
                    if cfg.log.console || cfg.log.file {
                        ui.label(
                            RichText::new(format!("Log: {}", cfg.log.level))
                                .small()
                                .color(Color32::from_rgb(234, 179, 8)),
                        );
                    }
                }
            });
        });
    }

    fn render_server_management(&mut self, ui: &mut egui::Ui) {
        // Filter bar: Search + Add + Settings
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.server_filter)
                    .hint_text("Search...")
                    .desired_width(280.0),
            );
            if ui.button("➕ Add").clicked() {
                let _ = self.store.start_editing_server(None);
                self.server_draft = ServerFormDraft::from_server(None);
            }
            if ui.button("⚙ Settings").clicked() {
                self.store.start_editing_config();
                self.settings_draft = SettingsFormDraft::from_store(&self.store);
            }
        });

        ui.add_space(8.0);

        if let Some(ref err) = self.store.connections.error.clone() {
            egui::Frame::group(ui.style())
                .fill(Color32::from_rgba_unmultiplied(220, 38, 38, 35))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(220, 38, 38)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Connection Error: {err}"))
                                .color(Color32::LIGHT_RED),
                        );
                        if ui.small_button("✖").clicked() {
                            self.store.connections.error = None;
                        }
                    });
                });
            ui.add_space(8.0);
        }

        let pattern = escape_regexp_string(&self.server_filter);
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(true)
            .build()
            .ok();

        let filtered_servers: Vec<Server> = self
            .store
            .servers
            .items
            .iter()
            .filter(|srv| {
                if self.server_filter.trim().is_empty() {
                    true
                } else if let Some(ref re) = regex {
                    re.is_match(&srv.name)
                } else {
                    srv.name
                        .to_lowercase()
                        .contains(&self.server_filter.to_lowercase())
                }
            })
            .cloned()
            .collect();

        if filtered_servers.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("No results").size(16.0).weak());
            });
            return;
        }

        let as_list = self
            .store
            .config
            .data
            .as_ref()
            .map(|c| c.connections_as_list)
            .unwrap_or(false);

        egui::ScrollArea::vertical().show(ui, |ui| {
            if as_list {
                for srv in filtered_servers {
                    self.render_server_card_or_row(ui, &srv, true);
                    ui.add_space(4.0);
                }
            } else {
                ui.horizontal_wrapped(|ui| {
                    for srv in filtered_servers {
                        ui.allocate_ui(egui::vec2(260.0, 130.0), |ui| {
                            self.render_server_card_or_row(ui, &srv, false);
                        });
                    }
                });
            }
        });
    }

    fn render_server_card_or_row(&mut self, ui: &mut egui::Ui, srv: &Server, is_row: bool) {
        let addr = if let Some(ref h) = srv.host {
            format!("{}:{}", h, srv.port.unwrap_or(0))
        } else {
            srv.socket_path.clone().unwrap_or_default()
        };
        let meta = if let Some(ref ssh) = srv.ssh {
            format!("{addr} via {}", ssh.host)
        } else {
            addr
        };

        egui::Frame::group(ui.style())
            .inner_margin(10.0)
            .show(ui, |ui| {
                if is_row {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("[{}]", srv.client.to_uppercase())).strong(),
                        );
                        ui.label(RichText::new(&srv.name).strong().size(15.0));
                        ui.label(RichText::new(&meta).weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Connect").clicked() {
                                self.connect_to_server(&srv.id, None, None);
                            }
                            if ui.button("✏ Edit").clicked() {
                                let _ = self.store.start_editing_server(Some(&srv.id));
                                let editing = self.store.servers.editing_server.clone();
                                self.server_draft = ServerFormDraft::from_server(editing.as_ref());
                            }
                        });
                    });
                } else {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("[{}]", srv.client.to_uppercase()))
                                    .color(Color32::from_rgb(56, 189, 248))
                                    .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("✏").on_hover_text("Edit server").clicked()
                                    {
                                        let _ = self.store.start_editing_server(Some(&srv.id));
                                        let editing = self.store.servers.editing_server.clone();
                                        self.server_draft =
                                            ServerFormDraft::from_server(editing.as_ref());
                                    }
                                },
                            );
                        });
                        ui.label(RichText::new(&srv.name).strong().size(16.0));
                        ui.label(RichText::new(&meta).small().weak());
                        ui.add_space(6.0);
                        if ui
                            .add_sized([ui.available_width(), 26.0], egui::Button::new("Connect"))
                            .clicked()
                        {
                            self.connect_to_server(&srv.id, None, None);
                        }
                    });
                }
            });
    }

    fn render_query_browser(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, server_id: &str) {
        if !self.sidebar_collapsed {
            egui::SidePanel::left("db_sidebar")
                .resizable(true)
                .default_width(250.0)
                .width_range(200.0..=600.0)
                .show_inside(ui, |ui| {
                    self.render_sidebar(ui, server_id);
                });
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .small_button(if self.sidebar_collapsed {
                        "▶ Sidebar"
                    } else {
                        "◀"
                    })
                    .clicked()
                {
                    self.sidebar_collapsed = !self.sidebar_collapsed;
                }
                self.render_query_tabs_bar(ui);
            });
            ui.separator();
            self.render_active_query_panel(ctx, ui);
        });
    }

    fn render_sidebar(&mut self, ui: &mut egui::Ui, server_id: &str) {
        if let Some(ref srv) = self.store.connections.server {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("[{}]", srv.client.to_uppercase())).strong());
                ui.label(RichText::new(&srv.name).strong());
            });
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.db_search_filter)
                    .hint_text("Search databases...")
                    .desired_width(ui.available_width()),
            );
        });
        ui.separator();

        let dbs: Vec<String> = self
            .store
            .databases
            .items
            .iter()
            .map(|d| d.name.clone())
            .filter(|name| {
                self.db_search_filter.trim().is_empty()
                    || name
                        .to_lowercase()
                        .contains(&self.db_search_filter.to_lowercase())
            })
            .collect();

        let current_db = self.store.current_database().unwrap_or_default();
        let disabled_features = self.store.connections.disabled_features.clone();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for db_name in dbs {
                let is_current = db_name == current_db;
                let is_expanded = self.expanded_databases.contains(&db_name);
                let header_label = format!("{} 🗄 {}", if is_expanded { "▼" } else { "▶" }, db_name);

                let resp = ui.selectable_label(is_current, RichText::new(header_label).strong());
                if resp.clicked() {
                    if self.expanded_databases.contains(&db_name) {
                        self.expanded_databases.remove(&db_name);
                    } else {
                        self.expanded_databases.insert(db_name.clone());
                        let rt = self.rt.clone();
                        let _ =
                            rt.block_on(self.store.connect(server_id, Some(&db_name), false, None));
                        let _ = rt.block_on(self.store.fetch_schemas_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_tables_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_views_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_routines_if_needed(&db_name));
                    }
                }

                resp.context_menu(|ui| {
                    if ui.button("🔄 Refresh Database").clicked() {
                        self.store.refresh_database(&db_name);
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.fetch_schemas_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_tables_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_views_if_needed(&db_name));
                        let _ = rt.block_on(self.store.fetch_routines_if_needed(&db_name));
                        ui.close_menu();
                    }
                    if ui.button("➕ Open Tab").clicked() {
                        self.store.new_query(&db_name);
                        ui.close_menu();
                    }
                });

                if is_expanded {
                    ui.indent(format!("db_indent_{db_name}"), |ui| {
                        let obj_filter = self.db_obj_filters.entry(db_name.clone()).or_default();
                        ui.horizontal(|ui| {
                            ui.label("🔎");
                            ui.add(
                                egui::TextEdit::singleline(obj_filter)
                                    .hint_text("Search objects...")
                                    .desired_width(ui.available_width()),
                            );
                        });
                        let filter_val = obj_filter.to_lowercase();

                        // 1. Tables
                        let tables = self
                            .store
                            .tables
                            .items_by_database
                            .get(&db_name)
                            .cloned()
                            .unwrap_or_default();
                        ui.collapsing(format!("Tables ({})", tables.len()), |ui| {
                            for tbl in tables {
                                if !filter_val.is_empty()
                                    && !tbl.name.to_lowercase().contains(&filter_val)
                                {
                                    continue;
                                }
                                let key = (db_name.clone(), tbl.name.clone());
                                let tbl_expanded = self.expanded_tables.contains(&key);
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button(if tbl_expanded { "▾" } else { "▸" })
                                        .clicked()
                                    {
                                        if tbl_expanded {
                                            self.expanded_tables.remove(&key);
                                        } else {
                                            self.expanded_tables.insert(key.clone());
                                            let rt = self.rt.clone();
                                            let _ = rt.block_on(
                                                self.store.fetch_table_columns_if_needed(
                                                    &db_name,
                                                    &tbl.name,
                                                    tbl.schema.as_deref(),
                                                ),
                                            );
                                            let _ = rt.block_on(
                                                self.store.fetch_table_triggers_if_needed(
                                                    &db_name,
                                                    &tbl.name,
                                                    tbl.schema.as_deref(),
                                                ),
                                            );
                                            let _ = rt.block_on(
                                                self.store.fetch_table_indexes_if_needed(
                                                    &db_name,
                                                    &tbl.name,
                                                    tbl.schema.as_deref(),
                                                ),
                                            );
                                        }
                                    }
                                    let item_resp = ui.link(&tbl.name);
                                    if item_resp.clicked() {
                                        let rt = self.rt.clone();
                                        let _ = rt.block_on(
                                            self.store.execute_default_select_query_if_needed(
                                                &db_name,
                                                &tbl.name,
                                                tbl.schema.as_deref(),
                                            ),
                                        );
                                    }
                                    item_resp.context_menu(|ui| {
                                        if ui.button("Select Rows (with limit)").clicked() {
                                            let rt = self.rt.clone();
                                            let _ = rt.block_on(
                                                self.store.execute_default_select_query_if_needed(
                                                    &db_name,
                                                    &tbl.name,
                                                    tbl.schema.as_deref(),
                                                ),
                                            );
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                        if !disabled_features
                                            .iter()
                                            .any(|f| f == "scriptCreateTable")
                                            && ui.button("Create Statement").clicked()
                                        {
                                            let rt = self.rt.clone();
                                            let _ =
                                                rt.block_on(self.store.get_sql_script_if_needed(
                                                    &db_name,
                                                    &tbl.name,
                                                    ScriptActionType::Create,
                                                    DbObjectType::Table,
                                                    tbl.schema.as_deref(),
                                                ));
                                            ui.close_menu();
                                        }
                                        for (label, act) in [
                                            ("Select Statement", ScriptActionType::Select),
                                            ("Insert Statement", ScriptActionType::Insert),
                                            ("Update Statement", ScriptActionType::Update),
                                            ("Delete Statement", ScriptActionType::Delete),
                                        ] {
                                            if ui.button(label).clicked() {
                                                let rt = self.rt.clone();
                                                let _ = rt.block_on(
                                                    self.store.get_sql_script_if_needed(
                                                        &db_name,
                                                        &tbl.name,
                                                        act,
                                                        DbObjectType::Table,
                                                        tbl.schema.as_deref(),
                                                    ),
                                                );
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                });

                                if tbl_expanded {
                                    ui.indent(format!("tbl_meta_{}_{}", db_name, tbl.name), |ui| {
                                        let cols = self
                                            .store
                                            .columns
                                            .columns_by_table
                                            .get(&db_name)
                                            .and_then(|m| m.get(&tbl.name))
                                            .cloned()
                                            .unwrap_or_default();
                                        ui.collapsing(format!("Columns ({})", cols.len()), |ui| {
                                            for c in cols {
                                                ui.horizontal(|ui| {
                                                    ui.label(&c.name);
                                                    ui.label(
                                                        RichText::new(c.data_type.to_uppercase())
                                                            .small()
                                                            .weak(),
                                                    );
                                                });
                                            }
                                        });
                                        let triggers = self
                                            .store
                                            .triggers
                                            .triggers_by_table
                                            .get(&db_name)
                                            .and_then(|m| m.get(&tbl.name))
                                            .cloned()
                                            .unwrap_or_default();
                                        ui.collapsing(
                                            format!("Triggers ({})", triggers.len()),
                                            |ui| {
                                                for tr in triggers {
                                                    ui.label(&tr.name);
                                                }
                                            },
                                        );
                                        let indexes = self
                                            .store
                                            .indexes
                                            .indexes_by_table
                                            .get(&db_name)
                                            .and_then(|m| m.get(&tbl.name))
                                            .cloned()
                                            .unwrap_or_default();
                                        ui.collapsing(
                                            format!("Indexes ({})", indexes.len()),
                                            |ui| {
                                                for idx in indexes {
                                                    ui.label(&idx.name);
                                                }
                                            },
                                        );
                                    });
                                }
                            }
                        });

                        // 2. Views
                        let views = self
                            .store
                            .views
                            .views_by_database
                            .get(&db_name)
                            .cloned()
                            .unwrap_or_default();
                        ui.collapsing(format!("Views ({})", views.len()), |ui| {
                            for vw in views {
                                if !filter_val.is_empty()
                                    && !vw.name.to_lowercase().contains(&filter_val)
                                {
                                    continue;
                                }
                                let resp = ui.link(&vw.name);
                                if resp.clicked() {
                                    let rt = self.rt.clone();
                                    let _ = rt.block_on(
                                        self.store.execute_default_select_query_if_needed(
                                            &db_name,
                                            &vw.name,
                                            vw.schema.as_deref(),
                                        ),
                                    );
                                }
                                resp.context_menu(|ui| {
                                    if ui.button("Select Rows (with limit)").clicked() {
                                        let rt = self.rt.clone();
                                        let _ = rt.block_on(
                                            self.store.execute_default_select_query_if_needed(
                                                &db_name,
                                                &vw.name,
                                                vw.schema.as_deref(),
                                            ),
                                        );
                                        ui.close_menu();
                                    }
                                    if !disabled_features.iter().any(|f| f == "scriptCreateTable")
                                        && ui.button("Create Statement").clicked()
                                    {
                                        let rt = self.rt.clone();
                                        let _ = rt.block_on(self.store.get_sql_script_if_needed(
                                            &db_name,
                                            &vw.name,
                                            ScriptActionType::Create,
                                            DbObjectType::View,
                                            vw.schema.as_deref(),
                                        ));
                                        ui.close_menu();
                                    }
                                });
                            }
                        });

                        // 3. Functions
                        let funcs = self
                            .store
                            .routines
                            .functions_by_database
                            .get(&db_name)
                            .cloned()
                            .unwrap_or_default();
                        ui.collapsing(format!("Functions ({})", funcs.len()), |ui| {
                            for fn_item in funcs {
                                let resp = ui.label(&fn_item.name);
                                resp.context_menu(|ui| {
                                    if ui.button("Create Statement").clicked() {
                                        let rt = self.rt.clone();
                                        let _ = rt.block_on(self.store.get_sql_script_if_needed(
                                            &db_name,
                                            &fn_item.name,
                                            ScriptActionType::Create,
                                            DbObjectType::Function,
                                            fn_item.schema.as_deref(),
                                        ));
                                        ui.close_menu();
                                    }
                                });
                            }
                        });

                        // 4. Procedures
                        let procs = self
                            .store
                            .routines
                            .procedures_by_database
                            .get(&db_name)
                            .cloned()
                            .unwrap_or_default();
                        ui.collapsing(format!("Procedures ({})", procs.len()), |ui| {
                            for pr_item in procs {
                                let resp = ui.label(&pr_item.name);
                                resp.context_menu(|ui| {
                                    if ui.button("Create Statement").clicked() {
                                        let rt = self.rt.clone();
                                        let _ = rt.block_on(self.store.get_sql_script_if_needed(
                                            &db_name,
                                            &pr_item.name,
                                            ScriptActionType::Create,
                                            DbObjectType::Procedure,
                                            pr_item.schema.as_deref(),
                                        ));
                                        ui.close_menu();
                                    }
                                });
                            }
                        });
                    });
                }
            }
        });
    }

    fn render_query_tabs_bar(&mut self, ui: &mut egui::Ui) {
        let query_ids = self.store.queries.query_ids.clone();
        let current_id = self.store.queries.current_query_id;

        ui.horizontal_wrapped(|ui| {
            for qid in query_ids {
                let Some(q_name) = self
                    .store
                    .queries
                    .queries_by_id
                    .get(&qid)
                    .map(|q| q.name.clone())
                else {
                    continue;
                };
                let is_active = Some(qid) == current_id;

                if self.renaming_tab_id == Some(qid) {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.rename_tab_buffer)
                            .desired_width(140.0),
                    );
                    if resp.lost_focus() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        self.store.select_query(qid);
                        let new_name = self.rename_tab_buffer.clone();
                        self.store.rename_query(&new_name);
                        self.renaming_tab_id = None;
                    }
                } else {
                    let btn = ui.selectable_label(is_active, &q_name);
                    if btn.clicked() {
                        self.store.select_query(qid);
                    }
                    if btn.double_clicked() {
                        self.renaming_tab_id = Some(qid);
                        self.rename_tab_buffer = q_name;
                    }
                    if ui.small_button("✖").clicked() {
                        self.store.remove_query(qid);
                    }
                }
                ui.separator();
            }

            if ui
                .button("➕")
                .on_hover_text("New Query Tab (Ctrl+T)")
                .clicked()
            {
                if let Some(db) = self.store.current_database() {
                    self.store.new_query(&db);
                }
            }
        });
    }

    fn render_active_query_panel(&mut self, _ctx: &egui::Context, ui: &mut egui::Ui) {
        let Some(qid) = self.store.queries.current_query_id else {
            return;
        };
        let Some(query_tab) = self.store.queries.queries_by_id.get(&qid).cloned() else {
            return;
        };

        let client_key = self
            .store
            .connections
            .server
            .as_ref()
            .map(|s| s.client.clone())
            .unwrap_or_default();
        let allow_cancel = !self
            .store
            .connections
            .disabled_features
            .iter()
            .any(|f| f == "cancelQuery");

        // SQL Editor area
        let mut sql_text = query_tab.query.clone();
        ui.add_sized(
            [ui.available_width(), 160.0],
            egui::TextEdit::multiline(&mut sql_text)
                .code_editor()
                .font(FontId::monospace(self.editor_font_size))
                .desired_rows(8)
                .hint_text("Write your SQL query here..."),
        );
        if sql_text != query_tab.query {
            self.store.update_query_if_needed(&sql_text, None, None);
        }

        // Inline Bind Parameters Bar (when `:param`, `@param`, or `$1` placeholders exist)
        let detected_params = extract_query_parameters(&query_tab.query);
        if !detected_params.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("🔧 Params:").strong());
                for p_name in detected_params {
                    ui.label(format!("{p_name} ="));
                    let entry = self.store.query_params.entry(p_name).or_default();
                    ui.add(egui::TextEdit::singleline(entry).desired_width(100.0));
                }
            });
        }

        // Editor Action Controls bar
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.wrap_contents, "Wrap Contents");
            ui.checkbox(&mut self.store.read_only_mode, "🔒 Read-Only");
            if matches!(client_key.as_str(), "mysql" | "sqlserver") && ui.button("ℹ Info").clicked()
            {
                self.show_client_info_modal = true;
            }
            if ui.button("✨ Format").clicked() {
                self.format_current_query();
            }
            if ui.button("🔖 Bookmarks / History").clicked() {
                self.show_bookmarks_modal = true;
            }
            if ui.button("🗺 Schema ERD").clicked() {
                if let Some(db) = self.store.current_database() {
                    let rt = self.rt.clone();
                    if let Ok(erd) = rt.block_on(self.store.generate_database_erd(&db)) {
                        self.erd_modal_diagram = Some(erd);
                    }
                }
            }
            if ui.button("📂 Open").clicked() {
                self.show_open_query_modal = true;
            }
            if ui.button("💾 Save").clicked() {
                self.show_save_query_modal = true;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if query_tab.is_executing && allow_cancel {
                    if ui
                        .add_enabled(!query_tab.is_canceling, egui::Button::new("⏹ Cancel"))
                        .clicked()
                    {
                        let rt = self.rt.clone();
                        let _ = rt.block_on(self.store.cancel_query(qid));
                    }
                } else if ui.button("🗑 Discard").clicked() {
                    self.store.update_query_if_needed("", Some(""), None);
                }

                if ui
                    .add_enabled(
                        !query_tab.is_executing,
                        egui::Button::new(RichText::new("▶ Execute").strong()),
                    )
                    .clicked()
                {
                    self.trigger_execute_current_query();
                }
            });
        });

        ui.separator();

        // Query Results area
        if query_tab.is_executing {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("⏳ Executing query...").size(16.0));
            });
            return;
        }

        if let Some(ref err) = query_tab.error {
            egui::Frame::group(ui.style())
                .fill(Color32::from_rgba_unmultiplied(220, 38, 38, 35))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(220, 38, 38)))
                .show(ui, |ui| {
                    ui.label(RichText::new("Error").strong().color(Color32::LIGHT_RED));
                    ui.monospace(err);
                });
            return;
        }

        let Some(ref results) = query_tab.results else {
            return;
        };

        let csv_delim = self
            .store
            .config
            .data
            .as_ref()
            .map(|c| c.csv_delimiter.clone())
            .unwrap_or_else(|| ",".to_string());

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (idx, res) in results.iter().enumerate() {
                if results.len() > 1 {
                    ui.heading(format!("Query {}", idx + 1));
                }

                let secs = query_tab.execution_time.unwrap_or(0) as f64 / 1000.0;
                let is_select_or_explain = matches!(
                    res.command.as_str(),
                    "SELECT" | "EXPLAIN" | "PRAGMA" | "SHOW" | "UNKNOWN"
                );

                if !is_select_or_explain {
                    ui.label(
                        RichText::new(format!(
                            "Query executed successfully. Affected rows: {}. Took {:.3}s.",
                            res.affected_rows.unwrap_or(0),
                            secs
                        ))
                        .color(Color32::from_rgb(34, 197, 94)),
                    );
                    continue;
                }

                if res.command == "EXPLAIN" {
                    let col = res
                        .fields
                        .first()
                        .map(|f| f.name.as_str())
                        .unwrap_or("QUERY PLAN");
                    let plan: Vec<String> = res
                        .rows
                        .iter()
                        .filter_map(|r| r.get(col).map(value_to_string))
                        .collect();
                    ui.monospace(plan.join("\n"));
                    continue;
                }

                if res.fields.is_empty() && res.rows.is_empty() {
                    ui.label(format!("Query executed successfully. Took {:.3}s.", secs));
                    continue;
                }

                let row_count = res.row_count.unwrap_or(res.rows.len());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("Rows: {row_count}")).strong());
                    ui.separator();
                    ui.label(format!("{:.3}s", secs));
                    if row_count > 0 {
                        ui.separator();
                        ui.label("🔍 Filter:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.result_filter_text)
                                .hint_text("Filter rows...")
                                .desired_width(130.0),
                        );
                        if ui.small_button("📊 Stats").clicked() {
                            let col_names: Vec<String> =
                                res.fields.iter().map(|f| f.name.clone()).collect();
                            self.stats_modal_data =
                                Some(compute_column_stats(&res.rows, &col_names));
                        }
                        ui.separator();
                        ui.label("Save as:");
                        for fmt in ["CSV", "JSON", "MARKDOWN", "SQL", "JSONL"] {
                            if ui.small_button(fmt).clicked() {
                                self.show_export_file_modal = Some((idx, fmt.to_string()));
                            }
                        }
                        ui.separator();
                        ui.label("Copy as:");
                        for fmt in ["CSV", "JSON", "MARKDOWN", "SQL", "JSONL"] {
                            if ui.small_button(fmt).clicked() {
                                let _ = self.store.db.export_query_result_to_clipboard(
                                    &res.rows,
                                    fmt,
                                    Some(&csv_delim),
                                );
                            }
                        }
                    }
                });

                let mut display_rows =
                    filter_result_rows(&res.rows, &self.result_filter_text, None);
                if let Some((ref s_col, dir)) = self.result_sort_col {
                    display_rows = sort_result_rows(&display_rows, s_col, dir);
                }

                if display_rows.is_empty() {
                    ui.label(RichText::new("No results found").italics().weak());
                    continue;
                }

                let cols: Vec<String> = res.fields.iter().map(|f| f.name.clone()).collect();
                let mut builder = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center));

                for _ in &cols {
                    builder = builder.column(Column::initial(130.0).at_least(40.0).clip(true));
                }

                builder
                    .header(22.0, |mut header| {
                        for col_name in &cols {
                            header.col(|ui| {
                                let sort_indicator = match &self.result_sort_col {
                                    Some((c, SortDirection::Ascending)) if c == col_name => " ▲",
                                    Some((c, SortDirection::Descending)) if c == col_name => " ▼",
                                    _ => "",
                                };
                                if ui
                                    .button(
                                        RichText::new(format!("{col_name}{sort_indicator}"))
                                            .strong(),
                                    )
                                    .clicked()
                                {
                                    let next_dir = match &self.result_sort_col {
                                        Some((c, SortDirection::Ascending)) if c == col_name => {
                                            SortDirection::Descending
                                        }
                                        _ => SortDirection::Ascending,
                                    };
                                    self.result_sort_col = Some((col_name.clone(), next_dir));
                                }
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(20.0, display_rows.len(), |mut row| {
                            let r_idx = row.index();
                            let row_data = &display_rows[r_idx];
                            for col_name in &cols {
                                row.col(|ui| {
                                    let val = row_data.get(col_name).unwrap_or(&Value::Null);
                                    if val.is_null() {
                                        ui.label(
                                            RichText::new("NULL")
                                                .small()
                                                .color(Color32::GRAY)
                                                .italics(),
                                        );
                                    } else {
                                        let text = value_to_string(val);
                                        let resp = ui.label(&text);
                                        resp.context_menu(|ui| {
                                            if ui.button("Open Preview").clicked() {
                                                self.preview_cell_value = Some(val.clone());
                                                ui.close_menu();
                                            }
                                        });
                                    }
                                });
                            }
                        });
                    });
            }
        });
    }

    fn render_modals(&mut self, ctx: &egui::Context) {
        // 1. Server Add/Edit Modal
        if self.store.servers.is_editing {
            egui::Window::new("Server Information")
                .collapsible(false)
                .resizable(true)
                .default_width(620.0)
                .show(ctx, |ui| {
                    if self.store.connections.test_connected {
                        ui.colored_label(Color32::GREEN, "✔ Successfully connected");
                    } else if let Some(ref err) = self.store.connections.test_error {
                        ui.colored_label(Color32::LIGHT_RED, format!("✖ Connection Error: {err}"));
                    }

                    if let Some(ref v_errs) = self.store.servers.error {
                        for (field, msg) in v_errs {
                            ui.colored_label(Color32::LIGHT_RED, format!("• {field}: {msg}"));
                        }
                    }

                    let disabled: HashSet<String> = CLIENTS
                        .iter()
                        .find(|c| c.key == self.server_draft.client)
                        .map(|c| c.disabled_features.iter().cloned().collect())
                        .unwrap_or_default();

                    egui::ScrollArea::vertical()
                        .max_height(420.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Name:");
                                ui.text_edit_singleline(&mut self.server_draft.name);
                                ui.label("Database Type:");
                                let prev_client = self.server_draft.client.clone();
                                egui::ComboBox::from_id_salt("server_client_combo")
                                    .selected_text(&self.server_draft.client)
                                    .show_ui(ui, |ui| {
                                        for c in CLIENTS.iter() {
                                            ui.selectable_value(
                                                &mut self.server_draft.client,
                                                c.key.clone(),
                                                &c.name,
                                            );
                                        }
                                    });
                                if self.server_draft.client != prev_client {
                                    if let Some(dp) = CLIENTS
                                        .iter()
                                        .find(|c| c.key == self.server_draft.client)
                                        .and_then(|c| c.default_port)
                                    {
                                        self.server_draft.port = dp.to_string();
                                    }
                                }
                                ui.add_enabled(
                                    !disabled.contains("server:ssl"),
                                    egui::Checkbox::new(&mut self.server_draft.ssl, "SSL"),
                                );
                            });

                            ui.separator();
                            ui.label(RichText::new("Server Address").strong());
                            ui.horizontal(|ui| {
                                ui.label("Host:");
                                ui.add_enabled(
                                    !disabled.contains("server:host")
                                        && self.server_draft.socket_path.trim().is_empty(),
                                    egui::TextEdit::singleline(&mut self.server_draft.host)
                                        .desired_width(180.0),
                                );
                                ui.label("Port:");
                                ui.add_enabled(
                                    !disabled.contains("server:port")
                                        && self.server_draft.socket_path.trim().is_empty(),
                                    egui::TextEdit::singleline(&mut self.server_draft.port)
                                        .desired_width(70.0),
                                );
                                ui.label("Domain:");
                                ui.add_enabled(
                                    !disabled.contains("server:domain"),
                                    egui::TextEdit::singleline(&mut self.server_draft.domain)
                                        .desired_width(110.0),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label("Unix Socket Path:");
                                ui.add_enabled(
                                    !disabled.contains("server:socketPath")
                                        && self.server_draft.host.trim().is_empty(),
                                    egui::TextEdit::singleline(&mut self.server_draft.socket_path)
                                        .desired_width(300.0),
                                );
                            });

                            ui.separator();
                            ui.label(RichText::new("Credentials & Database").strong());
                            ui.horizontal(|ui| {
                                ui.label("User:");
                                ui.add_enabled(
                                    !disabled.contains("server:user"),
                                    egui::TextEdit::singleline(&mut self.server_draft.user)
                                        .desired_width(150.0),
                                );
                                ui.label("Password:");
                                ui.add_enabled(
                                    !disabled.contains("server:password"),
                                    egui::TextEdit::singleline(&mut self.server_draft.password)
                                        .password(!self.server_draft.show_plain_password)
                                        .desired_width(150.0),
                                );
                                if ui
                                    .button(if self.server_draft.show_plain_password {
                                        "🙈"
                                    } else {
                                        "👁"
                                    })
                                    .clicked()
                                {
                                    self.server_draft.show_plain_password =
                                        !self.server_draft.show_plain_password;
                                    self.server_draft.conn_uri = build_connection_uri(
                                        &self.server_draft.to_server(),
                                        self.server_draft.show_plain_password,
                                    );
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label("Initial Database/Keyspace:");
                                ui.text_edit_singleline(&mut self.server_draft.database);
                                ui.label("Initial Schema:");
                                ui.add_enabled(
                                    !disabled.contains("server:schema"),
                                    egui::TextEdit::singleline(&mut self.server_draft.schema)
                                        .desired_width(120.0),
                                );
                            });

                            ui.horizontal(|ui| {
                                ui.label("Connection URI:");
                                let resp = ui.add_enabled(
                                    self.server_draft.show_plain_password,
                                    egui::TextEdit::singleline(&mut self.server_draft.conn_uri)
                                        .desired_width(360.0),
                                );
                                if resp.changed() {
                                    if let Some(parsed) = parse_connection_uri(
                                        &self.server_draft.conn_uri,
                                        Some(&self.server_draft.to_server()),
                                    ) {
                                        let show_pw = self.server_draft.show_plain_password;
                                        self.server_draft =
                                            ServerFormDraft::from_server(Some(&parsed));
                                        self.server_draft.show_plain_password = show_pw;
                                    }
                                }
                            });

                            if !disabled.contains("server:ssh") {
                                ui.separator();
                                ui.checkbox(&mut self.server_draft.ssh_enabled, "SSH Tunnel");
                                if self.server_draft.ssh_enabled {
                                    ui.horizontal(|ui| {
                                        ui.label("SSH Host:");
                                        ui.text_edit_singleline(&mut self.server_draft.ssh_host);
                                        ui.label("SSH Port:");
                                        ui.add(
                                            egui::TextEdit::singleline(
                                                &mut self.server_draft.ssh_port,
                                            )
                                            .desired_width(60.0),
                                        );
                                        ui.checkbox(
                                            &mut self.server_draft.ssh_use_agent,
                                            "Use ssh agent",
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("SSH User:");
                                        ui.text_edit_singleline(&mut self.server_draft.ssh_user);
                                        ui.label("SSH Password:");
                                        ui.add_enabled(
                                            !self.server_draft.ssh_use_agent
                                                && self
                                                    .server_draft
                                                    .ssh_private_key
                                                    .trim()
                                                    .is_empty(),
                                            egui::TextEdit::singleline(
                                                &mut self.server_draft.ssh_password,
                                            )
                                            .password(true),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Private Key:");
                                        ui.add_enabled(
                                            !self.server_draft.ssh_use_agent
                                                && self.server_draft.ssh_password.trim().is_empty(),
                                            egui::TextEdit::singleline(
                                                &mut self.server_draft.ssh_private_key,
                                            ),
                                        );
                                        ui.checkbox(
                                            &mut self.server_draft.ssh_private_key_with_passphrase,
                                            "Passphrase",
                                        );
                                    });
                                }
                            }

                            ui.separator();
                            ui.checkbox(&mut self.server_draft.filter_enabled, "Filter");
                            if self.server_draft.filter_enabled {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label("Database Only:");
                                        ui.text_edit_multiline(
                                            &mut self.server_draft.filter_db_only,
                                        );
                                    });
                                    ui.vertical(|ui| {
                                        ui.label("Database Ignore:");
                                        ui.text_edit_multiline(
                                            &mut self.server_draft.filter_db_ignore,
                                        );
                                    });
                                });
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label("Schema Only:");
                                        ui.text_edit_multiline(
                                            &mut self.server_draft.filter_schema_only,
                                        );
                                    });
                                    ui.vertical(|ui| {
                                        ui.label("Schema Ignore:");
                                        ui.text_edit_multiline(
                                            &mut self.server_draft.filter_schema_ignore,
                                        );
                                    });
                                });
                            }
                        });

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Test").clicked() {
                            let srv = self.server_draft.to_server();
                            let rt = self.rt.clone();
                            let _ = rt.block_on(self.store.test_connection(srv));
                        }
                        let is_new = self.server_draft.id.trim().is_empty();
                        if !is_new && ui.button("Duplicate").clicked() {
                            let srv = self.server_draft.to_server();
                            let _ = self.store.duplicate_server(srv);
                        }
                        if !is_new && ui.button("🗑 Remove").clicked() {
                            self.confirm_delete_server = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Save").clicked() {
                                let srv = self.server_draft.to_server();
                                let id = if self.server_draft.id.trim().is_empty() {
                                    None
                                } else {
                                    Some(self.server_draft.id.clone())
                                };
                                let _ = self.store.save_server(srv, id.as_deref());
                            }
                            if ui.button("Cancel").clicked() {
                                self.store.finish_editing_server();
                            }
                        });
                    });
                });
        }

        // 2. Confirm Server Delete Modal
        if self.confirm_delete_server {
            egui::Window::new(format!("Delete {}", self.server_draft.name))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Are you sure you want to remove this server connection?");
                    ui.horizontal(|ui| {
                        if ui.button("Yes, Remove").clicked() {
                            let id = self.server_draft.id.clone();
                            let _ = self.store.remove_server(&id);
                            self.confirm_delete_server = false;
                        }
                        if ui.button("No").clicked() {
                            self.confirm_delete_server = false;
                        }
                    });
                });
        }

        // 3. Settings Modal
        if self.store.config.is_editing {
            egui::Window::new("Settings")
                .collapsible(false)
                .resizable(true)
                .default_width(520.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Zoom Factor ({}%):",
                            (self.settings_draft.zoom_factor * 100.0).round() as i32
                        ));
                        ui.add(
                            egui::Slider::new(&mut self.settings_draft.zoom_factor, 0.4..=3.0)
                                .step_by(0.2),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Limit of Rows from Select Top Query:");
                        ui.text_edit_singleline(
                            &mut self.settings_draft.limit_query_default_select_top,
                        );
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.settings_draft.enabled_dark_theme, "Dark Theme");
                        ui.checkbox(
                            &mut self.settings_draft.disabled_open_animation,
                            "Disable Intro",
                        );
                        ui.checkbox(
                            &mut self.settings_draft.connections_as_list,
                            "List Connections",
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Custom Font:");
                        ui.text_edit_singleline(&mut self.settings_draft.custom_font);
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.checkbox(
                            &mut self.settings_draft.enabled_auto_complete,
                            "Auto Complete",
                        );
                        ui.checkbox(
                            &mut self.settings_draft.enabled_live_auto_complete,
                            "Live Auto Complete",
                        );
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Custom CSV Delimiter:");
                        ui.add_enabled(
                            !self.settings_draft.tab_delimited,
                            egui::TextEdit::singleline(&mut self.settings_draft.csv_delimiter)
                                .desired_width(60.0),
                        );
                        ui.checkbox(
                            &mut self.settings_draft.tab_delimited,
                            "Tab Delimited Values",
                        );
                    });
                    ui.separator();
                    ui.label(RichText::new("Logging").strong());
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.settings_draft.log_console, "Console");
                        ui.checkbox(&mut self.settings_draft.log_file, "File");
                        ui.label("Level:");
                        egui::ComboBox::from_id_salt("log_level_combo")
                            .selected_text(&self.settings_draft.log_level)
                            .show_ui(ui, |ui| {
                                for lvl in ["debug", "info", "warn", "error"] {
                                    ui.selectable_value(
                                        &mut self.settings_draft.log_level,
                                        lvl.to_string(),
                                        lvl,
                                    );
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Log Path:");
                        ui.text_edit_singleline(&mut self.settings_draft.log_path);
                    });
                    if let Some(ref p) = self.store.config.path {
                        ui.label(RichText::new(format!("Config Path: {p}")).small().weak());
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            let base = self.settings_draft.to_base_config();
                            let _ = self.store.save_config(&base);
                        }
                        if ui.button("Cancel").clicked() {
                            self.store.finish_editing_config();
                        }
                    });
                });
        }

        // 4. Cell Content Preview Modal (`PreviewModal`)
        if let Some(val) = self.preview_cell_value.clone() {
            egui::Window::new("Content Preview")
                .collapsible(false)
                .resizable(true)
                .default_width(500.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(self.preview_mode == "plain", "Plain Text")
                            .clicked()
                        {
                            self.preview_mode = "plain".to_string();
                        }
                        if ui
                            .selectable_label(self.preview_mode == "json", "JSON")
                            .clicked()
                        {
                            self.preview_mode = "json".to_string();
                        }
                    });
                    ui.separator();
                    let formatted = if self.preview_mode == "json" {
                        if let Value::String(ref s) = val {
                            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                                serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| s.clone())
                            } else {
                                serde_json::to_string_pretty(&val)
                                    .unwrap_or_else(|_| "Not valid format".to_string())
                            }
                        } else {
                            serde_json::to_string_pretty(&val)
                                .unwrap_or_else(|_| "Not valid format".to_string())
                        }
                    } else {
                        value_to_string(&val)
                    };
                    egui::ScrollArea::vertical()
                        .max_height(350.0)
                        .show(ui, |ui| {
                            ui.monospace(formatted);
                        });
                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.preview_cell_value = None;
                    }
                });
        }

        // 5. SSH Passphrase Prompt Modal (`PromptModal`)
        if self.store.connections.waiting_private_key_passphrase {
            egui::Window::new("SSH Private Key Passphrase")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Enter the private key passphrase:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.ssh_passphrase_input).password(true),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            if let Some(srv_id) =
                                self.store.connections.server.as_ref().map(|s| s.id.clone())
                            {
                                let pass = self.ssh_passphrase_input.clone();
                                self.ssh_passphrase_input.clear();
                                self.connect_to_server(&srv_id, None, Some(pass));
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            let rt = self.rt.clone();
                            rt.block_on(self.store.disconnect());
                        }
                    });
                });
        }

        // 6. Keyboard Shortcuts Modal
        if self.show_shortcuts_modal {
            egui::Window::new("Keyboard Shortcuts")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.monospace("Ctrl+Enter / Ctrl+R  : Execute Query");
                    ui.monospace("Ctrl+T               : New Query Tab");
                    ui.monospace("Ctrl+W               : Close Query Tab");
                    ui.monospace("Ctrl+S               : Save Query");
                    ui.monospace("Ctrl+I               : Format SQL Query");
                    ui.monospace("Ctrl+Q               : Quit Application");
                    if ui.button("Close").clicked() {
                        self.show_shortcuts_modal = false;
                    }
                });
        }

        // 7. Server DB Client Info Modal (`ServerDBClientInfoModal`)
        if self.show_client_info_modal {
            let client_key = self
                .store
                .connections
                .server
                .as_ref()
                .map(|s| s.client.clone())
                .unwrap_or_default();
            let client_name = CLIENTS
                .iter()
                .find(|c| c.key == client_key)
                .map(|c| c.name.as_str())
                .unwrap_or("Database");
            egui::Window::new(format!("{client_name} Query Information"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let notes: &[&str] = match client_key.as_str() {
                        "mysql" => &[
                            "MySQL treats commented query as a non select query. So you may see \"affected rows\" for a commented query.",
                            "Usually executing a single query per tab will give better results.",
                        ],
                        "sqlserver" => &[
                            "MSSQL treats multiple non select queries as a single query result. So you affected rows will show the amount over all queries executed in the same tab.",
                            "Usually executing a single query per tab will give better results.",
                        ],
                        _ => &["Usually executing a single query per tab will give better results."],
                    };
                    for note in notes {
                        ui.label(format!("• {note}"));
                    }
                    if ui.button("Close").clicked() {
                        self.show_client_info_modal = false;
                    }
                });
        }

        // 8. Save Query File Modal
        if self.show_save_query_modal {
            egui::Window::new("Save Query (.sql)")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("File path:");
                    ui.text_edit_singleline(&mut self.file_dialog_path);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            if let Some(q) = self.store.current_query().cloned() {
                                if let Ok(saved) =
                                    self.store.db.save_query(&self.file_dialog_path, &q.query)
                                {
                                    if let Some(qm) = self.store.current_query_mut() {
                                        qm.name = saved.name;
                                        qm.filename = Some(saved.filename);
                                    }
                                    self.store.status = "Query saved successfully".to_string();
                                }
                            }
                            self.show_save_query_modal = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_save_query_modal = false;
                        }
                    });
                });
        }

        // 9. Open Query File Modal
        if self.show_open_query_modal {
            egui::Window::new("Open Query (.sql)")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("File path:");
                    ui.text_edit_singleline(&mut self.file_dialog_path);
                    ui.horizontal(|ui| {
                        if ui.button("Open").clicked() {
                            if let Ok(opened) = self.store.db.open_query(&self.file_dialog_path) {
                                if let Some(qm) = self.store.current_query_mut() {
                                    qm.name = opened.name;
                                    qm.query = opened.query;
                                    qm.filename = Some(opened.filename);
                                }
                            }
                            self.show_open_query_modal = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_open_query_modal = false;
                        }
                    });
                });
        }

        // 10. Export Query Result to File Modal
        if let Some((res_idx, ref exp_type)) = self.show_export_file_modal.clone() {
            egui::Window::new(format!("Export Result as {exp_type}"))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("File path:");
                    ui.text_edit_singleline(&mut self.file_dialog_path);
                    ui.horizontal(|ui| {
                        if ui.button("Export").clicked() {
                            let delim = self
                                .store
                                .config
                                .data
                                .as_ref()
                                .map(|c| c.csv_delimiter.clone())
                                .unwrap_or_else(|| ",".to_string());
                            if let Some(rows) = self
                                .store
                                .current_query()
                                .and_then(|q| q.results.as_ref())
                                .and_then(|r| r.get(res_idx))
                                .map(|r| r.rows.clone())
                            {
                                let _ = self.store.db.export_query_result_to_file(
                                    &rows,
                                    exp_type,
                                    Some(&delim),
                                    &self.file_dialog_path,
                                );
                                if let Some(qm) = self.store.current_query_mut() {
                                    qm.saved = Some(true);
                                }
                            }
                            self.show_export_file_modal = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_export_file_modal = None;
                        }
                    });
                });
        }

        // 11. Destructive Query Safety Warning Modal
        if let Some((sql, report)) = self.pending_safety_confirm.clone() {
            egui::Window::new("⚠ Destructive Query Warning")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        "This query contains potentially destructive operations:",
                    );
                    for f in &report.findings {
                        ui.label(format!("• {}", f.reason));
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Execute Anyway").clicked() {
                            self.pending_safety_confirm = None;
                            self.execute_sql_confirmed(&sql);
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_safety_confirm = None;
                        }
                    });
                });
        }

        // 12. Bookmarks & Execution History Modal
        if self.show_bookmarks_modal {
            egui::Window::new("🔖 SQL Bookmarks & Execution History")
                .collapsible(false)
                .resizable(true)
                .default_width(580.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Save Current Query as Bookmark:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.bookmark_title_input)
                                .hint_text("Title...")
                                .desired_width(140.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.bookmark_tags_input)
                                .hint_text("Tags (comma-separated)...")
                                .desired_width(140.0),
                        );
                        if ui.button("➕ Save Bookmark").clicked() {
                            if let Some(sql) = self.store.current_query().map(|q| q.query.clone()) {
                                let tags: Vec<String> = self
                                    .bookmark_tags_input
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                                let client = self
                                    .store
                                    .connections
                                    .server
                                    .as_ref()
                                    .map(|s| s.client.clone());
                                self.store.bookmark_manager.add_bookmark(
                                    &self.bookmark_title_input,
                                    &sql,
                                    tags,
                                    client,
                                );
                                self.bookmark_title_input.clear();
                                self.bookmark_tags_input.clear();
                            }
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("🔍 Search:");
                        ui.text_edit_singleline(&mut self.bookmark_search_input);
                    });

                    ui.collapsing("Saved Bookmarks", |ui| {
                        let bms: Vec<_> = self
                            .store
                            .bookmark_manager
                            .search_bookmarks(&self.bookmark_search_input)
                            .into_iter()
                            .cloned()
                            .collect();
                        for bm in bms {
                            ui.horizontal(|ui| {
                                ui.strong(&bm.title);
                                ui.monospace(&bm.sql);
                                if ui.small_button("Insert").clicked() {
                                    self.store.append_query(&bm.sql);
                                }
                                if ui.small_button("🗑").clicked() {
                                    self.store.bookmark_manager.remove_bookmark(&bm.id);
                                }
                            });
                        }
                    });

                    ui.collapsing("Execution History Audit Log", |ui| {
                        let hist: Vec<_> = self
                            .store
                            .bookmark_manager
                            .search_history(&self.bookmark_search_input)
                            .into_iter()
                            .cloned()
                            .collect();
                        for h in hist {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "[{}] {}ms ({} rows)",
                                    h.database, h.duration_ms, h.row_count
                                ));
                                ui.monospace(&h.sql);
                                if ui.small_button("Replay").clicked() {
                                    self.store.update_query_if_needed(&h.sql, None, None);
                                }
                            });
                        }
                    });

                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.show_bookmarks_modal = false;
                    }
                });
        }

        // 13. Column Summary Statistics Modal
        if let Some(ref stats) = self.stats_modal_data.clone() {
            egui::Window::new("📊 Column Summary Statistics")
                .collapsible(false)
                .resizable(true)
                .default_width(600.0)
                .show(ctx, |ui| {
                    egui::Grid::new("col_stats_grid")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Column");
                            ui.strong("Total");
                            ui.strong("Nulls");
                            ui.strong("Distinct");
                            ui.strong("Min");
                            ui.strong("Max");
                            ui.strong("Mean");
                            ui.end_row();
                            for s in stats {
                                ui.label(&s.column_name);
                                ui.label(s.total_rows.to_string());
                                ui.label(s.null_count.to_string());
                                ui.label(s.distinct_count.to_string());
                                ui.label(s.min_value.as_deref().unwrap_or("-"));
                                ui.label(s.max_value.as_deref().unwrap_or("-"));
                                ui.label(
                                    s.mean
                                        .map(|m| format!("{m:.2}"))
                                        .unwrap_or_else(|| "-".to_string()),
                                );
                                ui.end_row();
                            }
                        });
                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.stats_modal_data = None;
                    }
                });
        }

        // 14. Schema Mermaid ERD Modal
        if let Some(ref erd) = self.erd_modal_diagram.clone() {
            egui::Window::new("🗺 Schema ER Diagram (Mermaid)")
                .collapsible(false)
                .resizable(true)
                .default_width(540.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(340.0)
                        .show(ui, |ui| {
                            ui.monospace(erd);
                        });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("📋 Copy Mermaid").clicked() {
                            write_clipboard_text(erd);
                        }
                        if ui.button("Close").clicked() {
                            self.erd_modal_diagram = None;
                        }
                    });
                });
        }
    }
}

impl eframe::App for RustSqlGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(ref cfg) = self.store.config.data {
            if cfg.enabled_dark_theme {
                ctx.set_visuals(egui::Visuals::dark());
            } else {
                ctx.set_visuals(egui::Visuals::light());
            }
            let z = (cfg.zoom_factor as f32).clamp(0.4, 3.0);
            ctx.set_pixels_per_point(z);
        }

        self.handle_global_shortcuts(ctx);

        egui::TopBottomPanel::top("rust_sql_header")
            .exact_height(56.0)
            .show(ctx, |ui| {
                self.render_header(ui);
            });

        egui::TopBottomPanel::bottom("rust_sql_footer")
            .exact_height(28.0)
            .show(ctx, |ui| {
                self.render_footer(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let route = self.route.clone();
            match route {
                ActiveRoute::ServerManagement => {
                    self.render_server_management(ui);
                }
                ActiveRoute::QueryBrowser(ref server_id) => {
                    self.render_query_browser(ctx, ui, server_id);
                }
            }
        });

        self.render_modals(ctx);
    }
}

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
