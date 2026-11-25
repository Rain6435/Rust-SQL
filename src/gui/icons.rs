//! Crisp vector icons painted directly with `egui::Painter` using `AppTheme` colors.

use crate::gui::theme::AppTheme;
use eframe::egui::{self, Color32, CornerRadius, Margin, Pos2, Rect, RichText, Stroke, Vec2};

/// Vector icon variants used across the Desktop GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Search,
    Plus,
    Settings,
    Grid,
    List,
    Edit,
    Play,
    Stop,
    Refresh,
    Disconnect,
    Database,
    Table,
    View,
    Column,
    Function,
    Index,
    Bookmark,
    Diagram,
    Import,
    FolderOpen,
    Save,
    Trash,
    Format,
    Lock,
    Chart,
    Copy,
    Download,
    Sun,
    Moon,
    Keyboard,
    Server,
    Check,
    Warning,
}

/// Allocates a square icon slot of `size` points and paints `kind` in `color`.
pub fn paint_icon_ui(ui: &mut egui::Ui, kind: IconKind, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_icon(ui.painter(), rect, kind, color);
    }
}

/// Renders an outlined secondary button with a leading vector icon and text label.
pub fn icon_subtle_button(
    ui: &mut egui::Ui,
    theme: &AppTheme,
    kind: IconKind,
    label: &str,
) -> egui::Response {
    icon_button_inner(
        ui,
        kind,
        label,
        theme.text_primary,
        theme.text_primary,
        theme.bg_surface,
        theme.border_hairline,
        theme.border_strong,
    )
}

/// Renders a primary rust-accented button with a leading vector icon and text label.
pub fn icon_primary_button(
    ui: &mut egui::Ui,
    theme: &AppTheme,
    kind: IconKind,
    label: &str,
) -> egui::Response {
    icon_button_inner(
        ui,
        kind,
        label,
        theme.accent_primary_fg,
        theme.accent_primary_fg,
        theme.accent_primary,
        theme.accent_primary,
        theme.accent_primary,
    )
}

/// Renders a full-width primary button with a centered vector icon and text label.
pub fn icon_primary_button_wide(
    ui: &mut egui::Ui,
    theme: &AppTheme,
    kind: IconKind,
    label: &str,
    width: f32,
    height: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if resp.hovered() {
            theme.accent_primary.gamma_multiply(0.9)
        } else {
            theme.accent_primary
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(4),
            fill,
            Stroke::new(1.0, theme.accent_primary),
            egui::StrokeKind::Inside,
        );

        let galley = ui.painter().layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(13.0),
            theme.accent_primary_fg,
        );
        let icon_sz = 13.0;
        let gap = 6.0;
        let total_w = icon_sz + gap + galley.size().x;
        let start_x = rect.center().x - total_w * 0.5;
        let icon_rect = Rect::from_center_size(
            Pos2::new(start_x + icon_sz * 0.5, rect.center().y),
            Vec2::splat(icon_sz),
        );
        paint_icon(ui.painter(), icon_rect, kind, theme.accent_primary_fg);
        ui.painter().galley(
            Pos2::new(
                start_x + icon_sz + gap,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            theme.accent_primary_fg,
        );
    }
    resp
}

#[allow(clippy::too_many_arguments)]
fn icon_button_inner(
    ui: &mut egui::Ui,
    kind: IconKind,
    label: &str,
    icon_color: Color32,
    text_color: Color32,
    fill: Color32,
    stroke_color: Color32,
    hover_stroke: Color32,
) -> egui::Response {
    let icon_sz = 13.0;
    let gap = if label.is_empty() { 0.0 } else { 5.0 };
    let pad = Margin::symmetric(9, 4);
    let galley = if label.is_empty() {
        None
    } else {
        Some(ui.painter().layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(13.0),
            text_color,
        ))
    };
    let text_w = galley.as_ref().map_or(0.0, |g| g.size().x);
    let text_h = galley.as_ref().map_or(icon_sz, |g| g.size().y);
    let desired = Vec2::new(
        pad.left as f32 + icon_sz + gap + text_w + pad.right as f32,
        (text_h.max(icon_sz) + pad.top as f32 + pad.bottom as f32).max(24.0),
    );
    let (rect, resp) = ui.allocate_exact_size(desired, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let stroke = Stroke::new(1.0, if hovered { hover_stroke } else { stroke_color });
        let bg = if hovered && fill != stroke_color {
            fill.gamma_multiply(1.15)
        } else {
            fill
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(4),
            bg,
            stroke,
            egui::StrokeKind::Inside,
        );
        let icon_rect = Rect::from_center_size(
            Pos2::new(
                rect.left() + pad.left as f32 + icon_sz * 0.5,
                rect.center().y,
            ),
            Vec2::splat(icon_sz),
        );
        paint_icon(ui.painter(), icon_rect, kind, icon_color);
        if let Some(g) = galley {
            let tx = icon_rect.right() + gap;
            let ty = rect.center().y - g.size().y * 0.5;
            ui.painter().galley(Pos2::new(tx, ty), g, text_color);
        }
    }
    resp
}

/// Renders a compact icon-only subtle button with a hover tooltip.
pub fn icon_only_button(
    ui: &mut egui::Ui,
    theme: &AppTheme,
    kind: IconKind,
    tooltip: &str,
) -> egui::Response {
    icon_subtle_button(ui, theme, kind, "").on_hover_text(tooltip)
}

/// Renders a small inline icon + colored label (for sidebar sections, badges, etc.).
pub fn icon_label(ui: &mut egui::Ui, kind: IconKind, icon_color: Color32, text: RichText) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        paint_icon_ui(ui, kind, 13.0, icon_color);
        ui.label(text);
    });
}

/// Paints a crisp 1.4px-stroke vector icon inside `rect`.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, kind: IconKind, color: Color32) {
    let r = rect.shrink(1.0);
    let c = r.center();
    let w = r.width();
    let h = r.height();
    let s = Stroke::new(1.4, color);
    let pt = |nx: f32, ny: f32| Pos2::new(r.left() + nx * w, r.top() + ny * h);

    match kind {
        IconKind::Search => {
            let center = pt(0.42, 0.42);
            painter.circle_stroke(center, w * 0.28, s);
            painter.line_segment([pt(0.62, 0.62), pt(0.92, 0.92)], s);
        }
        IconKind::Plus => {
            painter.line_segment([pt(0.5, 0.15), pt(0.5, 0.85)], s);
            painter.line_segment([pt(0.15, 0.5), pt(0.85, 0.5)], s);
        }
        IconKind::Settings => {
            painter.circle_stroke(c, w * 0.22, s);
            for i in 0..6 {
                let a = (i as f32) * std::f32::consts::TAU / 6.0;
                let p1 = Pos2::new(c.x + a.cos() * w * 0.28, c.y + a.sin() * h * 0.28);
                let p2 = Pos2::new(c.x + a.cos() * w * 0.44, c.y + a.sin() * h * 0.44);
                painter.line_segment([p1, p2], s);
            }
        }
        IconKind::Grid => {
            for (x, y) in [(0.12, 0.12), (0.56, 0.12), (0.12, 0.56), (0.56, 0.56)] {
                let cell = Rect::from_min_max(pt(x, y), pt(x + 0.32, y + 0.32));
                painter.rect_stroke(cell, CornerRadius::same(1), s, egui::StrokeKind::Inside);
            }
        }
        IconKind::List => {
            for y in [0.24, 0.50, 0.76] {
                painter.circle_filled(pt(0.16, y), 1.3, color);
                painter.line_segment([pt(0.32, y), pt(0.90, y)], s);
            }
        }
        IconKind::Edit => {
            painter.line_segment([pt(0.20, 0.82), pt(0.76, 0.18)], s);
            painter.line_segment([pt(0.16, 0.86), pt(0.34, 0.80)], s);
            painter.line_segment([pt(0.68, 0.14), pt(0.84, 0.28)], s);
        }
        IconKind::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![pt(0.24, 0.15), pt(0.86, 0.50), pt(0.24, 0.85)],
                color,
                Stroke::NONE,
            ));
        }
        IconKind::Stop => {
            let box_r = Rect::from_min_max(pt(0.22, 0.22), pt(0.78, 0.78));
            painter.rect_filled(box_r, CornerRadius::same(1), color);
        }
        IconKind::Refresh => {
            let pts: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let a = -0.4 + (i as f32 / 10.0) * std::f32::consts::PI * 1.55;
                    Pos2::new(c.x + a.cos() * w * 0.36, c.y + a.sin() * h * 0.36)
                })
                .collect();
            painter.add(egui::Shape::line(pts, s));
            painter.line_segment([pt(0.78, 0.15), pt(0.84, 0.38)], s);
            painter.line_segment([pt(0.84, 0.38), pt(0.62, 0.38)], s);
        }
        IconKind::Disconnect => {
            painter.line_segment([pt(0.50, 0.12), pt(0.50, 0.46)], s);
            let pts: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let a = -0.9 + (i as f32 / 10.0) * (std::f32::consts::TAU - 1.3);
                    Pos2::new(
                        c.x + (a - std::f32::consts::FRAC_PI_2).cos() * w * 0.35,
                        c.y + (a - std::f32::consts::FRAC_PI_2).sin() * h * 0.35,
                    )
                })
                .collect();
            painter.add(egui::Shape::line(pts, s));
        }
        IconKind::Database => {
            let top = Rect::from_min_max(pt(0.16, 0.12), pt(0.84, 0.36));
            painter.rect_stroke(top, CornerRadius::same(3), s, egui::StrokeKind::Inside);
            painter.line_segment([pt(0.16, 0.24), pt(0.16, 0.78)], s);
            painter.line_segment([pt(0.84, 0.24), pt(0.84, 0.78)], s);
            painter.line_segment([pt(0.16, 0.54), pt(0.84, 0.54)], s);
            painter.line_segment([pt(0.16, 0.78), pt(0.84, 0.78)], s);
        }
        IconKind::Table => {
            let box_r = Rect::from_min_max(pt(0.12, 0.16), pt(0.88, 0.84));
            painter.rect_stroke(box_r, CornerRadius::same(2), s, egui::StrokeKind::Inside);
            painter.line_segment([pt(0.12, 0.38), pt(0.88, 0.38)], s);
            painter.line_segment([pt(0.46, 0.38), pt(0.46, 0.84)], s);
        }
        IconKind::View => {
            painter.line_segment([pt(0.10, 0.50), pt(0.50, 0.24)], s);
            painter.line_segment([pt(0.50, 0.24), pt(0.90, 0.50)], s);
            painter.line_segment([pt(0.10, 0.50), pt(0.50, 0.76)], s);
            painter.line_segment([pt(0.50, 0.76), pt(0.90, 0.50)], s);
            painter.circle_filled(c, 1.8, color);
        }
        IconKind::Column => {
            let box_r = Rect::from_min_max(pt(0.30, 0.12), pt(0.70, 0.88));
            painter.rect_stroke(box_r, CornerRadius::same(1), s, egui::StrokeKind::Inside);
            painter.line_segment([pt(0.30, 0.36), pt(0.70, 0.36)], s);
            painter.line_segment([pt(0.30, 0.62), pt(0.70, 0.62)], s);
        }
        IconKind::Function => {
            painter.line_segment([pt(0.34, 0.18), pt(0.22, 0.50)], s);
            painter.line_segment([pt(0.22, 0.50), pt(0.34, 0.82)], s);
            painter.line_segment([pt(0.66, 0.18), pt(0.78, 0.50)], s);
            painter.line_segment([pt(0.78, 0.50), pt(0.66, 0.82)], s);
            painter.circle_filled(c, 1.4, color);
        }
        IconKind::Index => {
            painter.line_segment([pt(0.58, 0.12), pt(0.28, 0.52)], s);
            painter.line_segment([pt(0.28, 0.52), pt(0.54, 0.52)], s);
            painter.line_segment([pt(0.54, 0.52), pt(0.42, 0.88)], s);
        }
        IconKind::Bookmark => {
            let pts = vec![
                pt(0.26, 0.14),
                pt(0.74, 0.14),
                pt(0.74, 0.86),
                pt(0.50, 0.66),
                pt(0.26, 0.86),
                pt(0.26, 0.14),
            ];
            painter.add(egui::Shape::line(pts, s));
        }
        IconKind::Diagram => {
            let n1 = Rect::from_min_max(pt(0.10, 0.16), pt(0.44, 0.44));
            let n2 = Rect::from_min_max(pt(0.56, 0.56), pt(0.90, 0.84));
            painter.rect_stroke(n1, CornerRadius::same(1), s, egui::StrokeKind::Inside);
            painter.rect_stroke(n2, CornerRadius::same(1), s, egui::StrokeKind::Inside);
            painter.line_segment([pt(0.44, 0.30), pt(0.72, 0.30)], s);
            painter.line_segment([pt(0.72, 0.30), pt(0.72, 0.56)], s);
        }
        IconKind::Import => {
            painter.line_segment([pt(0.50, 0.14), pt(0.50, 0.64)], s);
            painter.line_segment([pt(0.32, 0.46), pt(0.50, 0.64)], s);
            painter.line_segment([pt(0.68, 0.46), pt(0.50, 0.64)], s);
            painter.line_segment([pt(0.16, 0.68), pt(0.16, 0.84)], s);
            painter.line_segment([pt(0.16, 0.84), pt(0.84, 0.84)], s);
            painter.line_segment([pt(0.84, 0.84), pt(0.84, 0.68)], s);
        }
        IconKind::FolderOpen => {
            let pts = vec![
                pt(0.12, 0.24),
                pt(0.42, 0.24),
                pt(0.50, 0.34),
                pt(0.88, 0.34),
                pt(0.88, 0.80),
                pt(0.12, 0.80),
                pt(0.12, 0.24),
            ];
            painter.add(egui::Shape::line(pts, s));
        }
        IconKind::Save => {
            let box_r = Rect::from_min_max(pt(0.14, 0.14), pt(0.86, 0.86));
            painter.rect_stroke(box_r, CornerRadius::same(1), s, egui::StrokeKind::Inside);
            let top = Rect::from_min_max(pt(0.28, 0.14), pt(0.70, 0.42));
            painter.rect_stroke(top, CornerRadius::ZERO, s, egui::StrokeKind::Inside);
            let bot = Rect::from_min_max(pt(0.26, 0.56), pt(0.74, 0.86));
            painter.rect_stroke(bot, CornerRadius::ZERO, s, egui::StrokeKind::Inside);
        }
        IconKind::Trash => {
            painter.line_segment([pt(0.18, 0.26), pt(0.82, 0.26)], s);
            painter.line_segment([pt(0.38, 0.14), pt(0.62, 0.14)], s);
            let body = Rect::from_min_max(pt(0.26, 0.26), pt(0.74, 0.86));
            painter.rect_stroke(body, CornerRadius::same(1), s, egui::StrokeKind::Inside);
            painter.line_segment([pt(0.42, 0.40), pt(0.42, 0.72)], s);
            painter.line_segment([pt(0.58, 0.40), pt(0.58, 0.72)], s);
        }
        IconKind::Format => {
            painter.line_segment([pt(0.16, 0.24), pt(0.84, 0.24)], s);
            painter.line_segment([pt(0.32, 0.50), pt(0.84, 0.50)], s);
            painter.line_segment([pt(0.16, 0.76), pt(0.68, 0.76)], s);
        }
        IconKind::Lock => {
            let body = Rect::from_min_max(pt(0.20, 0.46), pt(0.80, 0.86));
            painter.rect_stroke(body, CornerRadius::same(2), s, egui::StrokeKind::Inside);
            painter.line_segment([pt(0.32, 0.46), pt(0.32, 0.26)], s);
            painter.line_segment([pt(0.32, 0.26), pt(0.68, 0.26)], s);
            painter.line_segment([pt(0.68, 0.26), pt(0.68, 0.46)], s);
        }
        IconKind::Chart => {
            painter.line_segment([pt(0.22, 0.82), pt(0.22, 0.52)], s);
            painter.line_segment([pt(0.50, 0.82), pt(0.50, 0.22)], s);
            painter.line_segment([pt(0.78, 0.82), pt(0.78, 0.38)], s);
            painter.line_segment([pt(0.12, 0.84), pt(0.88, 0.84)], s);
        }
        IconKind::Copy => {
            let back = Rect::from_min_max(pt(0.16, 0.16), pt(0.66, 0.66));
            let front = Rect::from_min_max(pt(0.34, 0.34), pt(0.84, 0.84));
            painter.rect_stroke(back, CornerRadius::same(1), s, egui::StrokeKind::Inside);
            painter.rect_stroke(front, CornerRadius::same(1), s, egui::StrokeKind::Inside);
        }
        IconKind::Download => {
            painter.line_segment([pt(0.50, 0.16), pt(0.50, 0.68)], s);
            painter.line_segment([pt(0.30, 0.50), pt(0.50, 0.70)], s);
            painter.line_segment([pt(0.70, 0.50), pt(0.50, 0.70)], s);
            painter.line_segment([pt(0.18, 0.84), pt(0.82, 0.84)], s);
        }
        IconKind::Sun => {
            painter.circle_stroke(c, w * 0.20, s);
            for i in 0..8 {
                let a = (i as f32) * std::f32::consts::TAU / 8.0;
                let p1 = Pos2::new(c.x + a.cos() * w * 0.30, c.y + a.sin() * h * 0.30);
                let p2 = Pos2::new(c.x + a.cos() * w * 0.44, c.y + a.sin() * h * 0.44);
                painter.line_segment([p1, p2], s);
            }
        }
        IconKind::Moon => {
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = 0.6 + (i as f32 / 12.0) * std::f32::consts::PI * 1.45;
                    Pos2::new(c.x + a.cos() * w * 0.34, c.y + a.sin() * h * 0.34)
                })
                .collect();
            painter.add(egui::Shape::line(pts, s));
        }
        IconKind::Keyboard => {
            let box_r = Rect::from_min_max(pt(0.10, 0.24), pt(0.90, 0.76));
            painter.rect_stroke(box_r, CornerRadius::same(2), s, egui::StrokeKind::Inside);
            for x in [0.26, 0.42, 0.58, 0.74] {
                painter.circle_filled(pt(x, 0.44), 1.0, color);
            }
            painter.line_segment([pt(0.30, 0.62), pt(0.70, 0.62)], s);
        }
        IconKind::Server => {
            let top = Rect::from_min_max(pt(0.14, 0.16), pt(0.86, 0.44));
            let bot = Rect::from_min_max(pt(0.14, 0.56), pt(0.86, 0.84));
            painter.rect_stroke(top, CornerRadius::same(2), s, egui::StrokeKind::Inside);
            painter.rect_stroke(bot, CornerRadius::same(2), s, egui::StrokeKind::Inside);
            painter.circle_filled(pt(0.28, 0.30), 1.4, color);
            painter.circle_filled(pt(0.28, 0.70), 1.4, color);
        }
        IconKind::Check => {
            painter.line_segment([pt(0.18, 0.54), pt(0.42, 0.78)], s);
            painter.line_segment([pt(0.42, 0.78), pt(0.84, 0.24)], s);
        }
        IconKind::Warning => {
            let pts = vec![
                pt(0.50, 0.14),
                pt(0.88, 0.84),
                pt(0.12, 0.84),
                pt(0.50, 0.14),
            ];
            painter.add(egui::Shape::line(pts, s));
            painter.line_segment([pt(0.50, 0.38), pt(0.50, 0.58)], s);
            painter.circle_filled(pt(0.50, 0.72), 1.2, color);
        }
    }
}

/// Allocates a square slot of `size` points and paints the database engine logo for `client_key`.
pub fn engine_logo_ui(ui: &mut egui::Ui, client_key: &str, size: f32, theme: &AppTheme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_engine_logo(ui.painter(), rect, client_key, theme);
    }
}

/// Paints a recognizable vector logo for each supported SQL engine inside `rect`.
pub fn paint_engine_logo(painter: &egui::Painter, rect: Rect, client_key: &str, theme: &AppTheme) {
    let r = rect.shrink(1.0);
    let w = r.width();
    let h = r.height();
    let pt = |nx: f32, ny: f32| Pos2::new(r.left() + nx * w, r.top() + ny * h);
    let color = theme.dialect_color(client_key);
    let s = Stroke::new(1.4, color);

    match client_key.to_ascii_lowercase().as_str() {
        "postgresql" | "postgres" => {
            // Elephant head + ear + curved trunk
            let head = vec![
                pt(0.24, 0.44),
                pt(0.24, 0.24),
                pt(0.46, 0.14),
                pt(0.74, 0.20),
                pt(0.84, 0.42),
                pt(0.76, 0.62),
                pt(0.60, 0.64),
                pt(0.60, 0.84),
                pt(0.72, 0.88),
            ];
            painter.add(egui::Shape::line(head, s));
            // Ear
            painter.add(egui::Shape::line(
                vec![
                    pt(0.32, 0.28),
                    pt(0.14, 0.42),
                    pt(0.24, 0.64),
                    pt(0.42, 0.56),
                ],
                s,
            ));
            // Eye
            painter.circle_filled(pt(0.56, 0.36), 1.3, color);
        }
        "mysql" => {
            // Leaping dolphin arc + dorsal fin + tail
            let body = vec![
                pt(0.14, 0.72),
                pt(0.30, 0.36),
                pt(0.56, 0.20),
                pt(0.82, 0.34),
                pt(0.90, 0.52),
            ];
            painter.add(egui::Shape::line(body, s));
            // Dorsal fin
            painter.add(egui::Shape::line(
                vec![pt(0.46, 0.24), pt(0.52, 0.10), pt(0.60, 0.22)],
                s,
            ));
            // Belly & tail fluke
            painter.add(egui::Shape::line(
                vec![pt(0.24, 0.74), pt(0.48, 0.48), pt(0.74, 0.50)],
                Stroke::new(1.2, theme.accent_primary),
            ));
        }
        "mariadb" => {
            // Stylized seal silhouette + wave
            let seal = vec![
                pt(0.18, 0.72),
                pt(0.26, 0.44),
                pt(0.42, 0.20),
                pt(0.58, 0.18),
                pt(0.62, 0.32),
                pt(0.54, 0.52),
                pt(0.76, 0.68),
            ];
            painter.add(egui::Shape::line(seal, s));
            painter.add(egui::Shape::line(
                vec![
                    pt(0.12, 0.82),
                    pt(0.38, 0.74),
                    pt(0.64, 0.82),
                    pt(0.88, 0.74),
                ],
                Stroke::new(1.2, theme.accent_secondary),
            ));
        }
        "redshift" => {
            // Isometric 3D prism / cube
            let hex = vec![
                pt(0.50, 0.10),
                pt(0.86, 0.30),
                pt(0.86, 0.70),
                pt(0.50, 0.90),
                pt(0.14, 0.70),
                pt(0.14, 0.30),
                pt(0.50, 0.10),
            ];
            painter.add(egui::Shape::line(hex, s));
            painter.line_segment([pt(0.50, 0.50), pt(0.50, 0.90)], s);
            painter.line_segment([pt(0.50, 0.50), pt(0.14, 0.30)], s);
            painter.line_segment([pt(0.50, 0.50), pt(0.86, 0.30)], s);
        }
        "sqlserver" => {
            // Cylinder + crimson accent disc
            let top = Rect::from_min_max(pt(0.16, 0.14), pt(0.84, 0.36));
            painter.rect_filled(top, CornerRadius::same(3), color.gamma_multiply(0.30));
            painter.rect_stroke(top, CornerRadius::same(3), s, egui::StrokeKind::Inside);
            painter.line_segment([pt(0.16, 0.25), pt(0.16, 0.80)], s);
            painter.line_segment([pt(0.84, 0.25), pt(0.84, 0.80)], s);
            painter.line_segment([pt(0.16, 0.54), pt(0.84, 0.54)], s);
            painter.line_segment([pt(0.16, 0.80), pt(0.84, 0.80)], s);
        }
        "sqlite" => {
            // Page + diagonal feather quill
            let frame = Rect::from_min_max(pt(0.16, 0.14), pt(0.76, 0.86));
            painter.rect_stroke(frame, CornerRadius::same(2), s, egui::StrokeKind::Inside);
            painter.line_segment([pt(0.32, 0.72), pt(0.86, 0.16)], Stroke::new(1.6, color));
            painter.line_segment([pt(0.54, 0.34), pt(0.78, 0.34)], s);
        }
        "cassandra" => {
            // Eye of Cassandra: almond eye + iris + pupil
            painter.add(egui::Shape::line(
                vec![pt(0.10, 0.50), pt(0.50, 0.22), pt(0.90, 0.50)],
                s,
            ));
            painter.add(egui::Shape::line(
                vec![pt(0.10, 0.50), pt(0.50, 0.78), pt(0.90, 0.50)],
                s,
            ));
            painter.circle_stroke(pt(0.50, 0.50), w * 0.16, s);
            painter.circle_filled(pt(0.50, 0.50), 1.6, color);
        }
        _ => paint_icon(painter, rect, IconKind::Database, color),
    }
}
