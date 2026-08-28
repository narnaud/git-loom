//! Scrollable text pane: clamped scroll, paragraph rendering, scrollbar.

use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Block, Paragraph},
};

use crate::tui::widgets::common::render_scrollbar;

/// Scroll state for a text pane. Long lines clip; there is no wrapping.
pub(crate) struct DiffPane {
    scroll: u16,
    /// Visible content height of the last render, for page scrolling.
    page_height: u16,
}

impl DiffPane {
    pub fn new() -> Self {
        DiffPane {
            scroll: 0,
            page_height: 0,
        }
    }

    pub fn scroll(&self) -> u16 {
        self.scroll
    }

    pub fn set_scroll(&mut self, scroll: u16) {
        self.scroll = scroll;
    }

    /// Scroll by a signed amount, saturating at both ends (the render clamps
    /// to the content height).
    pub fn scroll_by(&mut self, delta: i32) {
        self.scroll = if delta < 0 {
            self.scroll.saturating_sub(delta.unsigned_abs() as u16)
        } else {
            self.scroll.saturating_add(delta as u16)
        };
    }

    pub fn reset(&mut self) {
        self.scroll = 0;
    }

    /// Scroll by one page (the height rendered last frame), up or down.
    pub fn scroll_page(&mut self, dir: i32) {
        self.scroll_by(dir * self.page_height.max(1) as i32);
    }

    /// Render `lines` scrolled, clamped so the last line stays at the bottom,
    /// plus a scrollbar when the content overflows.
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        lines: Vec<Line<'_>>,
        block: Block<'_>,
    ) {
        let inner_height = area.height.saturating_sub(2) as usize;
        self.page_height = inner_height as u16;
        let total = lines.len();
        let max_scroll = u16::try_from(total.saturating_sub(inner_height)).unwrap_or(u16::MAX);
        self.scroll = self.scroll.min(max_scroll);

        let paragraph = Paragraph::new(lines).block(block).scroll((self.scroll, 0));
        frame.render_widget(paragraph, area);
        render_scrollbar(frame, area, total, inner_height, self.scroll as usize);
    }
}
