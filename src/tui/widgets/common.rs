//! Small shared rendering helpers: pane frames, scrollbars, diff coloring.

use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::tui::theme::TuiTheme;

/// Bordered block for a pane, with the active border style when focused.
pub(crate) fn pane_block(title: &str, theme: &TuiTheme, focused: bool) -> Block<'static> {
    let border_style = if focused {
        theme.border_active
    } else {
        theme.border
    };
    Block::default()
        .title(title.to_string())
        .borders(Borders::ALL)
        .border_style(border_style)
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

/// Colorize plain diff text by line prefix.
///
/// `git show` output opens with a header (commit line, author, date, the
/// commit message, diffstat) whose lines would otherwise be styled as dim
/// context — the message is indented like context lines. Everything from
/// the start of a `commit `-led text up to the first `diff --git` renders
/// in normal terminal colors instead, with only the commit line dimmed.
pub(crate) fn colorize_diff(text: &str, theme: &TuiTheme) -> Vec<Line<'static>> {
    let mut in_header = text.starts_with("commit ");
    text.lines()
        .map(|raw| {
            if in_header && raw.starts_with("diff --git") {
                in_header = false;
            }
            let style = if in_header {
                if raw.starts_with("commit ") {
                    theme.dim
                } else {
                    Style::default()
                }
            } else {
                diff_line_style(raw, theme)
            };
            Line::from(Span::styled(raw.to_string(), style))
        })
        .collect()
}

/// Style for one line of full `git diff`/`git show` output.
pub(crate) fn diff_line_style(line: &str, theme: &TuiTheme) -> Style {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff --git") {
        theme.message.add_modifier(Modifier::BOLD)
    } else if line.starts_with("index ") {
        theme.dim
    } else if line.starts_with('+') {
        theme.added
    } else if line.starts_with('-') {
        theme.removed
    } else if line.starts_with("@@") {
        theme.hunk_header
    } else {
        theme.context
    }
}

/// Style for one line inside a hunk body. Distinct from [`diff_line_style`]:
/// hunk bodies carry no file headers, so a body line whose content begins
/// with `++` (from an added line reading `++ x`) must stay an added line.
pub(crate) fn hunk_line_style(line: &str, theme: &TuiTheme) -> Style {
    if line.starts_with('+') {
        theme.added
    } else if line.starts_with('-') {
        theme.removed
    } else if line.starts_with("@@") {
        theme.hunk_header
    } else {
        theme.context
    }
}
