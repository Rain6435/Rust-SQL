//! Drawing the TUI import dialog (see [`crate::tui::import`]).

use crate::core::import::PREVIEW_ROWS;
use crate::core::params::ParamValue;
use crate::tui::TuiApp;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

/// Longest preview cell shown before it is cut.
const CELL_CHARS: usize = 18;

fn cell(v: &ParamValue) -> String {
    let s = v.display_text().replace(['\n', '\r'], " ");
    if s.chars().count() > CELL_CHARS {
        format!("{}…", s.chars().take(CELL_CHARS - 1).collect::<String>())
    } else {
        s
    }
}

/// A rectangle covering most of `area`, centered.
fn centered(area: Rect) -> Rect {
    let w = area.width.saturating_sub(8).max(20).min(area.width);
    let h = area.height.saturating_sub(2).max(8).min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

impl TuiApp {
    /// Draws the import dialog over `area` when it is open.
    pub(crate) fn render_import(&self, f: &mut ratatui::Frame, area: Rect) {
        let Some(st) = self.import.as_ref() else {
            return;
        };
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let mut lines: Vec<Line> = Vec::new();
        let editing = st.data.is_none();
        lines.push(Line::styled(
            format!(
                "File (.csv, .tsv, .json, .jsonl): {}{}",
                st.path,
                if editing { "█" } else { "" }
            ),
            if editing {
                bold.fg(Color::Yellow)
            } else {
                bold
            },
        ));
        match &st.message {
            Some(Ok(m)) => lines.push(Line::styled(m.clone(), Style::default().fg(Color::Green))),
            Some(Err(e)) => lines.push(Line::styled(e.clone(), Style::default().fg(Color::Red))),
            None => lines.push(Line::from("")),
        }
        if self.store.read_only_mode {
            lines.push(Line::styled(
                "Read-Only mode: import is disabled.",
                Style::default().fg(Color::Red),
            ));
        }
        let keys = if let Some(data) = &st.data {
            lines.push(Line::from(format!(
                "{} rows, {} of {} columns mapped. Source column -> target column (first row):",
                data.rows.len(),
                st.mappings().len(),
                data.columns.len()
            )));
            for (i, name) in data.columns.iter().enumerate() {
                let target = st.mapping.get(i).cloned().flatten();
                let sample = data.rows.first().and_then(|r| r.get(i)).map(cell);
                let marker = if i == st.selected { "▶" } else { " " };
                let style = if i == st.selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                lines.push(Line::styled(
                    format!(
                        "{marker} {name:<20} -> {:<20} {}",
                        target.as_deref().unwrap_or("(skip)"),
                        sample.unwrap_or_default()
                    ),
                    style,
                ));
            }
            lines.push(Line::styled("Preview", bold));
            for row in data.preview(PREVIEW_ROWS.min(5)) {
                let cells: Vec<String> = row.iter().map(cell).collect();
                lines.push(Line::from(format!("  {}", cells.join(" | "))));
            }
            if data.requires_confirmation() {
                lines.push(Line::styled(
                    format!(
                        "[{}] I confirm importing all {} rows (press y)",
                        if st.confirmed { "x" } else { " " },
                        data.rows.len()
                    ),
                    Style::default().fg(Color::Magenta),
                ));
            }
            " [Up/Down] Column  [Left/Right] Target  [y] Confirm  [Enter] Import  [e] Path  [Esc] Close "
        } else {
            " [Enter] Load file  [Esc] Close "
        };
        let target = match &st.table.schema {
            Some(s) => format!("{s}.{}", st.table.name),
            None => st.table.name.clone(),
        };
        let popup = centered(area);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title(format!(" Import into {target} "))
                    .title_bottom(keys),
            ),
            popup,
        );
    }
}
