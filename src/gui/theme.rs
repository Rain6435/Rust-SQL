//! Unified visual identity, dual-mode color tokens, typography, and reusable UI primitives
//! for the native `eframe` / `egui` Desktop GUI.

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, TextStyle, Vec2};

/// Semantic color token set for a single GUI theme mode (Dark or Light).
#[derive(Debug, Clone, Copy)]
pub struct AppTheme {
    pub is_dark: bool,
    /// Deepest background canvas (behind panels / editor / results).
    pub bg_canvas: Color32,
    /// Primary surface for sidebars, cards, and header/footer bars.
    pub bg_surface: Color32,
    /// Elevated surface for inputs, table headers, active tabs, and hover states.
    pub bg_elevated: Color32,
    /// Crisp 1px structural outline for cards, panes, and tables.
    pub border_hairline: Color32,
    /// Stronger border for hovered/focused interactive elements.
    pub border_strong: Color32,
    /// Primary readable text.
    pub text_primary: Color32,
    /// Secondary/muted text (metadata, placeholders, NULLs).
    pub text_muted: Color32,
    /// Primary brand accent (Burnished Rust) for primary CTAs and active indicators.
    pub accent_primary: Color32,
    /// Text color rendered on top of `accent_primary` fills.
    pub accent_primary_fg: Color32,
    /// Secondary blueprint/steel-cyan accent for links, active selections, and info.
    pub accent_secondary: Color32,
    /// Success / connected / affected-rows color.
    pub semantic_ok: Color32,
    /// Warning / read-only / log-active color.
    pub semantic_warn: Color32,
    /// Error / destructive / emulated-banner color.
    pub semantic_err: Color32,
    /// Subtle alternating row tint for tables.
    pub table_stripe: Color32,
}

impl AppTheme {
    /// "Deep Slate Foundry" dark theme.
    pub const DARK: Self = Self {
        is_dark: true,
        bg_canvas: Color32::from_rgb(17, 20, 26),
        bg_surface: Color32::from_rgb(24, 28, 36),
        bg_elevated: Color32::from_rgb(33, 39, 50),
        border_hairline: Color32::from_rgb(44, 52, 67),
        border_strong: Color32::from_rgb(71, 85, 105),
        text_primary: Color32::from_rgb(230, 234, 242),
        text_muted: Color32::from_rgb(139, 149, 167),
        accent_primary: Color32::from_rgb(229, 115, 66),
        accent_primary_fg: Color32::from_rgb(255, 255, 255),
        accent_secondary: Color32::from_rgb(78, 168, 222),
        semantic_ok: Color32::from_rgb(52, 211, 153),
        semantic_warn: Color32::from_rgb(251, 191, 36),
        semantic_err: Color32::from_rgb(248, 113, 113),
        table_stripe: Color32::from_rgb(21, 25, 33),
    };

    /// "Drafting Paper" light theme.
    pub const LIGHT: Self = Self {
        is_dark: false,
        bg_canvas: Color32::from_rgb(238, 241, 245),
        bg_surface: Color32::from_rgb(255, 255, 255),
        bg_elevated: Color32::from_rgb(226, 232, 240),
        border_hairline: Color32::from_rgb(203, 213, 225),
        border_strong: Color32::from_rgb(148, 163, 184),
        text_primary: Color32::from_rgb(19, 24, 34),
        text_muted: Color32::from_rgb(92, 103, 125),
        accent_primary: Color32::from_rgb(200, 75, 30),
        accent_primary_fg: Color32::from_rgb(255, 255, 255),
        accent_secondary: Color32::from_rgb(29, 111, 165),
        semantic_ok: Color32::from_rgb(5, 150, 105),
        semantic_warn: Color32::from_rgb(217, 119, 6),
        semantic_err: Color32::from_rgb(220, 38, 38),
        table_stripe: Color32::from_rgb(246, 248, 251),
    };

    /// Returns the active theme for the given dark-mode flag.
    pub fn from_dark(dark: bool) -> Self {
        if dark {
            Self::DARK
        } else {
            Self::LIGHT
        }
    }

    /// Returns a distinct, readable accent color for each SQL dialect key.
    pub fn dialect_color(&self, client: &str) -> Color32 {
        match client.to_ascii_lowercase().as_str() {
            "postgresql" | "postgres" => {
                if self.is_dark {
                    Color32::from_rgb(96, 165, 250)
                } else {
                    Color32::from_rgb(37, 99, 235)
                }
            }
            "mysql" | "mariadb" => {
                if self.is_dark {
                    Color32::from_rgb(245, 158, 11)
                } else {
                    Color32::from_rgb(180, 83, 9)
                }
            }
            "sqlite" => {
                if self.is_dark {
                    Color32::from_rgb(52, 211, 153)
                } else {
                    Color32::from_rgb(5, 150, 105)
                }
            }
            "sqlserver" => {
                if self.is_dark {
                    Color32::from_rgb(248, 113, 113)
                } else {
                    Color32::from_rgb(185, 28, 28)
                }
            }
            "cassandra" => {
                if self.is_dark {
                    Color32::from_rgb(167, 139, 250)
                } else {
                    Color32::from_rgb(109, 40, 217)
                }
            }
            _ => self.accent_secondary,
        }
    }

    /// Applies this theme's palette, spacing, outlines, and type scale to `ctx`.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = if self.is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        visuals.panel_fill = self.bg_canvas;
        visuals.window_fill = self.bg_surface;
        visuals.extreme_bg_color = self.bg_canvas;
        visuals.faint_bg_color = self.table_stripe;
        visuals.code_bg_color = self.bg_canvas;
        visuals.hyperlink_color = self.accent_secondary;
        visuals.warn_fg_color = self.semantic_warn;
        visuals.error_fg_color = self.semantic_err;
        visuals.window_corner_radius = CornerRadius::same(8);
        visuals.window_stroke = Stroke::new(1.0, self.border_hairline);
        visuals.menu_corner_radius = CornerRadius::same(6);

        visuals.selection.bg_fill = self.accent_primary.gamma_multiply(0.28);
        visuals.selection.stroke = Stroke::new(1.0, self.accent_primary);

        // Non-interactive widgets (labels, group frames)
        visuals.widgets.noninteractive.bg_fill = self.bg_surface;
        visuals.widgets.noninteractive.weak_bg_fill = self.bg_surface;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border_hairline);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.noninteractive.corner_radius = CornerRadius::same(5);

        // Inactive buttons / inputs
        visuals.widgets.inactive.bg_fill = self.bg_elevated;
        visuals.widgets.inactive.weak_bg_fill = self.bg_surface;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border_hairline);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(4);

        // Hovered
        visuals.widgets.hovered.bg_fill = self.bg_elevated;
        visuals.widgets.hovered.weak_bg_fill = self.bg_elevated;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.border_strong);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, self.text_primary);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(4);

        // Active / pressed
        visuals.widgets.active.bg_fill = self.accent_primary.gamma_multiply(0.20);
        visuals.widgets.active.weak_bg_fill = self.accent_primary.gamma_multiply(0.20);
        visuals.widgets.active.bg_stroke = Stroke::new(1.2, self.accent_primary);
        visuals.widgets.active.fg_stroke = Stroke::new(1.5, self.text_primary);
        visuals.widgets.active.corner_radius = CornerRadius::same(4);

        // Open menu / combo
        visuals.widgets.open.bg_fill = self.bg_elevated;
        visuals.widgets.open.weak_bg_fill = self.bg_elevated;
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, self.accent_primary);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, self.text_primary);
        visuals.widgets.open.corner_radius = CornerRadius::same(4);

        let egui_theme = if self.is_dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        ctx.set_theme(egui_theme);
        ctx.set_visuals_of(egui_theme, visuals);

        ctx.style_mut_of(egui_theme, |style| {
            style.spacing.item_spacing = Vec2::new(8.0, 6.0);
            style.spacing.button_padding = Vec2::new(10.0, 4.5);
            style.spacing.window_margin = Margin::same(12);
            style.spacing.indent = 16.0;
            style.spacing.scroll.bar_width = 8.0;

            style
                .text_styles
                .insert(TextStyle::Heading, egui::FontId::proportional(17.0));
            style
                .text_styles
                .insert(TextStyle::Body, egui::FontId::proportional(13.5));
            style
                .text_styles
                .insert(TextStyle::Button, egui::FontId::proportional(13.0));
            style
                .text_styles
                .insert(TextStyle::Small, egui::FontId::proportional(11.5));
            style
                .text_styles
                .insert(TextStyle::Monospace, egui::FontId::monospace(13.0));
        });
    }
}

/// Renders a compact outlined pill badge (e.g., dialect tag, status indicator, completion kind).
pub fn badge_pill(ui: &mut egui::Ui, text: &str, accent: Color32) {
    let fill = accent.gamma_multiply(0.16);
    let stroke = Stroke::new(1.0, accent.gamma_multiply(0.55));
    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().strong().color(accent));
        });
}

/// Builds a primary CTA button styled with the theme's burnished-rust accent.
pub fn primary_button(theme: &AppTheme, label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label).strong().color(theme.accent_primary_fg))
        .fill(theme.accent_primary)
        .stroke(Stroke::new(1.0, theme.accent_primary))
        .corner_radius(CornerRadius::same(4))
}

/// Builds a destructive-action button styled with the theme's semantic error color.
pub fn danger_button(theme: &AppTheme, label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label).strong().color(Color32::WHITE))
        .fill(theme.semantic_err)
        .stroke(Stroke::new(1.0, theme.semantic_err))
        .corner_radius(CornerRadius::same(4))
}

/// Builds a subtle outlined secondary button.
pub fn subtle_button(theme: &AppTheme, label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label).color(theme.text_primary))
        .fill(theme.bg_surface)
        .stroke(Stroke::new(1.0, theme.border_hairline))
        .corner_radius(CornerRadius::same(4))
}

/// Builds a framed card container with consistent 1px outline and surface fill.
pub fn card_frame(theme: &AppTheme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.bg_surface)
        .stroke(Stroke::new(1.0, theme.border_hairline))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(12))
}

#[cfg(test)]
mod tests {
    use super::AppTheme;

    #[test]
    fn from_dark_selects_matching_theme_polarity() {
        let dark = AppTheme::from_dark(true);
        let light = AppTheme::from_dark(false);
        assert!(dark.is_dark);
        assert!(!light.is_dark);
        assert_ne!(dark.bg_canvas, light.bg_canvas);
        assert_ne!(dark.accent_primary, light.accent_primary);
    }

    #[test]
    fn dialect_colors_are_distinct_across_major_engines() {
        let theme = AppTheme::DARK;
        let pg = theme.dialect_color("postgresql");
        let mysql = theme.dialect_color("mysql");
        let sqlite = theme.dialect_color("sqlite");
        let mssql = theme.dialect_color("sqlserver");
        let cass = theme.dialect_color("cassandra");
        assert_ne!(pg, mysql);
        assert_ne!(mysql, sqlite);
        assert_ne!(sqlite, mssql);
        assert_ne!(mssql, cass);
    }
}
