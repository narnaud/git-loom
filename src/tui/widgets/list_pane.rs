//! Left-pane list with a cursor: selection highlight, scrollbar, click math.

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, List, ListItem, ListState},
};

use crate::tui::widgets::common::render_scrollbar;

/// Cursor + view state for a list pane. The list content is app data passed
/// in per call; this only owns how it is browsed and drawn.
pub(crate) struct ListPane {
    cursor: usize,
    state: ListState,
}

impl ListPane {
    pub fn new(cursor: usize) -> Self {
        ListPane {
            cursor,
            state: ListState::default(),
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, i: usize) {
        self.cursor = i;
    }

    /// Scroll offset of the last render.
    #[cfg(test)]
    pub fn offset(&self) -> usize {
        self.state.offset()
    }

    /// Move the cursor by `dir`, skipping rows for which `focusable` is
    /// false. Returns true if the cursor moved.
    pub fn move_cursor(
        &mut self,
        dir: isize,
        len: usize,
        focusable: impl Fn(usize) -> bool,
    ) -> bool {
        let mut pos = self.cursor as isize;
        loop {
            pos += dir;
            if pos < 0 || pos as usize >= len {
                return false;
            }
            if focusable(pos as usize) {
                self.cursor = pos as usize;
                return true;
            }
        }
    }

    /// Map a clicked screen row to a list index, using the scroll offset of
    /// the last render. Only clicks inside the inner area count — the bottom
    /// border would otherwise map to the row after the last visible one.
    /// The returned index may be past the end of the list.
    pub fn hit_test(&self, area: Rect, screen_row: u16) -> Option<usize> {
        let inner_top = area.y + 1;
        let inner_height = area.height.saturating_sub(2);
        if screen_row < inner_top || screen_row >= inner_top + inner_height {
            return None;
        }
        Some((screen_row - inner_top) as usize + self.state.offset())
    }

    /// Render the list with the cursor row highlighted, plus a scrollbar when
    /// the content overflows.
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        items: Vec<ListItem>,
        block: Block<'_>,
        highlight: Style,
    ) {
        let total = items.len();
        let list = List::new(items).block(block).highlight_style(highlight);
        self.state.select(Some(self.cursor));
        frame.render_stateful_widget(list, area, &mut self.state);

        let inner_height = area.height.saturating_sub(2) as usize;
        render_scrollbar(frame, area, total, inner_height, self.state.offset());
    }
}
