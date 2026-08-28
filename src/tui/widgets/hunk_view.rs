//! Interactive hunk pane: a hunk cursor over a scrollable diff, with
//! per-hunk toggling. The hunk data itself is app state passed in per call.

use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Block,
};

use crate::tui::hunk_selector::{FileEntry, HunkEntry, HunkOrigin};
use crate::tui::theme::TuiTheme;
use crate::tui::widgets::common::hunk_line_style;
use crate::tui::widgets::diff_pane::DiffPane;

/// What a hunk-cursor move means for the host app.
pub(crate) enum HunkEvent {
    /// The cursor moved (or stayed) within the current file.
    Handled,
    /// Moving up from the first hunk — the app decides whether a previous
    /// file exists to wrap into.
    WrapToPrevFile,
    /// Moving down from the last hunk — same, for the next file.
    WrapToNextFile,
}

/// Hunk cursor + scroll state for the interactive diff pane.
pub(crate) struct HunkView {
    index: usize,
    pane: DiffPane,
}

impl HunkView {
    pub fn new() -> Self {
        HunkView {
            index: 0,
            pane: DiffPane::new(),
        }
    }

    /// Index of the focused hunk.
    #[cfg(test)]
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn scroll(&self) -> u16 {
        self.pane.scroll()
    }

    pub fn scroll_by(&mut self, delta: i32) {
        self.pane.scroll_by(delta);
    }

    /// Back to the first hunk, scrolled to the top.
    pub fn reset(&mut self) {
        self.index = 0;
        self.pane.reset();
    }

    pub fn move_up(&mut self, hunks: &[HunkEntry]) -> HunkEvent {
        if self.index > 0 {
            self.index -= 1;
            self.scroll_to_hunk(hunks);
            HunkEvent::Handled
        } else {
            HunkEvent::WrapToPrevFile
        }
    }

    pub fn move_down(&mut self, hunks: &[HunkEntry]) -> HunkEvent {
        if self.index + 1 < hunks.len() {
            self.index += 1;
            self.scroll_to_hunk(hunks);
            HunkEvent::Handled
        } else {
            HunkEvent::WrapToNextFile
        }
    }

    /// Focus the last hunk (wrap-to-previous-file target).
    pub fn focus_last(&mut self, hunks: &[HunkEntry]) {
        self.index = hunks.len().saturating_sub(1);
        self.scroll_to_hunk(hunks);
    }

    pub fn toggle_focused(&mut self, hunks: &mut [HunkEntry]) {
        if let Some(h) = hunks.get_mut(self.index) {
            h.selected = !h.selected;
        }
    }

    /// A click on `clicked_line` (0-based content line, scroll already added)
    /// focuses the hunk it falls into and toggles it when on its header row.
    pub fn click(&mut self, hunks: &mut [HunkEntry], clicked_line: usize) {
        let total = hunks.len();
        let mut line: usize = 0;
        for (i, entry) in hunks.iter_mut().enumerate() {
            let hunk_lines = 1 + entry.hunk.text.lines().count();
            let separator = if i + 1 < total { 1 } else { 0 };
            if clicked_line < line + hunk_lines {
                self.index = i;
                if clicked_line == line {
                    entry.selected = !entry.selected;
                }
                return;
            }
            line += hunk_lines + separator;
        }
    }

    /// Scroll so the focused hunk's header is at the top of the pane.
    /// Rows before it: one header line, the hunk lines, one separator each.
    fn scroll_to_hunk(&mut self, hunks: &[HunkEntry]) {
        let row: usize = hunks[..self.index]
            .iter()
            .map(|entry| 2 + entry.hunk.text.lines().count())
            .sum();
        self.pane.set_scroll(u16::try_from(row).unwrap_or(u16::MAX));
    }

    /// Render one file's hunks: a `[✓] Hunk i/n` header per hunk (REVERSED on
    /// the focused one while the pane has focus), colorized hunk lines, blank
    /// separators, and two trailing blanks for breathing room.
    pub fn build_lines(
        &self,
        file: &FileEntry,
        theme: &TuiTheme,
        focused: bool,
    ) -> Vec<Line<'static>> {
        let total = file.hunks.len();
        let mut lines: Vec<Line<'static>> = Vec::new();
        for (i, entry) in file.hunks.iter().enumerate() {
            let marker = if entry.selected { "\u{2713}" } else { " " };
            let origin_label = match entry.origin {
                HunkOrigin::Staged => " (staged)",
                HunkOrigin::Unstaged => "",
                HunkOrigin::Commit => "",
            };
            let header_text = format!("[{}] Hunk {}/{}{}", marker, i + 1, total, origin_label);
            let header_style = if focused && i == self.index {
                theme.hunk_header.add_modifier(Modifier::REVERSED)
            } else {
                theme.hunk_header
            };
            lines.push(Line::from(Span::styled(header_text, header_style)));

            for raw_line in entry.hunk.text.lines() {
                let style = hunk_line_style(raw_line, theme);
                lines.push(Line::from(Span::styled(raw_line.to_string(), style)));
            }

            if i + 1 < total {
                lines.push(Line::from(""));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        lines: Vec<Line<'_>>,
        block: Block<'_>,
    ) {
        self.pane.render(frame, area, lines, block);
    }
}
