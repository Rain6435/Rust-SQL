//! SSH bastion host-key approval prompt and the trusted host-key manager (Settings).

use crate::gui::icons::{icon_primary_button, IconKind};
use crate::gui::theme::{modal_section, subtle_button};
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, RichText};

impl RustSqlGuiApp {
    /// Shows the fingerprint of an unknown or expiring bastion key and asks for approval.
    pub(crate) fn render_host_key_prompt(&mut self, ctx: &egui::Context) {
        let Some(challenge) = self.store.connections.host_key_prompt.clone() else {
            return;
        };
        let theme = self.current_theme();
        egui::Window::new("Verify SSH Host Key")
            .collapsible(false)
            .resizable(false)
            .default_width(540.0)
            .show(ctx, |ui| {
                modal_section(
                    ui,
                    &theme,
                    IconKind::Warning,
                    "Bastion Key Verification",
                    |ui| {
                        ui.label(challenge.prompt_text());
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Fingerprint:");
                            ui.monospace(
                                RichText::new(&challenge.fingerprint_sha256)
                                    .strong()
                                    .color(theme.accent_secondary),
                            );
                        });
                    },
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_primary_button(
                            ui,
                            &theme,
                            IconKind::Check,
                            "Trust this key and connect",
                        )
                        .clicked()
                        {
                            match self.store.approve_host_key() {
                                Ok(Some(server_id)) => {
                                    self.connect_to_server(&server_id, None, None)
                                }
                                Ok(None) => {}
                                Err(e) => self.store.connections.error = Some(e),
                            }
                        }
                        if ui.add(subtle_button(&theme, "Cancel")).clicked() {
                            self.store.reject_host_key();
                        }
                    });
                });
            });
    }

    /// Lists trusted bastion host keys and lets the user delete a pin (needed after rotation).
    pub(crate) fn render_host_keys_manager(&mut self, ctx: &egui::Context) {
        if !self.show_host_keys_modal {
            return;
        }
        let theme = self.current_theme();
        let pins = match self.store.list_host_keys() {
            Ok(p) => p,
            Err(e) => {
                self.store.status = e;
                Vec::new()
            }
        };
        let mut open = true;
        let mut delete: Option<(String, u32)> = None;
        egui::Window::new("Trusted SSH Host Keys")
            .open(&mut open)
            .resizable(true)
            .default_width(640.0)
            .show(ctx, |ui| {
                modal_section(ui, &theme, IconKind::Lock, "Pinned Keys", |ui| {
                    ui.label(
                        RichText::new(
                            "Keys are re-confirmed every 180 days. Delete a pin only after your \
                             administrator confirms the bastion's key was rotated.",
                        )
                        .small()
                        .color(theme.text_muted),
                    );
                    ui.add_space(4.0);
                    if pins.is_empty() {
                        ui.label(
                            RichText::new("No host keys trusted yet.").color(theme.text_muted),
                        );
                    }
                    egui::Grid::new("host_keys_grid")
                        .striped(true)
                        .spacing([14.0, 6.0])
                        .show(ui, |ui| {
                            for pin in &pins {
                                ui.strong(format!("{}:{}", pin.host, pin.port));
                                ui.monospace(format!(
                                    "{} {}",
                                    pin.key_algorithm, pin.fingerprint_sha256
                                ));
                                ui.label(pin.approved_at.date().to_string());
                                if ui
                                    .small_button(RichText::new("Delete").color(theme.semantic_err))
                                    .clicked()
                                {
                                    delete = Some((pin.host.clone(), pin.port));
                                }
                                ui.end_row();
                            }
                        });
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
