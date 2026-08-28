use std::borrow::Cow;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::{ListItem, Paragraph},
};

use crate::core::diff::DiffHunk;
use crate::tui::shell::{KeyResult, PaneId, Shell, ShellApp, ShellConfig};
use crate::tui::theme::TuiTheme;
use crate::tui::widgets::common::pane_block;
use crate::tui::widgets::hunk_view::{HunkEvent, HunkView};
use crate::tui::widgets::list_pane::ListPane;

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
    /// From a commit diff (`git diff <oid>^..<oid>`).
    Commit,
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

/// Why the selector exited.
pub(crate) enum Verdict {
    Confirm,
    Cancel,
}

/// An entry in the display list for the file tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisplayRow {
    /// Directory header grouping files at indices `dir_start..=dir_end`.
    Directory { dir_start: usize, dir_end: usize },
    /// A single file — index into the `files` vec.
    File(usize),
}

/// All state for the interactive hunk selector. `pub(crate)` so a host TUI
/// can embed it as a [`ShellApp`] later.
pub(crate) struct HunkSelectorApp {
    files: Vec<FileEntry>,
    display_rows: Vec<DisplayRow>,
    /// File-list cursor and view state.
    list: ListPane,
    /// Hunk cursor and diff-pane scroll state.
    hunks: HunkView,
    theme: TuiTheme,
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
    pub(crate) fn new(files: Vec<FileEntry>, theme: TuiTheme) -> Self {
        let display_rows = build_display_rows(&files);
        Self {
            files,
            display_rows,
            list: ListPane::new(0),
            hunks: HunkView::new(),
            theme,
        }
    }

    /// Return the file index if the cursor is on a file row, or `None` on a
    /// directory header.
    fn current_file_index(&self) -> Option<usize> {
        match self.display_rows.get(self.list.cursor()) {
            Some(DisplayRow::File(i)) => Some(*i),
            _ => None,
        }
    }

    // -- rendering ----------------------------------------------------------

    fn render_file_list(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
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

        let block = pane_block(" Files ", &self.theme, focused);
        self.list
            .render(frame, area, items, block, self.theme.file_selected);
    }

    fn render_diff_view(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let block = pane_block(" Diff ", &self.theme, focused);

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
                    self.display_rows.get(self.list.cursor())
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
        let lines = self.hunks.build_lines(file, &self.theme, focused);
        self.hunks.render(frame, area, lines, block);
    }

    // -- navigation -----------------------------------------------------------

    /// Move the file-list cursor; the hunk cursor follows to the new file.
    fn move_file_cursor(&mut self, dir: isize) {
        if self
            .list
            .move_cursor(dir, self.display_rows.len(), |_| true)
        {
            self.hunks.reset();
        }
    }

    fn navigate_up(&mut self, pane: PaneId) {
        if self.display_rows.is_empty() {
            return;
        }
        match pane {
            PaneId::Left => self.move_file_cursor(-1),
            PaneId::Right => {
                let Some(file_idx) = self.current_file_index() else {
                    return;
                };
                if let HunkEvent::WrapToPrevFile = self.hunks.move_up(&self.files[file_idx].hunks) {
                    // Move to the last hunk of the previous file.
                    if let Some(prev) = self.prev_file_row() {
                        self.list.set_cursor(prev);
                        let file_idx = match self.display_rows[prev] {
                            DisplayRow::File(i) => i,
                            _ => unreachable!(),
                        };
                        self.hunks.focus_last(&self.files[file_idx].hunks);
                    }
                }
            }
        }
    }

    fn navigate_down(&mut self, pane: PaneId) {
        if self.display_rows.is_empty() {
            return;
        }
        match pane {
            PaneId::Left => self.move_file_cursor(1),
            PaneId::Right => {
                let Some(file_idx) = self.current_file_index() else {
                    return;
                };
                if let HunkEvent::WrapToNextFile = self.hunks.move_down(&self.files[file_idx].hunks)
                {
                    // Move to the first hunk of the next file.
                    if let Some(next) = self.next_file_row() {
                        self.list.set_cursor(next);
                        self.hunks.reset();
                    }
                }
            }
        }
    }

    fn toggle(&mut self, pane: PaneId) {
        if self.display_rows.is_empty() {
            return;
        }
        match pane {
            PaneId::Left => match self.display_rows[self.list.cursor()] {
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
            PaneId::Right => {
                if let Some(file_idx) = self.current_file_index() {
                    self.hunks.toggle_focused(&mut self.files[file_idx].hunks);
                }
            }
        }
    }

    /// Find the previous File row before the cursor, skipping directory headers.
    fn prev_file_row(&self) -> Option<usize> {
        let mut pos = self.list.cursor();
        while pos > 0 {
            pos -= 1;
            if matches!(self.display_rows[pos], DisplayRow::File(_)) {
                return Some(pos);
            }
        }
        None
    }

    /// Find the next File row after the cursor, skipping directory headers.
    fn next_file_row(&self) -> Option<usize> {
        let mut pos = self.list.cursor();
        while pos + 1 < self.display_rows.len() {
            pos += 1;
            if matches!(self.display_rows[pos], DisplayRow::File(_)) {
                return Some(pos);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Shell integration
// ---------------------------------------------------------------------------

impl ShellApp for HunkSelectorApp {
    type Exit = Verdict;

    fn config(&self) -> ShellConfig {
        // Two panes: ~30% file list, ~70% diff view.
        ShellConfig { split: (30, 70) }
    }

    fn theme(&self) -> &TuiTheme {
        &self.theme
    }

    fn quit_exit(&mut self) -> Verdict {
        Verdict::Cancel
    }

    fn handle_key(
        &mut self,
        focused: PaneId,
        code: KeyCode,
        _modifiers: KeyModifiers,
    ) -> KeyResult<Verdict> {
        match code {
            KeyCode::Esc => KeyResult::Exit(Verdict::Cancel),
            // Ctrl-C never reaches here — the shell intercepts it first.
            KeyCode::Char('c') | KeyCode::Enter => KeyResult::Exit(Verdict::Confirm),
            KeyCode::Up | KeyCode::Char('k') => {
                self.navigate_up(focused);
                KeyResult::Handled
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.navigate_down(focused);
                KeyResult::Handled
            }
            KeyCode::Char(' ') => {
                self.toggle(focused);
                KeyResult::Handled
            }
            _ => KeyResult::Handled,
        }
    }

    fn handle_mouse(&mut self, pane: PaneId, kind: MouseEventKind, pos: Position, area: Rect) {
        match pane {
            PaneId::Left => match kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let Some(clicked) = self.list.hit_test(area, pos.y) else {
                        return;
                    };
                    if clicked < self.display_rows.len() {
                        self.list.set_cursor(clicked);
                        self.hunks.reset();
                    }
                }
                MouseEventKind::ScrollUp => self.move_file_cursor(-1),
                MouseEventKind::ScrollDown => self.move_file_cursor(1),
                _ => {}
            },
            PaneId::Right => match kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let Some(file_idx) = self.current_file_index() else {
                        return;
                    };
                    // Inner area: subtract 1-row border top, then account for scroll.
                    let inner_top = area.y + 1;
                    let inner_height = area.height.saturating_sub(2);
                    if pos.y < inner_top || pos.y >= inner_top + inner_height {
                        return;
                    }
                    let clicked_line = (pos.y - inner_top) as usize + self.hunks.scroll() as usize;
                    self.hunks
                        .click(&mut self.files[file_idx].hunks, clicked_line);
                }
                MouseEventKind::ScrollUp => self.hunks.scroll_by(-3),
                MouseEventKind::ScrollDown => self.hunks.scroll_by(3),
                _ => {}
            },
        }
    }

    fn render_pane(&mut self, frame: &mut Frame, pane: PaneId, area: Rect, focused: bool) {
        match pane {
            PaneId::Left => self.render_file_list(frame, area, focused),
            PaneId::Right => self.render_diff_view(frame, area, focused),
        }
    }

    fn status_hints(&self, _focused: PaneId) -> Vec<Cow<'static, str>> {
        vec![
            "Navigate: \u{2191}/\u{2193}".into(),
            "Switch Pane: tab".into(),
            "Toggle: space".into(),
            "Confirm: c or Enter".into(),
            "Quit: q or Esc".into(),
        ]
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
    // Backstop for agent mode — the primary guard rejects `-p` at dispatch
    // time, but any future call path must not open a full-screen TUI either.
    if crate::core::agent_mode::enabled() {
        anyhow::bail!(
            "--patch is interactive and unavailable in agent mode\n\
             Pass explicit files instead"
        );
    }
    if files.is_empty() {
        return Ok(None);
    }

    let (app, verdict) = Shell::new(HunkSelectorApp::new(files, theme)).run()?;
    Ok(match verdict {
        Verdict::Confirm => Some(app.files),
        Verdict::Cancel => None,
    })
}

/// Fixtures shared with the shell tests.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::core::graph::Theme;

    /// A selector over one single-hunk file, for render-level tests.
    pub(crate) fn sample_app() -> HunkSelectorApp {
        let files = vec![FileEntry {
            path: "main.rs".to_string(),
            hunks: vec![HunkEntry {
                hunk: DiffHunk {
                    text: "@@ -1,1 +1,1 @@\n-a\n+b\n".to_string(),
                    modified_lines: vec![],
                },
                selected: false,
                origin: HunkOrigin::Unstaged,
            }],
            index_status: ' ',
            worktree_status: 'M',
            binary: false,
        }];
        HunkSelectorApp::new(files, TuiTheme::from_graph_theme(&Theme::dark()))
    }
}

#[cfg(test)]
#[path = "hunk_selector_test.rs"]
mod tests;
