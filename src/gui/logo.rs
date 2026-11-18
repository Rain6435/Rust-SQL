//! Embedded application logo loading for the native Desktop GUI.

use eframe::egui::{self, Color32};

pub(crate) const APP_LOGO_PNG: &[u8] = include_bytes!("../../assets/logo.png");

/// Decodes the embedded PNG logo into `egui::IconData` for the native window viewport.
pub(crate) fn load_app_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(APP_LOGO_PNG).ok()
}

/// Renders the application logo in the GUI header, lazily uploading the texture once.
pub(crate) fn render_header_logo(ui: &mut egui::Ui, cache: &mut Option<egui::TextureHandle>) {
    let logo_tex = cache.get_or_insert_with(|| {
        let image = load_app_icon()
            .map(|icon| {
                egui::ColorImage::from_rgba_unmultiplied(
                    [icon.width as usize, icon.height as usize],
                    &icon.rgba,
                )
            })
            .unwrap_or_else(|| egui::ColorImage::filled([1, 1], Color32::TRANSPARENT));
        ui.ctx()
            .load_texture("rust-sql-logo", image, egui::TextureOptions::LINEAR)
    });
    ui.add(
        egui::Image::new(&*logo_tex)
            .max_size(egui::vec2(22.0, 22.0))
            .corner_radius(4.0),
    );
}

#[cfg(test)]
mod tests {
    use super::load_app_icon;

    #[test]
    fn app_logo_decodes_to_valid_icon_data() {
        let icon = load_app_icon().expect("embedded assets/logo.png should decode");
        assert_eq!(icon.width, 512);
        assert_eq!(icon.height, 512);
        assert_eq!(icon.rgba.len(), 512 * 512 * 4);
    }
}
