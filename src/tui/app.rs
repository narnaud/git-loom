//! Interactive status TUI (`loom tui`): the status tree on the left, the diff
//! of the item under the cursor on the right.
//!
//! Actions (commit, fold, branch, drop, reword) suspend the TUI, run the
//! regular loom command — prompts and editors work as usual — then reload the
//! tree. Fold picks its target in a second step inside the tree itself.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::ListItem,
};

use crate::core::graph::{self, Section};
use crate::core::repo::{self, RemoteStatus};
use crate::core::shortid::IdAllocator;
use crate::core::{msg, transaction};
use crate::git;
use crate::tui::shell::{KeyResult, PaneId, Shell, ShellApp, ShellConfig};
use crate::tui::status_tree::{self, LOCAL_CHANGES_KEY, Row, RowKind};
use crate::tui::theme::TuiTheme;
use crate::tui::widgets::common::{colorize_diff, pane_block};
use crate::tui::widgets::diff_pane::DiffPane;
use crate::tui::widgets::list_pane::ListPane;
use crate::{branch, commit, drop, fold, reword};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// Everything gathered from the repo for one TUI round.
struct Snapshot {
    workdir: PathBuf,
    git_dir: PathBuf,
    cwd_prefix: String,
    sections: Vec<Section>,
    ids: IdAllocator,
}

/// A loom command to run once the TUI is suspended.
enum Action {
    /// `loom commit [files...]` — empty means all tracked changes.
    Commit { files: Vec<String> },
    /// `loom fold <sources...> <target>`.
    Fold {
        sources: Vec<String>,
        target: String,
    },
    /// `loom branch new [-t target]`.
    NewBranch { target: Option<String> },
    /// `loom drop <target>`.
    Drop { target: String },
    /// `loom reword <target>`.
    Reword { target: String },
}

/// Why the event loop returned.
enum Outcome {
    Quit,
    Refresh,
    Run(Action),
}

/// Input mode: normal, or picking the target of a pending fold.
enum Mode {
    Normal,
    FoldTarget { sources: Vec<String> },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the interactive status TUI.
pub fn run(theme: graph::Theme) -> Result<()> {
    // Backstop for agent mode — the primary guard rejects `tui` at dispatch
    // time, but any future call path must not open a full-screen TUI either.
    if crate::core::agent_mode::enabled() {
        anyhow::bail!(
            "the TUI is interactive and unavailable in agent mode\n\
             Use `loom status` instead"
        );
    }

    let tui_theme = TuiTheme::from_graph_theme(&theme);

    // Expansion state survives reloads; local changes start expanded.
    let mut expanded: HashSet<String> = HashSet::new();
    expanded.insert(LOCAL_CHANGES_KEY.to_string());
    let mut cursor_key: Option<String> = None;

    loop {
        let snapshot = load_snapshot()?;
        let git_dir = snapshot.git_dir.clone();

        let (outcome, last_key) =
            run_tui_once(&snapshot, &tui_theme, &mut expanded, cursor_key.take())?;
        cursor_key = last_key;

        match outcome {
            Outcome::Quit => return Ok(()),
            Outcome::Refresh => continue,
            Outcome::Run(action) => {
                if let Err(e) = execute_action(action, &git_dir, &theme) {
                    msg::error(&e.to_string());
                }
                // A conflict pauses the operation; the TUI cannot continue
                // because every other command is blocked until it's resolved.
                if let Ok(Some(state)) = transaction::load(&git_dir) {
                    println!(
                        "\nA `loom {}` is paused due to conflicts.\n\
                         Resolve them, then run `loom continue` to resume, \
                         or `loom abort` to cancel.",
                        state.command
                    );
                    return Ok(());
                }
                wait_for_enter();
            }
        }
    }
}

/// Gather repo info and build the graph sections, exactly like `loom status`
/// with files enabled.
fn load_snapshot() -> Result<Snapshot> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "display status")?.to_path_buf();
    let git_dir = repo.path().to_path_buf();
    let cwd_prefix = repo::cwd_relative_to_repo(&repo).unwrap_or_default();

    let mut info = repo::gather_repo_info(&repo, true, 1)?;
    // Collect entities before filtering so short IDs stay stable.
    let ids = IdAllocator::new(info.collect_entities());
    crate::status::apply_hidden_branches(&repo, &mut info);

    Ok(Snapshot {
        workdir,
        git_dir,
        cwd_prefix,
        sections: graph::build_sections(info),
        ids,
    })
}

/// Run the loom command for `action` while the terminal is in normal mode.
fn execute_action(action: Action, git_dir: &std::path::Path, theme: &graph::Theme) -> Result<()> {
    // Browsing isn't trace-logged (`tui` is excluded in main); actions are.
    // A second action in the same session appends to the same log.
    let name = match &action {
        Action::Commit { .. } => "commit",
        Action::Fold { .. } => "fold",
        Action::NewBranch { .. } => "branch new",
        Action::Drop { .. } => "drop",
        Action::Reword { .. } => "reword",
    };
    crate::trace::init(git_dir, &format!("loom tui: {}", name));
    match action {
        Action::Commit { files } => commit::run(None, None, false, files, theme),
        Action::Fold { sources, target } => {
            let mut args = sources;
            args.push(target);
            fold::run(false, false, args, theme)
        }
        Action::NewBranch { target } => branch::new::run(None, target),
        Action::Drop { target } => drop::run(target, false),
        Action::Reword { target } => reword::run(target, None),
    }
}

/// Let the user read the command output before the TUI redraws over it.
fn wait_for_enter() {
    print!("\nPress Enter to return to loom tui...");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

/// Run the app inside the shell (terminal setup, event loop, restore).
/// Returns the outcome and the key of the row the cursor was on.
fn run_tui_once(
    snapshot: &Snapshot,
    theme: &TuiTheme,
    expanded: &mut HashSet<String>,
    cursor_key: Option<String>,
) -> Result<(Outcome, Option<String>)> {
    let app = App::new(snapshot, theme, expanded.clone(), cursor_key);
    let (app, outcome) = Shell::new(app).run()?;

    *expanded = app.expanded;
    let key = app.rows.get(app.tree.cursor()).map(|r| r.key.clone());
    Ok((outcome, key))
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct App<'a> {
    snapshot: &'a Snapshot,
    theme: &'a TuiTheme,
    rows: Vec<Row>,
    /// Tree pane cursor and view state.
    tree: ListPane,
    /// Keys of the multi-selected rows.
    selected: HashSet<String>,
    expanded: HashSet<String>,
    mode: Mode,
    /// Right-pane scroll state.
    diff: DiffPane,
    /// Diff lines cached per row key.
    diff_cache: HashMap<String, Vec<Line<'static>>>,
    /// Transient message shown in the status bar until the next key.
    notice: Option<String>,
    /// Exit value set by deep handlers, drained after each key.
    outcome: Option<Outcome>,
}

impl<'a> App<'a> {
    fn new(
        snapshot: &'a Snapshot,
        theme: &'a TuiTheme,
        expanded: HashSet<String>,
        cursor_key: Option<String>,
    ) -> Self {
        let rows = status_tree::build_rows(&snapshot.sections, &snapshot.ids, &expanded);
        let cursor = cursor_key
            .and_then(|key| rows.iter().position(|r| r.focusable && r.key == key))
            .or_else(|| rows.iter().position(|r| r.focusable))
            .unwrap_or(0);
        let mut app = App {
            snapshot,
            theme,
            rows,
            tree: ListPane::new(cursor),
            selected: HashSet::new(),
            expanded,
            mode: Mode::Normal,
            diff: DiffPane::new(),
            diff_cache: HashMap::new(),
            notice: None,
            outcome: None,
        };
        // Prime the diff for the initial cursor row so the first render
        // doesn't have to.
        app.ensure_diff_cached();
        app
    }

    fn current_row(&self) -> Option<&Row> {
        self.rows.get(self.tree.cursor())
    }

    /// Rebuild rows after an expansion change, keeping the cursor on `key`.
    fn rebuild_rows(&mut self, key: &str) {
        self.rows =
            status_tree::build_rows(&self.snapshot.sections, &self.snapshot.ids, &self.expanded);
        if let Some(pos) = self.rows.iter().position(|r| r.key == key) {
            self.tree.set_cursor(pos);
        } else if self.tree.cursor() >= self.rows.len() {
            self.tree.set_cursor(self.rows.len().saturating_sub(1));
        }
    }

    // -- keyboard handling ----------------------------------------------------

    /// Esc: cancel fold-target mode, else clear the selection, else quit.
    fn handle_escape(&mut self) {
        if matches!(self.mode, Mode::FoldTarget { .. }) {
            self.mode = Mode::Normal;
            self.notice = Some("fold cancelled".to_string());
        } else if !self.selected.is_empty() {
            self.selected.clear();
        } else {
            self.outcome = Some(Outcome::Quit);
        }
    }

    fn move_cursor(&mut self, dir: isize) {
        if self
            .tree
            .move_cursor(dir, self.rows.len(), |i| self.rows[i].focusable)
        {
            self.diff.reset();
        }
    }

    fn expand_current(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        if row.expandable && !row.expanded {
            let key = row.key.clone();
            self.expanded.insert(key.clone());
            self.rebuild_rows(&key);
        }
    }

    /// Collapse the current row, or jump to (and collapse) its parent when the
    /// cursor is on a child file row.
    fn collapse_current(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        if row.expandable && row.expanded {
            let key = row.key.clone();
            self.expanded.remove(&key);
            self.rebuild_rows(&key);
            return;
        }
        if matches!(
            row.kind,
            RowKind::WorkingFile { .. } | RowKind::CommitFile { .. }
        ) {
            // Walk up to the expandable parent.
            let mut pos = self.tree.cursor();
            while pos > 0 {
                pos -= 1;
                if self.rows[pos].expandable {
                    let key = self.rows[pos].key.clone();
                    self.expanded.remove(&key);
                    self.rebuild_rows(&key);
                    return;
                }
            }
        }
    }

    fn toggle_current(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        if !row.expandable {
            return;
        }
        if row.expanded {
            self.collapse_current();
        } else {
            self.expand_current();
        }
    }

    fn toggle_selection(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        if !row.selectable {
            return;
        }
        let key = row.key.clone();
        if !self.selected.remove(&key) {
            self.selected.insert(key);
        }
        self.move_cursor(1);
    }

    // -- actions ----------------------------------------------------------------

    /// Targets of the selected rows, in tree order; falls back to the cursor row.
    fn selection_targets(&self) -> Vec<String> {
        if self.selected.is_empty() {
            return self
                .current_row()
                .and_then(|r| r.target.clone())
                .into_iter()
                .collect();
        }
        self.rows
            .iter()
            .filter(|r| self.selected.contains(&r.key))
            .filter_map(|r| r.target.clone())
            .collect()
    }

    /// `c`: commit the selected working-tree files (the index as-is when
    /// nothing relevant is selected).
    fn action_commit(&mut self) {
        let mut files: Vec<String> = Vec::new();
        if self.selected.is_empty() {
            if let Some(row) = self.current_row()
                && let RowKind::WorkingFile { .. } = row.kind
            {
                files.extend(row.target.clone());
            }
        } else {
            let mut non_working = false;
            for row in &self.rows {
                if !self.selected.contains(&row.key) {
                    continue;
                }
                match row.kind {
                    RowKind::WorkingFile { .. } => files.extend(row.target.clone()),
                    RowKind::LocalChanges { .. } => {} // header = all files
                    _ => non_working = true,
                }
            }
            if non_working {
                self.notice = Some("commit acts on local changes only".to_string());
                return;
            }
        }
        self.outcome = Some(Outcome::Run(Action::Commit { files }));
    }

    /// `f`: remember the sources, then let the user pick the target in the tree.
    fn action_fold_start(&mut self) {
        let sources = self.selection_targets();
        if sources.is_empty() {
            self.notice = Some("fold: select source rows first".to_string());
            return;
        }
        self.mode = Mode::FoldTarget { sources };
    }

    fn confirm_fold_target(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        let Some(target) = row.target.clone() else {
            self.notice = Some("fold: this row cannot be a target".to_string());
            return;
        };
        let Mode::FoldTarget { sources } = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        if sources.contains(&target) {
            self.mode = Mode::FoldTarget { sources };
            self.notice = Some("fold: target is one of the sources".to_string());
            return;
        }
        self.outcome = Some(Outcome::Run(Action::Fold { sources, target }));
    }

    /// `b`: new branch, using the cursor commit/branch as target when on one.
    fn action_new_branch(&mut self) {
        let target = self.current_row().and_then(|row| match row.kind {
            RowKind::Commit { .. } | RowKind::BranchName { .. } => row.target.clone(),
            _ => None,
        });
        self.outcome = Some(Outcome::Run(Action::NewBranch { target }));
    }

    /// `d`: drop the row under the cursor (commit, branch, or local change).
    fn action_drop(&mut self) {
        let target = self.current_row().and_then(|row| match row.kind {
            RowKind::Commit { .. } | RowKind::BranchName { .. } | RowKind::WorkingFile { .. } => {
                row.target.clone()
            }
            _ => None,
        });
        match target {
            Some(target) => self.outcome = Some(Outcome::Run(Action::Drop { target })),
            None => self.notice = Some("drop: move to a commit, branch, or file".to_string()),
        }
    }

    /// `r`: reword the commit or rename the branch under the cursor.
    fn action_reword(&mut self) {
        let target = self.current_row().and_then(|row| match row.kind {
            RowKind::Commit { .. } | RowKind::BranchName { .. } => row.target.clone(),
            _ => None,
        });
        match target {
            Some(target) => self.outcome = Some(Outcome::Run(Action::Reword { target })),
            None => self.notice = Some("reword: move to a commit or branch".to_string()),
        }
    }

    // -- rendering ----------------------------------------------------------------

    fn render_tree(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let cursor = self.tree.cursor();
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                ListItem::new(row_line(
                    row,
                    self.theme,
                    &self.snapshot.cwd_prefix,
                    self.selected.contains(&row.key),
                    i == cursor,
                ))
            })
            .collect();

        let title = match &self.mode {
            Mode::Normal => " Status ".to_string(),
            Mode::FoldTarget { sources } => {
                format!(" Fold {} item(s) into... ", sources.len())
            }
        };
        let block = pane_block(&title, self.theme, focused);
        self.tree
            .render(frame, area, items, block, self.theme.file_selected);
    }

    fn render_diff(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let key = self.ensure_diff_cached();
        let lines = borrowed_lines(&self.diff_cache[&key]);
        let block = pane_block(" Diff ", self.theme, focused);
        self.diff.render(frame, area, lines, block);
    }

    /// Compute and cache the diff for the cursor row (a git call on first
    /// visit); returns the cache key. Called from the event path so rendering
    /// never shells out.
    fn ensure_diff_cached(&mut self) -> String {
        let key = match self.rows.get(self.tree.cursor()) {
            Some(row) => row.key.clone(),
            None => String::new(),
        };
        if !self.diff_cache.contains_key(&key) {
            let lines = match self.rows.get(self.tree.cursor()) {
                Some(row) => colorize_diff(&diff_text(self.snapshot, row), self.theme),
                None => vec![Line::from("")],
            };
            self.diff_cache.insert(key.clone(), lines);
        }
        key
    }
}

/// Borrow cached lines for rendering without copying their strings.
fn borrowed_lines<'a>(lines: &'a [Line<'static>]) -> Vec<Line<'a>> {
    lines
        .iter()
        .map(|line| {
            Line::from(
                line.spans
                    .iter()
                    .map(|s| Span::styled(s.content.as_ref(), s.style))
                    .collect::<Vec<Span<'a>>>(),
            )
            .style(line.style)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Shell integration
// ---------------------------------------------------------------------------

impl ShellApp for App<'_> {
    type Exit = Outcome;

    fn config(&self) -> ShellConfig {
        ShellConfig { split: (45, 55) }
    }

    fn theme(&self) -> &TuiTheme {
        self.theme
    }

    fn quit_exit(&mut self) -> Outcome {
        Outcome::Quit
    }

    fn handle_key(
        &mut self,
        focused: PaneId,
        code: KeyCode,
        _modifiers: KeyModifiers,
    ) -> KeyResult<Outcome> {
        match code {
            KeyCode::Esc => self.handle_escape(),
            KeyCode::Up | KeyCode::Char('k') => match focused {
                PaneId::Left => self.move_cursor(-1),
                PaneId::Right => self.diff.scroll_by(-1),
            },
            KeyCode::Down | KeyCode::Char('j') => match focused {
                PaneId::Left => self.move_cursor(1),
                PaneId::Right => self.diff.scroll_by(1),
            },
            KeyCode::PageUp => self.diff.scroll_page(-1),
            KeyCode::PageDown => self.diff.scroll_page(1),
            KeyCode::Right | KeyCode::Char('l') => self.expand_current(),
            KeyCode::Left | KeyCode::Char('h') => self.collapse_current(),
            KeyCode::Enter => match &self.mode {
                Mode::FoldTarget { .. } => self.confirm_fold_target(),
                Mode::Normal => self.toggle_current(),
            },
            // While picking a fold target only navigation, Enter, and Esc
            // apply — action keys must not fire and discard the pending fold.
            KeyCode::Char(' ' | 'c' | 'f' | 'b' | 'd' | 'r' | 'R') | KeyCode::F(5)
                if matches!(self.mode, Mode::FoldTarget { .. }) =>
            {
                self.notice = Some("fold: Enter to confirm, Esc to cancel".to_string());
            }
            KeyCode::Char(' ') => self.toggle_selection(),
            KeyCode::Char('c') => self.action_commit(),
            KeyCode::Char('f') => self.action_fold_start(),
            KeyCode::Char('b') => self.action_new_branch(),
            KeyCode::Char('d') => self.action_drop(),
            KeyCode::Char('r') => self.action_reword(),
            KeyCode::Char('R') | KeyCode::F(5) => self.outcome = Some(Outcome::Refresh),
            _ => {}
        }
        // Deep handlers (actions, Esc, fold confirm) report exits through the
        // outcome field.
        match self.outcome.take() {
            Some(outcome) => KeyResult::Exit(outcome),
            None => {
                self.ensure_diff_cached();
                KeyResult::Handled
            }
        }
    }

    fn handle_mouse(&mut self, pane: PaneId, kind: MouseEventKind, pos: Position, area: Rect) {
        match pane {
            PaneId::Left => match kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let Some(clicked) = self.tree.hit_test(area, pos.y) else {
                        return;
                    };
                    if clicked < self.rows.len() && self.rows[clicked].focusable {
                        self.tree.set_cursor(clicked);
                        self.diff.reset();
                    }
                }
                MouseEventKind::ScrollUp => self.move_cursor(-1),
                MouseEventKind::ScrollDown => self.move_cursor(1),
                _ => {}
            },
            PaneId::Right => match kind {
                MouseEventKind::ScrollUp => self.diff.scroll_by(-3),
                MouseEventKind::ScrollDown => self.diff.scroll_by(3),
                _ => {}
            },
        }
        self.ensure_diff_cached();
    }

    fn render_pane(&mut self, frame: &mut Frame, pane: PaneId, area: Rect, focused: bool) {
        match pane {
            PaneId::Left => self.render_tree(frame, area, focused),
            PaneId::Right => self.render_diff(frame, area, focused),
        }
    }

    fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    fn clear_notice(&mut self) {
        self.notice = None;
    }

    fn mode_hint(&self) -> Option<String> {
        match &self.mode {
            Mode::FoldTarget { .. } => {
                Some(" fold: move to the target, Enter to confirm, Esc to cancel".to_string())
            }
            Mode::Normal => None,
        }
    }

    fn status_hints(&self, _focused: PaneId) -> Vec<Cow<'static, str>> {
        vec![
            "Navigate: ↑/↓".into(),
            "Fold/unfold: ←/→".into(),
            "Select: space".into(),
            "Commit: c".into(),
            "Fold: f".into(),
            "Branch: b".into(),
            "Drop: d".into(),
            "Reword: r".into(),
            "Refresh: R".into(),
            "Quit: q".into(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Row rendering
// ---------------------------------------------------------------------------

/// Render one tree row as a styled line. The first span is the multi-select
/// gutter.
fn row_line(
    row: &Row,
    theme: &TuiTheme,
    cwd_prefix: &str,
    selected: bool,
    is_cursor: bool,
) -> Line<'static> {
    // On the cursor row the selection background swallows regular dim text.
    let dim = if is_cursor {
        theme.dim_selected
    } else {
        theme.dim
    };
    let mut spans: Vec<Span<'static>> = vec![if selected {
        Span::styled("✓ ", theme.selection)
    } else {
        Span::raw("  ")
    }];

    let display = |path: &str| crate::core::repo::cwd_relative_path(path, cwd_prefix);

    match &row.kind {
        RowKind::LocalChanges { count } => {
            spans.push(Span::styled("╭─ ", theme.graph));
            spans.push(Span::styled(row.sid.clone(), theme.shortid));
            spans.push(Span::styled(" [", dim));
            spans.push(Span::styled("local changes", theme.label));
            spans.push(Span::styled("]", dim));
            if *count == 0 {
                spans.push(Span::styled(" no changes", dim));
            } else if !row.expanded {
                spans.push(Span::styled(format!(" ({} files)", count), dim));
            }
        }
        RowKind::WorkingFile {
            path,
            index,
            worktree,
        } => {
            spans.push(Span::styled("│   ", theme.graph));
            spans.push(Span::styled(row.sid.clone(), theme.shortid));
            if *index == '?' && *worktree == '?' {
                spans.push(Span::styled(" ⁕ ", theme.untracked));
            } else if *index == '!' && *worktree == '!' {
                spans.push(Span::styled(
                    " !! ",
                    theme.unstaged_status.add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(index.to_string(), theme.staged_status));
                spans.push(Span::styled(worktree.to_string(), theme.unstaged_status));
                spans.push(Span::raw(" "));
            }
            spans.push(Span::raw(display(path)));
        }
        RowKind::BranchName {
            name,
            remote,
            connector,
            ..
        } => {
            spans.push(Span::styled(format!("{} ", connector), theme.graph));
            spans.push(Span::styled(row.sid.clone(), theme.shortid));
            spans.push(Span::styled(" [", dim));
            spans.push(Span::styled(name.clone(), theme.branch));
            spans.push(Span::styled("]", dim));
            match remote {
                Some(RemoteStatus::Synced) => spans.push(Span::styled(" ✓", theme.remote_synced)),
                Some(RemoteStatus::Ahead) => spans.push(Span::styled(" ↑", theme.remote_ahead)),
                Some(RemoteStatus::Gone) => spans.push(Span::styled(" ✗", theme.remote_gone)),
                None => {}
            }
        }
        RowKind::Commit {
            message,
            sid_rest,
            dot_color,
            file_count,
            ..
        } => {
            match dot_color {
                Some(idx) => {
                    spans.push(Span::styled("│", theme.graph));
                    spans.push(Span::styled(
                        "●",
                        theme.branch_dots[idx % theme.branch_dots.len()],
                    ));
                    spans.push(Span::raw("  "));
                }
                None => {
                    spans.push(Span::styled("●", theme.graph));
                    spans.push(Span::raw("   "));
                }
            }
            spans.push(Span::styled(row.sid.clone(), theme.shortid));
            spans.push(Span::styled(sid_rest.clone(), dim));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(message.clone(), theme.message));
            if row.expandable && !row.expanded {
                spans.push(Span::styled(format!(" ({} files)", file_count), dim));
            }
        }
        RowKind::CommitFile {
            path,
            index,
            worktree,
            on_branch,
            ..
        } => {
            let prefix = if *on_branch { "│┊    " } else { "┊     " };
            spans.push(Span::styled(prefix, theme.graph));
            spans.push(Span::styled(row.sid.clone(), theme.shortid));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(index.to_string(), theme.staged_status));
            spans.push(Span::styled(worktree.to_string(), theme.unstaged_status));
            spans.push(Span::raw(" "));
            spans.push(Span::raw(display(path)));
        }
        RowKind::Upstream {
            label,
            base_short_id,
            base_message,
            commits_ahead,
        } => {
            spans.push(Span::styled("●  ", theme.graph));
            spans.push(Span::styled(format!("{} ", base_short_id), dim));
            spans.push(Span::styled("(upstream) ", theme.label));
            spans.push(Span::styled("[", dim));
            spans.push(Span::styled(label.clone(), theme.branch));
            spans.push(Span::styled("] ", dim));
            if *commits_ahead > 0 {
                spans.push(Span::styled(
                    format!(
                        "⏫ {} new commit{} — ",
                        commits_ahead,
                        if *commits_ahead == 1 { "" } else { "s" }
                    ),
                    theme.message,
                ));
            }
            spans.push(Span::styled(base_message.clone(), theme.message));
        }
        RowKind::Context {
            short_hash,
            date,
            message,
        } => {
            spans.push(Span::styled("· ", dim));
            spans.push(Span::styled(format!("{} {} ", short_hash, date), dim));
            spans.push(Span::styled(message.clone(), theme.message));
        }
        RowKind::Spacer(text) => {
            spans.push(Span::styled(*text, theme.graph));
        }
    }

    Line::from(spans)
}

// ---------------------------------------------------------------------------
// Diff pane content
// ---------------------------------------------------------------------------

/// Produce the raw diff text for a row by shelling out to git.
fn diff_text(snapshot: &Snapshot, row: &Row) -> String {
    let workdir = &snapshot.workdir;
    let result = match &row.kind {
        RowKind::LocalChanges { count } => {
            if *count == 0 {
                return "no changes".to_string();
            }
            git::run_git_stdout(workdir, &["diff", "HEAD"])
        }
        RowKind::WorkingFile {
            path,
            index,
            worktree,
        } => {
            if *index == '?' && *worktree == '?' {
                return untracked_file_text(workdir, path);
            }
            git::run_git_stdout(workdir, &["diff", "HEAD", "--", path])
        }
        RowKind::BranchName { range, .. } => match range {
            Some((base, tip)) => git::run_git_stdout(workdir, &["diff", base, tip]),
            None => return "branch has no commits of its own".to_string(),
        },
        RowKind::Commit { oid, .. } => {
            git::run_git_stdout(workdir, &["show", "--stat", "--patch", &oid.to_string()])
        }
        RowKind::CommitFile { oid, path, .. } => git::run_git_stdout(
            workdir,
            &["show", "--format=", &oid.to_string(), "--", path],
        ),
        RowKind::Upstream {
            label,
            base_short_id,
            base_message,
            commits_ahead,
        } => {
            return format!(
                "upstream: {}\ncommon base: {} {}\n{} new commit(s) on the remote",
                label, base_short_id, base_message, commits_ahead
            );
        }
        RowKind::Context { short_hash, .. } => {
            git::run_git_stdout(workdir, &["show", "--stat", "--patch", short_hash])
        }
        RowKind::Spacer(_) => return String::new(),
    };
    match result {
        Ok(text) if text.trim().is_empty() => "no changes".to_string(),
        Ok(text) => text,
        Err(e) => format!("error: {}", e),
    }
}

/// Render an untracked file's content as added lines.
///
/// Only a bounded prefix is read so a large artifact never spikes memory;
/// the binary (NUL byte) check runs on that prefix, like git's own heuristic.
fn untracked_file_text(workdir: &std::path::Path, path: &str) -> String {
    const MAX_LINES: usize = 2000;
    const MAX_BYTES: u64 = 256 * 1024;
    use std::io::Read;

    let full = workdir.join(path);
    let Ok(file) = std::fs::File::open(&full) else {
        return "(unreadable file)".to_string();
    };
    let mut bytes = Vec::new();
    if file.take(MAX_BYTES + 1).read_to_end(&mut bytes).is_err() {
        return "(unreadable file)".to_string();
    }
    let mut truncated = bytes.len() as u64 > MAX_BYTES;
    bytes.truncate(MAX_BYTES as usize);
    if bytes.contains(&0) {
        return "(binary file)".to_string();
    }
    let content = String::from_utf8_lossy(&bytes);
    let mut out = format!("untracked file: {}\n", path);
    for (i, line) in content.lines().enumerate() {
        if i >= MAX_LINES {
            truncated = true;
            break;
        }
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    if truncated {
        out.push_str("...\n");
    }
    out
}

#[cfg(test)]
#[path = "app_test.rs"]
mod tests;
