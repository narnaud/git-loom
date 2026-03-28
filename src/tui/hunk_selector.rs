use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::core::diff::DiffHunk;
use crate::tui::theme::TuiTheme;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// Where a hunk came from — determines how to apply/reverse on confirm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HunkOrigin {
    /// From `git diff --cached` (already staged).
    Staged,
    /// From `git diff` (unstaged working-tree change).
    Unstaged,
}

/// A single hunk with a toggle state and origin.
pub(crate) struct HunkEntry {
    pub hunk: DiffHunk,
    pub selected: bool,
    pub origin: HunkOrigin,
}

/// A file and its parsed hunks, with git status information.
pub(crate) struct FileEntry {
    pub path: String,
    pub hunks: Vec<HunkEntry>,
    /// Index (staged) status character: ' ', 'A', 'M', 'D', 'R', or '?'.
    pub index_status: char,
    /// Worktree (unstaged) status character: ' ', 'M', 'D', 'R', '?', or '!'.
    pub worktree_status: char,
    /// Whether this file is binary (no hunk-level patching possible).
    pub binary: bool,
}

impl FileEntry {
    /// Compute the effective status characters based on current hunk selections.
    ///
    /// Returns `(index_char, worktree_char)` reflecting what `git status` would
    /// show if the current selections were applied.
    pub(crate) fn effective_status(&self) -> (char, char) {
        let will_have_staged = self.hunks.iter().any(|h| h.selected);
        let will_have_unstaged = self.hunks.iter().any(|h| !h.selected);

        let is_untracked = self.index_status == '?' && self.worktree_status == '?';

        if is_untracked {
            return if will_have_staged {
                ('A', ' ')
            } else {
                ('?', '?')
            };
        }

        // Staged new file fully deselected → back to untracked.
        if self.index_status == 'A' && !will_have_staged {
            return ('?', '?');
        }

        let eff_index = if will_have_staged {
            match self.index_status {
                'A' | 'M' | 'D' | 'R' => self.index_status,
                _ => match self.worktree_status {
                    'D' => 'D',
                    _ => 'M',
                },
            }
        } else {
            ' '
        };

        let eff_worktree = if will_have_unstaged {
            match self.worktree_status {
                'M' | 'D' => self.worktree_status,
                _ => match self.index_status {
                    'D' => 'D',
                    _ => 'M',
                },
            }
        } else {
            ' '
        };

        (eff_index, eff_worktree)
    }
}

/// Which pane is focused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    Left,
    Right,
}

/// An entry in the display list for the file tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisplayRow {
    /// Directory header grouping files at indices `dir_start..=dir_end`.
    Directory { dir_start: usize, dir_end: usize },
    /// A single file — index into the `files` vec.
    File(usize),
}

/// All state for the interactive hunk selector.
struct HunkSelectorApp {
    files: Vec<FileEntry>,
    display_rows: Vec<DisplayRow>,
    cursor_pos: usize,
    hunk_index: usize,
    active_pane: Pane,
    theme: TuiTheme,
    should_quit: bool,
    confirmed: bool,
    scroll_offset: u16,
}

// ---------------------------------------------------------------------------
// Tree helpers
// ---------------------------------------------------------------------------

/// Extract the directory portion of a path, or `""` for root-level files.
fn directory_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(pos) => &path[..pos],
        None => "",
    }
}

/// Extract just the filename from a path.
fn filename_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(pos) => &path[pos + 1..],
        None => path,
    }
}

/// Build the display row list from sorted file entries, grouping files in the
/// same directory under a directory header.
fn build_display_rows(files: &[FileEntry]) -> Vec<DisplayRow> {
    let mut rows = Vec::new();
    let mut i = 0;
    while i < files.len() {
        let dir = directory_of(&files[i].path);
        if dir.is_empty() {
            // Root-level file — no directory header.
            rows.push(DisplayRow::File(i));
            i += 1;
        } else {
            // Directory group — find all consecutive files with the same parent dir.
            let dir_start = i;
            while i < files.len() && directory_of(&files[i].path) == dir {
                i += 1;
            }
            let dir_end = i - 1;
            rows.push(DisplayRow::Directory { dir_start, dir_end });
            for j in dir_start..=dir_end {
                rows.push(DisplayRow::File(j));
            }
        }
    }
    rows
}

// ---------------------------------------------------------------------------
// App logic
// ---------------------------------------------------------------------------

impl HunkSelectorApp {
    fn new(files: Vec<FileEntry>, theme: TuiTheme) -> Self {
        let display_rows = build_display_rows(&files);
        Self {
            files,
            display_rows,
            cursor_pos: 0,
            hunk_index: 0,
            active_pane: Pane::Left,
            theme,
            should_quit: false,
            confirmed: false,
            scroll_offset: 0,
        }
    }

    /// Return the file index if the cursor is on a file row, or `None` on a
    /// directory header.
    fn current_file_index(&self) -> Option<usize> {
        match self.display_rows.get(self.cursor_pos) {
            Some(DisplayRow::File(i)) => Some(*i),
            _ => None,
        }
    }

    // -- rendering ----------------------------------------------------------

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Reserve one row for the status bar.
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        // Two panes: ~30% file list, ~70% diff view.
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(outer[0]);

        self.render_file_list(frame, panes[0]);
        self.render_diff_view(frame, panes[1]);
        self.render_status_bar(frame, outer[1]);
    }

    fn render_file_list(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .display_rows
            .iter()
            .map(|row| match row {
                DisplayRow::Directory { dir_start, .. } => {
                    let dir = directory_of(&self.files[*dir_start].path);
                    let text = format!("\u{25BC} {}", dir);
                    ListItem::new(Line::from(Span::styled(text, self.theme.file_normal)))
                }
                DisplayRow::File(idx) => {
                    let f = &self.files[*idx];
                    let (eff_idx, eff_wt) = f.effective_status();
                    let in_dir = !directory_of(&f.path).is_empty();
                    let name = if in_dir {
                        filename_of(&f.path)
                    } else {
                        &f.path
                    };
                    let indent = if in_dir { "  " } else { "" };

                    let is_untracked = eff_idx == '?' && eff_wt == '?';
                    let mut spans: Vec<Span> = if is_untracked {
                        vec![Span::styled(
                            format!("{}??", indent),
                            self.theme.untracked_status,
                        )]
                    } else {
                        vec![
                            Span::raw(indent.to_string()),
                            Span::styled(eff_idx.to_string(), self.theme.staged_status),
                            Span::styled(eff_wt.to_string(), self.theme.unstaged_status),
                        ]
                    };
                    let name_style = if eff_idx == '?' || eff_wt == '?' {
                        self.theme.file_normal
                    } else if eff_idx != ' ' && eff_wt == ' ' {
                        self.theme.file_fully_staged
                    } else if eff_idx != ' ' && eff_wt != ' ' {
                        self.theme.file_partially_staged
                    } else {
                        self.theme.file_normal
                    };
                    spans.push(Span::styled(format!(" {}", name), name_style));
                    ListItem::new(Line::from(spans))
                }
            })
            .collect();

        let border_style = if self.active_pane == Pane::Left {
            self.theme.border_active
        } else {
            self.theme.border
        };

        let block = Block::default()
            .title(" Files ")
            .borders(Borders::ALL)
            .border_style(border_style);

        let list = List::new(items)
            .block(block)
            .highlight_style(self.theme.file_selected)
            .highlight_symbol("> ");

        let mut state = ListState::default();
        state.select(Some(self.cursor_pos));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_diff_view(&self, frame: &mut Frame, area: Rect) {
        let border_style = if self.active_pane == Pane::Right {
            self.theme.border_active
        } else {
            self.theme.border
        };

        let block = Block::default()
            .title(" Diff ")
            .borders(Borders::ALL)
            .border_style(border_style);

        if self.files.is_empty() {
            let empty = Paragraph::new("No files").block(block);
            frame.render_widget(empty, area);
            return;
        }

        // Directory header selected — show summary.
        let file_idx = match self.current_file_index() {
            Some(i) => i,
            None => {
                if let Some(DisplayRow::Directory { dir_start, dir_end }) =
                    self.display_rows.get(self.cursor_pos)
                {
                    let count = dir_end - dir_start + 1;
                    let dir = directory_of(&self.files[*dir_start].path);
                    let text = format!("{} file(s) in {}/", count, dir);
                    let p = Paragraph::new(text).block(block);
                    frame.render_widget(p, area);
                } else {
                    let empty = Paragraph::new("No files").block(block);
                    frame.render_widget(empty, area);
                }
                return;
            }
        };

        let file = &self.files[file_idx];
        let total_hunks = file.hunks.len();
        let mut lines: Vec<Line> = Vec::new();

        for (i, entry) in file.hunks.iter().enumerate() {
            let marker = if entry.selected { "\u{2713}" } else { " " };
            let origin_label = match entry.origin {
                HunkOrigin::Staged => " (staged)",
                HunkOrigin::Unstaged => "",
            };
            let header_text = format!(
                "[{}] Hunk {}/{}{}",
                marker,
                i + 1,
                total_hunks,
                origin_label
            );

            // Highlight the focused hunk header when right pane is active.
            let header_style = if self.active_pane == Pane::Right && i == self.hunk_index {
                self.theme.hunk_header.add_modifier(Modifier::REVERSED)
            } else {
                self.theme.hunk_header
            };
            lines.push(Line::from(Span::styled(header_text, header_style)));

            // Render each line of the hunk text with syntax coloring.
            for raw_line in entry.hunk.text.lines() {
                let style = if raw_line.starts_with('+') {
                    self.theme.added
                } else if raw_line.starts_with('-') {
                    self.theme.removed
                } else if raw_line.starts_with("@@") {
                    self.theme.hunk_header
                } else {
                    self.theme.context
                };
                lines.push(Line::from(Span::styled(raw_line.to_string(), style)));
            }

            // Blank separator between hunks.
            if i + 1 < total_hunks {
                lines.push(Line::from(""));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        frame.render_widget(paragraph, area);
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let text = " Navigate: \u{2191}/\u{2193} or j/k | Switch Pane: tab | Toggle: space | Confirm: c or Enter | Quit: q or Esc";
        let bar = Paragraph::new(text).style(self.theme.status_bar);
        frame.render_widget(bar, area);
    }

    // -- keyboard handling --------------------------------------------------

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
                self.confirmed = false;
            }
            KeyCode::Char('c') if !modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                self.confirmed = true;
            }
            KeyCode::Enter => {
                self.should_quit = true;
                self.confirmed = true;
            }
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl-C: quit without staging.
                self.should_quit = true;
                self.confirmed = false;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.active_pane = match self.active_pane {
                    Pane::Left => Pane::Right,
                    Pane::Right => Pane::Left,
                };
            }
            KeyCode::Up | KeyCode::Char('k') => self.navigate_up(),
            KeyCode::Down | KeyCode::Char('j') => self.navigate_down(),
            KeyCode::Char(' ') => self.toggle(),
            _ => {}
        }
    }

    fn navigate_up(&mut self) {
        if self.display_rows.is_empty() {
            return;
        }
        match self.active_pane {
            Pane::Left => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.hunk_index = 0;
                    self.scroll_offset = 0;
                }
            }
            Pane::Right => {
                if self.current_file_index().is_some() && self.hunk_index > 0 {
                    self.hunk_index -= 1;
                    self.adjust_scroll_to_hunk();
                } else if self.current_file_index().is_some() && self.hunk_index == 0 {
                    // Move to the last hunk of the previous file.
                    if let Some(prev) = self.prev_file_row() {
                        self.cursor_pos = prev;
                        let file_idx = match self.display_rows[prev] {
                            DisplayRow::File(i) => i,
                            _ => unreachable!(),
                        };
                        let count = self.files[file_idx].hunks.len();
                        self.hunk_index = count.saturating_sub(1);
                        self.adjust_scroll_to_hunk();
                    }
                }
            }
        }
    }

    fn navigate_down(&mut self) {
        if self.display_rows.is_empty() {
            return;
        }
        match self.active_pane {
            Pane::Left => {
                if self.cursor_pos + 1 < self.display_rows.len() {
                    self.cursor_pos += 1;
                    self.hunk_index = 0;
                    self.scroll_offset = 0;
                }
            }
            Pane::Right => {
                if let Some(file_idx) = self.current_file_index() {
                    let hunk_count = self.files[file_idx].hunks.len();
                    if self.hunk_index + 1 < hunk_count {
                        self.hunk_index += 1;
                        self.adjust_scroll_to_hunk();
                    } else {
                        // Move to the first hunk of the next file.
                        if let Some(next) = self.next_file_row() {
                            self.cursor_pos = next;
                            self.hunk_index = 0;
                            self.scroll_offset = 0;
                        }
                    }
                }
            }
        }
    }

    fn toggle(&mut self) {
        if self.display_rows.is_empty() {
            return;
        }
        match self.active_pane {
            Pane::Left => match self.display_rows[self.cursor_pos] {
                DisplayRow::Directory { dir_start, dir_end } => {
                    // Toggle all hunks in all files under this directory.
                    let any_selected = (dir_start..=dir_end)
                        .any(|i| self.files[i].hunks.iter().any(|h| h.selected));
                    let new_state = !any_selected;
                    for i in dir_start..=dir_end {
                        for h in &mut self.files[i].hunks {
                            h.selected = new_state;
                        }
                    }
                }
                DisplayRow::File(idx) => {
                    // Toggle all hunks in the current file.
                    let any_selected = self.files[idx].hunks.iter().any(|h| h.selected);
                    let new_state = !any_selected;
                    for h in &mut self.files[idx].hunks {
                        h.selected = new_state;
                    }
                }
            },
            Pane::Right => {
                if let Some(file_idx) = self.current_file_index()
                    && let Some(h) = self.files[file_idx].hunks.get_mut(self.hunk_index)
                {
                    h.selected = !h.selected;
                }
            }
        }
    }

    /// Find the previous File row before `cursor_pos`, skipping directory headers.
    fn prev_file_row(&self) -> Option<usize> {
        let mut pos = self.cursor_pos;
        while pos > 0 {
            pos -= 1;
            if matches!(self.display_rows[pos], DisplayRow::File(_)) {
                return Some(pos);
            }
        }
        None
    }

    /// Find the next File row after `cursor_pos`, skipping directory headers.
    fn next_file_row(&self) -> Option<usize> {
        let mut pos = self.cursor_pos;
        while pos + 1 < self.display_rows.len() {
            pos += 1;
            if matches!(self.display_rows[pos], DisplayRow::File(_)) {
                return Some(pos);
            }
        }
        None
    }

    /// Rough scroll adjustment: each hunk header + its lines contribute to the
    /// total row count. We estimate line offsets to keep the focused hunk visible.
    fn adjust_scroll_to_hunk(&mut self) {
        let file_idx = match self.current_file_index() {
            Some(i) => i,
            None => return,
        };
        let file = &self.files[file_idx];
        let mut row: u16 = 0;
        for (i, entry) in file.hunks.iter().enumerate() {
            if i == self.hunk_index {
                break;
            }
            // 1 for the header line, plus content lines, plus 1 separator.
            row += 1 + entry.hunk.text.lines().count() as u16 + 1;
        }
        self.scroll_offset = row;
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the interactive hunk selector TUI.
///
/// Returns `Ok(Some(files))` with updated selection state if the user confirms,
/// or `Ok(None)` if cancelled / empty input.
pub fn run_hunk_selector(files: Vec<FileEntry>, theme: TuiTheme) -> Result<Option<Vec<FileEntry>>> {
    if files.is_empty() {
        return Ok(None);
    }

    let mut terminal = ratatui::init();

    // Panic-safe cleanup: install a hook that restores the terminal before the
    // default handler fires.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        prev_hook(info);
    }));

    let result = run_event_loop(&mut terminal, files, theme);

    // Restore terminal state on normal exit.
    ratatui::restore();

    // Remove our custom panic hook — back to default.
    let _ = std::panic::take_hook();

    result
}

fn run_event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    files: Vec<FileEntry>,
    theme: TuiTheme,
) -> Result<Option<Vec<FileEntry>>> {
    let mut app = HunkSelectorApp::new(files, theme);

    loop {
        terminal.draw(|frame| app.render(frame))?;

        if let Event::Key(key) = event::read()? {
            // On Windows, crossterm fires both Press and Release. Only handle Press.
            if key.kind != KeyEventKind::Press {
                continue;
            }
            app.handle_key(key.code, key.modifiers);
        }

        if app.should_quit {
            return if app.confirmed {
                Ok(Some(app.files))
            } else {
                Ok(None)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::diff::DiffHunk;
    use crate::core::graph::Theme;
    use crate::tui::theme::TuiTheme;

    fn make_hunk(text: &str) -> DiffHunk {
        DiffHunk {
            text: text.to_string(),
            modified_lines: vec![],
        }
    }

    fn make_theme() -> TuiTheme {
        TuiTheme::from_graph_theme(&Theme::dark())
    }

    /// Root-level files (no directory grouping) — keeps existing tests simple.
    fn make_files() -> Vec<FileEntry> {
        vec![
            FileEntry {
                path: "main.rs".to_string(),
                hunks: vec![
                    HunkEntry {
                        hunk: make_hunk("@@ -1,3 +1,4 @@\n context\n-old\n+new\n"),
                        selected: true,
                        origin: HunkOrigin::Staged,
                    },
                    HunkEntry {
                        hunk: make_hunk("@@ -10,2 +11,3 @@\n context\n+added\n"),
                        selected: false,
                        origin: HunkOrigin::Unstaged,
                    },
                ],
                index_status: 'M',
                worktree_status: 'M',
                binary: false,
            },
            FileEntry {
                path: "lib.rs".to_string(),
                hunks: vec![HunkEntry {
                    hunk: make_hunk("@@ -5,2 +5,2 @@\n-old line\n+new line\n"),
                    selected: false,
                    origin: HunkOrigin::Unstaged,
                }],
                index_status: ' ',
                worktree_status: 'M',
                binary: false,
            },
        ]
    }

    /// Files in a subdirectory — for tree-specific tests.
    fn make_files_in_dir() -> Vec<FileEntry> {
        vec![
            FileEntry {
                path: "src/main.rs".to_string(),
                hunks: vec![HunkEntry {
                    hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                    selected: true,
                    origin: HunkOrigin::Staged,
                }],
                index_status: 'M',
                worktree_status: ' ',
                binary: false,
            },
            FileEntry {
                path: "src/lib.rs".to_string(),
                hunks: vec![HunkEntry {
                    hunk: make_hunk("@@ -1,1 +1,1 @@\n-x\n+y\n"),
                    selected: false,
                    origin: HunkOrigin::Unstaged,
                }],
                index_status: ' ',
                worktree_status: 'M',
                binary: false,
            },
        ]
    }

    /// Mix of root-level files and files in directories.
    fn make_files_mixed() -> Vec<FileEntry> {
        vec![
            FileEntry {
                path: "README.md".to_string(),
                hunks: vec![HunkEntry {
                    hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                    selected: false,
                    origin: HunkOrigin::Unstaged,
                }],
                index_status: ' ',
                worktree_status: 'M',
                binary: false,
            },
            FileEntry {
                path: "src/main.rs".to_string(),
                hunks: vec![HunkEntry {
                    hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                    selected: true,
                    origin: HunkOrigin::Staged,
                }],
                index_status: 'M',
                worktree_status: ' ',
                binary: false,
            },
            FileEntry {
                path: "src/lib.rs".to_string(),
                hunks: vec![HunkEntry {
                    hunk: make_hunk("@@ -1,1 +1,1 @@\n-x\n+y\n"),
                    selected: false,
                    origin: HunkOrigin::Unstaged,
                }],
                index_status: ' ',
                worktree_status: 'M',
                binary: false,
            },
        ]
    }

    #[test]
    fn new_initializes_correctly() {
        let files = make_files();
        let app = HunkSelectorApp::new(files, make_theme());
        assert_eq!(app.cursor_pos, 0);
        assert_eq!(app.hunk_index, 0);
        assert_eq!(app.active_pane, Pane::Left);
        assert!(!app.should_quit);
        assert!(!app.confirmed);
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn navigate_files_in_left_pane() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        assert_eq!(app.cursor_pos, 0);

        app.navigate_down();
        assert_eq!(app.cursor_pos, 1);

        // Can't go past the last file.
        app.navigate_down();
        assert_eq!(app.cursor_pos, 1);

        app.navigate_up();
        assert_eq!(app.cursor_pos, 0);

        // Can't go before 0.
        app.navigate_up();
        assert_eq!(app.cursor_pos, 0);
    }

    #[test]
    fn navigate_hunks_in_right_pane() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        app.active_pane = Pane::Right;

        // File 0 has 2 hunks.
        assert_eq!(app.cursor_pos, 0);
        assert_eq!(app.hunk_index, 0);
        app.navigate_down();
        assert_eq!(app.hunk_index, 1);

        // Past last hunk → move to next file, first hunk.
        app.navigate_down();
        assert_eq!(app.cursor_pos, 1);
        assert_eq!(app.hunk_index, 0);

        // File 1 has 1 hunk — can't go further.
        app.navigate_down();
        assert_eq!(app.cursor_pos, 1);
        assert_eq!(app.hunk_index, 0);

        // Up from first hunk of file 1 → last hunk of file 0.
        app.navigate_up();
        assert_eq!(app.cursor_pos, 0);
        assert_eq!(app.hunk_index, 1);

        app.navigate_up();
        assert_eq!(app.hunk_index, 0);

        // Can't go before first hunk of first file.
        app.navigate_up();
        assert_eq!(app.cursor_pos, 0);
        assert_eq!(app.hunk_index, 0);
    }

    #[test]
    fn navigate_hunks_cross_file_with_dir_headers() {
        let mut app = HunkSelectorApp::new(make_files_in_dir(), make_theme());
        app.active_pane = Pane::Right;
        // display_rows: [Dir(0..1), File(0), File(1)]
        // Start on dir header — right pane nav is no-op.
        assert_eq!(app.cursor_pos, 0);
        assert!(app.current_file_index().is_none());

        // Move cursor to file 0 first.
        app.active_pane = Pane::Left;
        app.navigate_down();
        assert_eq!(app.cursor_pos, 1);
        app.active_pane = Pane::Right;

        // File 0 has 1 hunk. Down → should skip dir headers and land on file 1.
        app.navigate_down();
        assert_eq!(app.cursor_pos, 2);
        assert_eq!(app.current_file_index(), Some(1));
        assert_eq!(app.hunk_index, 0);

        // Up from file 1 → back to file 0's last hunk.
        app.navigate_up();
        assert_eq!(app.cursor_pos, 1);
        assert_eq!(app.current_file_index(), Some(0));
        assert_eq!(app.hunk_index, 0); // file 0 has only 1 hunk
    }

    #[test]
    fn toggle_hunk_in_right_pane() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        app.active_pane = Pane::Right;

        assert!(app.files[0].hunks[0].selected);
        app.toggle();
        assert!(!app.files[0].hunks[0].selected);
        app.toggle();
        assert!(app.files[0].hunks[0].selected);
    }

    #[test]
    fn toggle_file_in_left_pane() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        // First file: one staged (selected), one unstaged (not selected) → any_selected=true
        assert!(app.files[0].hunks[0].selected);
        assert!(!app.files[0].hunks[1].selected);

        app.toggle(); // Left pane: deselect all (since any are selected).
        assert!(app.files[0].hunks.iter().all(|h| !h.selected));

        app.toggle(); // Now none selected → select all.
        assert!(app.files[0].hunks.iter().all(|h| h.selected));
    }

    #[test]
    fn quit_sets_flags() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.should_quit);
        assert!(!app.confirmed);
    }

    #[test]
    fn confirm_sets_flags() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        app.handle_key(KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(app.should_quit);
        assert!(app.confirmed);
    }

    #[test]
    fn enter_confirms() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        app.handle_key(KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.should_quit);
        assert!(app.confirmed);
    }

    #[test]
    fn tab_switches_pane() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        assert_eq!(app.active_pane, Pane::Left);
        app.handle_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.active_pane, Pane::Right);
        app.handle_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(app.active_pane, Pane::Left);
    }

    #[test]
    fn switching_file_resets_hunk_and_scroll() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        app.active_pane = Pane::Right;
        app.navigate_down(); // Move to hunk 1
        assert_eq!(app.hunk_index, 1);
        assert!(app.scroll_offset > 0);

        // Switch back to left pane and navigate to next file.
        app.active_pane = Pane::Left;
        app.navigate_down();
        assert_eq!(app.cursor_pos, 1);
        assert_eq!(app.hunk_index, 0);
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn empty_files_returns_none() {
        let result = run_hunk_selector(vec![], make_theme()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn ctrl_c_quits() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app.should_quit);
        assert!(!app.confirmed);
    }

    #[test]
    fn esc_quits() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        app.handle_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.should_quit);
        assert!(!app.confirmed);
    }

    #[test]
    fn hunk_origin_preserved_through_toggle() {
        let mut app = HunkSelectorApp::new(make_files(), make_theme());
        app.active_pane = Pane::Right;

        // First hunk is Staged
        assert_eq!(app.files[0].hunks[0].origin, HunkOrigin::Staged);
        app.toggle();
        // Origin unchanged after toggle
        assert_eq!(app.files[0].hunks[0].origin, HunkOrigin::Staged);
        assert!(!app.files[0].hunks[0].selected);
    }

    // -- effective_status tests -----------------------------------------------

    #[test]
    fn effective_status_staged_only_deselect_some() {
        // M  → deselect one of two staged hunks → MM
        let file = FileEntry {
            path: "f.rs".into(),
            hunks: vec![
                HunkEntry {
                    hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                    selected: true,
                    origin: HunkOrigin::Staged,
                },
                HunkEntry {
                    hunk: make_hunk("@@ -10,1 +10,1 @@\n-c\n+d\n"),
                    selected: false, // deselected
                    origin: HunkOrigin::Staged,
                },
            ],
            index_status: 'M',
            worktree_status: ' ',
            binary: false,
        };
        assert_eq!(file.effective_status(), ('M', 'M'));
    }

    #[test]
    fn effective_status_staged_only_deselect_all() {
        // M  → deselect all → _M
        let file = FileEntry {
            path: "f.rs".into(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                selected: false,
                origin: HunkOrigin::Staged,
            }],
            index_status: 'M',
            worktree_status: ' ',
            binary: false,
        };
        assert_eq!(file.effective_status(), (' ', 'M'));
    }

    #[test]
    fn effective_status_unstaged_only_select_all() {
        // _M → select all → M_
        let file = FileEntry {
            path: "f.rs".into(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                selected: true,
                origin: HunkOrigin::Unstaged,
            }],
            index_status: ' ',
            worktree_status: 'M',
            binary: false,
        };
        assert_eq!(file.effective_status(), ('M', ' '));
    }

    #[test]
    fn effective_status_untracked_select() {
        // ?? → select → A_
        let file = FileEntry {
            path: "new.rs".into(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -0,0 +1,1 @@\n+new\n"),
                selected: true,
                origin: HunkOrigin::Unstaged,
            }],
            index_status: '?',
            worktree_status: '?',
            binary: false,
        };
        assert_eq!(file.effective_status(), ('A', ' '));
    }

    #[test]
    fn effective_status_untracked_no_select() {
        // ?? stays ??
        let file = FileEntry {
            path: "new.rs".into(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -0,0 +1,1 @@\n+new\n"),
                selected: false,
                origin: HunkOrigin::Unstaged,
            }],
            index_status: '?',
            worktree_status: '?',
            binary: false,
        };
        assert_eq!(file.effective_status(), ('?', '?'));
    }

    #[test]
    fn effective_status_new_file_deselect() {
        // A_ → deselect → ??
        let file = FileEntry {
            path: "new.rs".into(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -0,0 +1,1 @@\n+new\n"),
                selected: false,
                origin: HunkOrigin::Staged,
            }],
            index_status: 'A',
            worktree_status: ' ',
            binary: false,
        };
        assert_eq!(file.effective_status(), ('?', '?'));
    }

    #[test]
    fn effective_status_deletion_deselect() {
        // D_ → deselect → _D
        let file = FileEntry {
            path: "old.rs".into(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("(file deleted)"),
                selected: false,
                origin: HunkOrigin::Staged,
            }],
            index_status: 'D',
            worktree_status: ' ',
            binary: false,
        };
        assert_eq!(file.effective_status(), (' ', 'D'));
    }

    // -- tree display tests ---------------------------------------------------

    #[test]
    fn display_rows_root_files_no_headers() {
        let files = make_files();
        let rows = build_display_rows(&files);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], DisplayRow::File(0));
        assert_eq!(rows[1], DisplayRow::File(1));
    }

    #[test]
    fn display_rows_dir_files_have_header() {
        let files = make_files_in_dir();
        let rows = build_display_rows(&files);
        // directory header + 2 files = 3 rows
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0],
            DisplayRow::Directory {
                dir_start: 0,
                dir_end: 1
            }
        );
        assert_eq!(rows[1], DisplayRow::File(0));
        assert_eq!(rows[2], DisplayRow::File(1));
    }

    #[test]
    fn display_rows_mixed_root_and_dir() {
        let files = make_files_mixed();
        let rows = build_display_rows(&files);
        // README.md (root), then ▼ src header, then src/main.rs, src/lib.rs
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], DisplayRow::File(0)); // README.md
        assert_eq!(
            rows[1],
            DisplayRow::Directory {
                dir_start: 1,
                dir_end: 2
            }
        );
        assert_eq!(rows[2], DisplayRow::File(1)); // src/main.rs
        assert_eq!(rows[3], DisplayRow::File(2)); // src/lib.rs
    }

    #[test]
    fn navigate_through_dir_header() {
        let mut app = HunkSelectorApp::new(make_files_in_dir(), make_theme());
        // display_rows: [Dir(0..1), File(0), File(1)]
        assert_eq!(app.cursor_pos, 0);
        assert!(app.current_file_index().is_none()); // on dir header

        app.navigate_down();
        assert_eq!(app.cursor_pos, 1);
        assert_eq!(app.current_file_index(), Some(0)); // on first file

        app.navigate_down();
        assert_eq!(app.cursor_pos, 2);
        assert_eq!(app.current_file_index(), Some(1)); // on second file

        // Can't go past last row.
        app.navigate_down();
        assert_eq!(app.cursor_pos, 2);
    }

    #[test]
    fn toggle_directory_toggles_all_files() {
        let mut app = HunkSelectorApp::new(make_files_in_dir(), make_theme());
        // cursor_pos 0 = dir header
        // File 0: one hunk selected. File 1: one hunk not selected.
        assert!(app.files[0].hunks[0].selected);
        assert!(!app.files[1].hunks[0].selected);

        // Toggle dir: any_selected=true → deselect all.
        app.toggle();
        assert!(!app.files[0].hunks[0].selected);
        assert!(!app.files[1].hunks[0].selected);

        // Toggle again: none selected → select all.
        app.toggle();
        assert!(app.files[0].hunks[0].selected);
        assert!(app.files[1].hunks[0].selected);
    }

    #[test]
    fn right_pane_noop_on_dir_header() {
        let mut app = HunkSelectorApp::new(make_files_in_dir(), make_theme());
        app.active_pane = Pane::Right;

        // On directory header — hunk navigation should be no-op.
        assert_eq!(app.hunk_index, 0);
        app.navigate_down();
        assert_eq!(app.hunk_index, 0);

        // Toggle on dir in right pane should be no-op.
        app.toggle();
        assert!(app.files[0].hunks[0].selected); // unchanged
    }

    #[test]
    fn directory_of_extracts_parent() {
        assert_eq!(directory_of("src/main.rs"), "src");
        assert_eq!(directory_of("a/b/c.rs"), "a/b");
        assert_eq!(directory_of("file.rs"), "");
    }

    #[test]
    fn filename_of_extracts_name() {
        assert_eq!(filename_of("src/main.rs"), "main.rs");
        assert_eq!(filename_of("a/b/c.rs"), "c.rs");
        assert_eq!(filename_of("file.rs"), "file.rs");
    }
}
