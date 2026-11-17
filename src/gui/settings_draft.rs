//! Editable draft state (`SettingsFormDraft`) for the application Settings modal.

use crate::common::types::{BaseConfig, LogOptions};
use crate::state::AppStore;

/// Editable form fields for the application Settings modal.
#[derive(Debug, Clone, Default)]
pub struct SettingsFormDraft {
    /// UI zoom scale factor (`0.4..=3.0`).
    pub zoom_factor: f64,
    /// Default row limit string for `SELECT TOP` queries.
    pub limit_query_default_select_top: String,
    /// Whether the dark visual theme is enabled.
    pub enabled_dark_theme: bool,
    /// Whether intro animations are disabled.
    pub disabled_open_animation: bool,
    /// Whether connections are displayed as a vertical list instead of cards.
    pub connections_as_list: bool,
    /// Custom font family name.
    pub custom_font: String,
    /// Whether SQL auto-completion is enabled.
    pub enabled_auto_complete: bool,
    /// Whether live auto-completion is enabled.
    pub enabled_live_auto_complete: bool,
    /// Custom CSV delimiter string.
    pub csv_delimiter: String,
    /// Whether tab-delimited CSV export is enabled.
    pub tab_delimited: bool,
    /// Whether console logging is enabled.
    pub log_console: bool,
    /// Whether file logging is enabled.
    pub log_file: bool,
    /// Log file destination path.
    pub log_path: String,
    /// Active log level (`"debug"`, `"info"`, `"warn"`, `"error"`).
    pub log_level: String,
}

impl SettingsFormDraft {
    /// Populates a [`SettingsFormDraft`] from the active configuration in `store`.
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

    /// Converts the settings draft into a [`BaseConfig`] ready to be saved to disk.
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
