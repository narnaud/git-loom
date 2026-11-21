//! Small shared rendering helpers: pane frames, scrollbars, popup placement,
//! backtick highlighting, diff coloring.

use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::session::diff_text::{self, DiffLine, expand_tabs};
use crate::tui::theme::TuiTheme;

/// Bordered block for a pane, with the active border style when focused.
pub(crate) fn pane_block(title: &str, theme: &TuiTheme, focused: bool) -> Block<'static> {
    pane_block_titled(Line::from(title.to_string()), theme, focused)
}

/// [`pane_block`] with a styled title.
pub(crate) fn pane_block_titled(
    title: Line<'static>,
    theme: &TuiTheme,
    focused: bool,
) -> Block<'static> {
    let border_style = if focused {
        theme.border_active
    } else {
        theme.border
    };
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style)
}

/// Centered rect of at most `width`×`height` inside `area`.
pub(crate) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Style `text` with backtick-quoted words highlighted, like the CLI's yellow.
pub(crate) fn highlight_backticks(text: &str, base: Style, theme: &TuiTheme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('`') else { break };
        if start > 0 {
            spans.push(Span::styled(rest[..start].to_string(), base));
        }
        spans.push(Span::styled(after[..end].to_string(), theme.highlight));
        rest = &after[end + 1..];
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), base));
    }
    spans
}

/// Vertical scrollbar on the right edge of `area` for content `total` rows
/// tall with `viewport` visible rows, scrolled to `position`. No-op when the
/// content fits.
///
/// Ratatui maps the thumb to the bottom when position = content_length - 1.
/// Setting content_length = max_pos + 1 makes position = max_pos land there,
/// and gives thumb_size = track * viewport / total (the correct visible
/// fraction).
pub(crate) fn render_scrollbar(
    frame: &mut Frame,
    area: Rect,
    total: usize,
    viewport: usize,
    position: usize,
) {
    if total <= viewport {
        return;
    }
    let max_pos = total - viewport;
    let mut state = ScrollbarState::new(max_pos + 1)
        .position(position.min(max_pos))
        .viewport_content_length(viewport);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(None),
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        }),
        &mut state,
    );
}

/// Colorize plain diff text by line kind, tabs expanded per the theme.
pub(crate) fn colorize_diff(text: &str, theme: &TuiTheme) -> Vec<Line<'static>> {
    diff_text::classify(text)
        .map(|(kind, raw)| {
            Line::from(Span::styled(
                expand_tabs(raw, theme.tab_width),
                diff_style(kind, theme),
            ))
        })
        .collect()
}

pub(crate) fn diff_style(kind: DiffLine, theme: &TuiTheme) -> Style {
    match kind {
        DiffLine::FileHeader => theme.message.add_modifier(Modifier::BOLD),
        DiffLine::Index | DiffLine::CommitLine => theme.dim,
        DiffLine::Added => theme.added,
        DiffLine::Removed => theme.removed,
        DiffLine::HunkHeader => theme.hunk_header,
        DiffLine::Context => theme.context,
        DiffLine::CommitHeader => Style::default(),
    }
}
