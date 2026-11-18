//! Autocomplete strip under the SQL editor: schema-aware suggestions from the metadata cache.
//!
//! Suggestions appear as you type (Live Auto Complete) or on Ctrl+Space. Tab or a click
//! inserts one; Esc hides them. Both Settings toggles are honored.

use crate::core::completion::CompletionKind;
use crate::gui::RustSqlGuiApp;
use eframe::egui::{self, text::CCursor, text_edit::TextEditState, RichText};

/// Most suggestions shown at once.
const SHOWN: usize = 12;

impl RustSqlGuiApp {
    /// Recomputes suggestions for `text` at char index `cursor`.
    pub(crate) fn refresh_completions(
        &mut self,
        db: &str,
        text: &str,
        cursor: usize,
        explicit: bool,
    ) {
        let rt = self.rt.clone();
        rt.block_on(self.store.load_completion_columns(db, text));
        let c = self.store.completions(db, text, cursor, explicit);
        self.completion = (!c.items.is_empty()).then_some(c);
    }

    /// Draws the suggestions. Returns the label to insert (clicked, or the first on Tab).
    pub(crate) fn render_completion_strip(
        &mut self,
        ui: &mut egui::Ui,
        accept_first: bool,
    ) -> Option<String> {
        let c = self.completion.as_ref()?;
        if accept_first {
            return c.items.first().map(|i| i.label.clone());
        }
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Suggestions:").weak());
            for item in c.items.iter().take(SHOWN) {
                let tag = match item.kind {
                    CompletionKind::Column => "col",
                    CompletionKind::Table => "table",
                    CompletionKind::Keyword => "kw",
                };
                if ui.small_button(&item.label).on_hover_text(tag).clicked() {
                    picked = Some(item.label.clone());
                }
            }
        });
        picked
    }

    /// Replaces the partial word with `label` and moves the editor cursor after it.
    pub(crate) fn apply_completion(
        &mut self,
        ctx: &egui::Context,
        id: egui::Id,
        text: &str,
        label: &str,
    ) {
        let Some(c) = self.completion.take() else {
            return;
        };
        let chars: Vec<char> = text.chars().collect();
        let end = (c.start + c.prefix.chars().count()).min(chars.len());
        let start = c.start.min(end);
        let mut new_text: String = chars[..start].iter().collect();
        new_text.push_str(label);
        new_text.extend(&chars[end..]);
        self.store.update_query_if_needed(&new_text, None, None);
        if let Some(mut state) = TextEditState::load(ctx, id) {
            let at = CCursor::new(start + label.chars().count());
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(at)));
            state.store(ctx, id);
        }
    }
}
