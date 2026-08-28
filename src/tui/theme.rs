use ratatui::style::{Color, Modifier, Style};

use crate::core::graph;

/// TUI color theme derived from the graph theme, providing styles for the
/// interactive hunk selector and other TUI components.
pub struct TuiTheme {
    /// Added lines in diffs (`+`).
    pub added: Style,
    /// Removed lines in diffs (`-`).
    pub removed: Style,
    /// Context (unchanged) lines in diffs.
    pub context: Style,
    /// Hunk headers (`@@ ... @@`).
    pub hunk_header: Style,
    /// Cursor/selected line (file list, status tree): a background shade
    /// clearly distinct from the terminal background; dim spans on that line
    /// use [`TuiTheme::dim_selected`] to stay readable.
    pub file_selected: Style,
    /// Normal (unselected) item in the file list.
    pub file_normal: Style,
    /// Status bar at the bottom.
    pub status_bar: Style,
    /// Border lines around panes.
    pub border: Style,
    /// Border for the active (focused) pane.
    pub border_active: Style,
    /// Staged (index) status character color (green).
    pub staged_status: Style,
    /// Unstaged (worktree) status character color (red).
    pub unstaged_status: Style,
    /// Untracked file status color (red, same as unstaged).
    pub untracked_status: Style,
    /// File name when fully staged (green).
    pub file_fully_staged: Style,
    /// File name when partially staged (yellow/orange).
    pub file_partially_staged: Style,
    /// Graph structure: lines, connectors, dots on the integration line.
    pub graph: Style,
    /// Branch names in brackets.
    pub branch: Style,
    /// Labels like (upstream) and [local changes].
    pub label: Style,
    /// Dimmed secondary text.
    pub dim: Style,
    /// Dimmed secondary text on the cursor/selected line.
    pub dim_selected: Style,
    /// Commit message text.
    pub message: Style,
    /// Short ID prefix (underlined).
    pub shortid: Style,
    /// Untracked file marker.
    pub untracked: Style,
    /// Remote tracking indicators.
    pub remote_synced: Style,
    pub remote_ahead: Style,
    pub remote_gone: Style,
    /// Marker for multi-selected rows in the status tree.
    pub selection: Style,
    /// Rotating colors for commit dots on feature branches.
    pub branch_dots: Vec<Style>,
}

impl TuiTheme {
    /// Build a TUI theme from the graph theme used in status output.
    pub fn from_graph_theme(theme: &graph::Theme) -> Self {
        TuiTheme {
            added: Style::default().fg(map_color(theme.staged)),
            removed: Style::default().fg(map_color(theme.unstaged)),
            context: Style::default().fg(map_color(theme.dim)),
            hunk_header: Style::default().fg(Color::Blue),
            file_selected: Style::default().bg(map_color(theme.selection_bg)),
            file_normal: Style::default(),
            status_bar: Style::default().fg(map_color(theme.dim)),
            border: Style::default().fg(map_color(theme.graph)),
            border_active: Style::default()
                .fg(map_color(theme.message))
                .add_modifier(Modifier::BOLD),
            staged_status: Style::default().fg(map_color(theme.staged)),
            unstaged_status: Style::default().fg(map_color(theme.unstaged)),
            untracked_status: Style::default().fg(map_color(theme.unstaged)),
            file_fully_staged: Style::default().fg(map_color(theme.staged)),
            file_partially_staged: Style::default().fg(Color::Yellow),
            graph: Style::default().fg(map_color(theme.graph)),
            branch: Style::default()
                .fg(map_color(theme.branch))
                .add_modifier(Modifier::BOLD),
            label: Style::default().fg(map_color(theme.label)),
            dim: Style::default().fg(map_color(theme.dim)),
            dim_selected: Style::default().fg(map_color(theme.dim_selected)),
            message: Style::default().fg(map_color(theme.message)),
            shortid: Style::default()
                .fg(map_color(theme.shortid))
                .add_modifier(Modifier::UNDERLINED),
            untracked: Style::default().fg(map_color(theme.untracked)),
            remote_synced: Style::default().fg(map_color(theme.remote_synced)),
            remote_ahead: Style::default().fg(map_color(theme.remote_ahead)),
            remote_gone: Style::default().fg(map_color(theme.remote_gone)),
            selection: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            branch_dots: theme
                .branch_dots
                .iter()
                .map(|c| Style::default().fg(map_color(*c)))
                .collect(),
        }
    }
}

/// Map a `colored::Color` to a `ratatui::style::Color`.
fn map_color(c: colored::Color) -> Color {
    match c {
        colored::Color::Black => Color::Black,
        colored::Color::Red => Color::Red,
        colored::Color::Green => Color::Green,
        colored::Color::Yellow => Color::Yellow,
        colored::Color::Blue => Color::Blue,
        colored::Color::Magenta => Color::Magenta,
        colored::Color::Cyan => Color::Cyan,
        colored::Color::White => Color::White,
        colored::Color::BrightBlack => Color::DarkGray,
        colored::Color::BrightRed => Color::LightRed,
        colored::Color::BrightGreen => Color::LightGreen,
        colored::Color::BrightYellow => Color::LightYellow,
        colored::Color::BrightBlue => Color::LightBlue,
        colored::Color::BrightMagenta => Color::LightMagenta,
        colored::Color::BrightCyan => Color::LightCyan,
        colored::Color::BrightWhite => Color::White,
        colored::Color::AnsiColor(n) => Color::Indexed(n),
        colored::Color::TrueColor { r, g, b } => Color::Rgb(r, g, b),
    }
}
