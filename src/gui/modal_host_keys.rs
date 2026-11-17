//! SSH bastion host-key approval prompt and the trusted host-key manager (Settings).

use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, Color32, RichText};

impl RustSqlGuiApp {
    /// Shows the fingerprint of an unknown or expiring bastion key and asks for approval.
    pub(crate) fn render_host_key_prompt(&mut self, ctx: &egui::Context) {
        let Some(challenge) = self.store.connections.host_key_prompt.clone() else {
            return;
        };
        egui::Window::new("Verify SSH host key")
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(challenge.prompt_text());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Fingerprint:");
                    ui.monospace(RichText::new(&challenge.fingerprint_sha256).strong());
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Trust this key and connect").clicked() {
                        match self.store.approve_host_key() {
                            Ok(Some(server_id)) => self.connect_to_server(&server_id, None, None),
                            Ok(None) => {}
                            Err(e) => self.store.connections.error = Some(e),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.store.reject_host_key();
                    }
                });
            });
    }

    /// Lists trusted bastion host keys and lets the user delete a pin (needed after rotation).
    pub(crate) fn render_host_keys_manager(&mut self, ctx: &egui::Context) {
        if !self.show_host_keys_modal {
            return;
        }
        let pins = match self.store.list_host_keys() {
            Ok(p) => p,
            Err(e) => {
                self.store.status = e;
                Vec::new()
            }
        };
        let mut open = true;
        let mut delete: Option<(String, u32)> = None;
        egui::Window::new("Trusted SSH host keys")
            .open(&mut open)
            .resizable(true)
            .default_width(620.0)
            .show(ctx, |ui| {
                ui.label(
                    "Keys are re-confirmed every 180 days. Delete a pin only after your \
                     administrator confirms the bastion's key was rotated.",
                );
                ui.separator();
                if pins.is_empty() {
                    ui.label("No host keys trusted yet.");
                }
                egui::Grid::new("host_keys_grid")
                    .striped(true)
                    .show(ui, |ui| {
                        for pin in &pins {
                            ui.label(format!("{}:{}", pin.host, pin.port));
                            ui.monospace(format!(
                                "{} {}",
                                pin.key_algorithm, pin.fingerprint_sha256
                            ));
                            ui.label(pin.approved_at.date().to_string());
                            if ui
                                .button(RichText::new("Delete").color(Color32::LIGHT_RED))
                                .clicked()
                            {
                                delete = Some((pin.host.clone(), pin.port));
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some((host, port)) = delete {
            if let Err(e) = self.store.delete_host_key(&host, port) {
                self.store.status = e;
            }
        }
        self.show_host_keys_modal = open;
    }
}
