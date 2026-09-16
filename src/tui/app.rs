//! Interactive status TUI (`loom tui`): the status tree on the left, the diff
//! of the item under the cursor on the right.
//!
//! Actions (commit, fold, branch, drop, reword) run the regular loom command
//! on a worker thread while the TUI stays up: the command's prompts become
//! popups and its messages a log (`core::ui`), and only an editor takes the
//! terminal over. Fold picks its target in a second step inside the tree.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

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
use crate::core::transaction;
use crate::core::ui::{self, Answer, Cancelled, Level, Request};
use crate::git;
use crate::tui::shell::{KeyResult, PaneId, Shell, ShellApp, ShellConfig, Tick};
use crate::tui::status_tree::{self, LOCAL_CHANGES_KEY, Row, RowKind, branch_key};
use crate::tui::theme::TuiTheme;
use crate::tui::widgets::common::{colorize_diff, pane_block};
use crate::tui::widgets::diff_pane::DiffPane;
use crate::tui::widgets::list_pane::ListPane;
use crate::tui::widgets::popup::{self, LogEntry, Notice, Prompt, PromptOutcome, TextField};
use crate::{branch, commit, drop, fold, reword};

// ── Data model ───────────────────────────────────────────────────────────

/// Everything gathered from the repo for one TUI round.
struct Snapshot {
    workdir: PathBuf,
    git_dir: PathBuf,
    cwd_prefix: String,
    sections: Vec<Section>,
    ids: IdAllocator,
}

/// A loom command to run on the worker thread.
#[derive(Debug, PartialEq, Eq)]
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
    /// `loom reword <target>`; `name` is the new branch name typed in the
    /// tree, `None` for a commit (the editor asks for the message).
    Reword {
        target: String,
        name: Option<String>,
    },
}

/// Why the event loop returned.
enum Outcome {
    Quit,
}

/// Input mode: normal, picking the target of a pending fold, or typing a
/// branch's new name over its row.
enum Mode {
    Normal,
    FoldTarget { sources: Vec<String> },
    RenameBranch { branch: String, field: TextField },
}

/// The action currently running on its worker thread.
struct Running {
    handle: JoinHandle<Result<()>>,
    command: String,
    /// Text of the command's active spinner, if any.
    spinner: Option<String>,
    /// Ticks so far, for the status-bar animation.
    ticks: usize,
}

/// What is drawn over the panes and owns the keyboard.
enum Popup {
    /// A prompt from the running command; the answer goes back on `reply`.
    Prompt {
        prompt: Prompt,
        reply: Sender<Option<Answer>>,
    },
    /// A message to dismiss; `then` runs on dismissal.
    Notice {
        notice: Notice,
        then: AfterNotice,
    },
    Log {
        scroll: DiffPane,
    },
}

enum AfterNotice {
    Nothing,
    /// The action failed: reload the tree to show whatever it left.
    Reload,
    /// The action paused on conflicts: nothing else can run, so leave.
    Quit,
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ── Entry point ──────────────────────────────────────────────────────────

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
    let snapshot = load_snapshot()?;
    let git_dir = snapshot.git_dir.clone();

    // Local changes start expanded.
    let mut expanded: HashSet<String> = HashSet::new();
    expanded.insert(LOCAL_CHANGES_KEY.to_string());

    let app = App::new(snapshot, &tui_theme, theme, expanded);
    let (_, Outcome::Quit) = Shell::new(app).run()?;

    // The TUI leaves on a conflict pause; repeat the popup's guidance where
    // it stays readable.
    if let Ok(Some(state)) = transaction::load(&git_dir) {
        println!("{}", paused_message(&state.command));
    }
    Ok(())
}

fn paused_message(command: &str) -> String {
    format!(
        "A `loom {}` is paused due to conflicts.\n\
         Resolve them, then run `loom continue` to resume, or `loom abort` to cancel.",
        command
    )
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

/// Run the loom command for `action`. Runs on the worker thread: the trace
/// logger is thread-local, so each action gets its own log file, named after
/// the command line the log shows.
fn execute_action(
    action: Action,
    command: &str,
    git_dir: &std::path::Path,
    theme: &graph::Theme,
) -> Result<()> {
    crate::trace::init(git_dir, &format!("loom tui: {}", command));
    let result = match action {
        Action::Commit { files } => commit::run(None, false, None, false, files, vec![], theme),
        Action::Fold { sources, target } => {
            let mut args = sources;
            args.push(target);
            fold::run(false, false, None, args, theme)
        }
        Action::NewBranch { target } => branch::new::run(None, target),
        Action::Drop { target } => drop::run(target, false),
        Action::Reword { target, name } => reword::run(target, name),
    };
    crate::trace::finalize();
    result
}

// ── App state ────────────────────────────────────────────────────────────

struct App<'a> {
    snapshot: Snapshot,
    theme: &'a TuiTheme,
    /// Handed to each action's worker (the hunk pickers take it).
    graph_theme: graph::Theme,
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
    /// Row key to put the cursor on after the next reload, for an action that
    /// renames the row it acts on.
    next_cursor: Option<String>,
    /// Transient message shown in the status bar until the next key.
    notice: Option<String>,
    /// Exit value set by deep handlers, drained after each key.
    outcome: Option<Outcome>,
    /// Requests from the running action (`core::ui`); `request_tx` is what
    /// each worker gets a clone of.
    requests: Receiver<Request>,
    request_tx: Sender<Request>,
    running: Option<Running>,
    popup: Option<Popup>,
    log: Vec<LogEntry>,
}

impl<'a> App<'a> {
    fn new(
        snapshot: Snapshot,
        theme: &'a TuiTheme,
        graph_theme: graph::Theme,
        expanded: HashSet<String>,
    ) -> Self {
        let rows = status_tree::build_rows(&snapshot.sections, &snapshot.ids, &expanded);
        let cursor = rows.iter().position(|r| r.focusable).unwrap_or(0);
        let (request_tx, requests) = channel();
        let mut app = App {
            snapshot,
            theme,
            graph_theme,
            rows,
            tree: ListPane::new(cursor),
            selected: HashSet::new(),
            expanded,
            mode: Mode::Normal,
            diff: DiffPane::new(),
            diff_cache: HashMap::new(),
            next_cursor: None,
            notice: None,
            outcome: None,
            requests,
            request_tx,
            running: None,
            popup: None,
            log: Vec::new(),
        };
        // Prime the diff for the initial cursor row so the first render
        // doesn't have to.
        app.ensure_diff_cached();
        app
    }

    fn current_row(&self) -> Option<&Row> {
        self.rows.get(self.tree.cursor())
    }

    /// Replace the repo state, keeping the cursor on the same row when it
    /// still exists. Selection and the diff cache are tied to the old rows.
    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let key = self
            .next_cursor
            .take()
            .or_else(|| self.current_row().map(|r| r.key.clone()));
        self.snapshot = snapshot;
        self.selected.clear();
        self.diff_cache.clear();
        self.rows =
            status_tree::build_rows(&self.snapshot.sections, &self.snapshot.ids, &self.expanded);
        let cursor = key
            .and_then(|key| self.rows.iter().position(|r| r.focusable && r.key == key))
            .or_else(|| self.rows.iter().position(|r| r.focusable))
            .unwrap_or(0);
        self.tree.set_cursor(cursor);
        self.diff.reset();
        self.ensure_diff_cached();
    }

    /// Reload the tree from the repo; a failure is shown, the old tree kept.
    fn reload(&mut self) {
        match load_snapshot() {
            Ok(snapshot) => self.apply_snapshot(snapshot),
            Err(e) => {
                self.next_cursor = None;
                self.show_error(&e.to_string(), AfterNotice::Nothing);
            }
        }
    }

    fn show_error(&mut self, text: &str, then: AfterNotice) {
        self.popup = Some(Popup::Notice {
            notice: Notice::new("Error", Level::Error, text),
            then,
        });
    }

    // -- running an action ------------------------------------------------------

    /// The CLI line equivalent to `action`, with the short IDs the tree
    /// shows, for the log.
    fn command_line(&self, action: &Action) -> String {
        let sid = |target: &str| -> String {
            self.rows
                .iter()
                .find(|r| r.target.as_deref() == Some(target) && !r.sid.is_empty())
                .map(|r| r.sid.clone())
                .unwrap_or_else(|| target.to_string())
        };
        let mut words = vec!["loom".to_string()];
        match action {
            Action::Commit { files } => {
                words.push("commit".into());
                words.extend(files.iter().map(|f| sid(f)));
            }
            Action::Fold { sources, target } => {
                words.push("fold".into());
                words.extend(sources.iter().map(|s| sid(s)));
                words.push(sid(target));
            }
            Action::NewBranch { target } => {
                words.extend(["branch".into(), "new".into()]);
                if let Some(target) = target {
                    words.extend(["-t".into(), sid(target)]);
                }
            }
            Action::Drop { target } => words.extend(["drop".into(), sid(target)]),
            Action::Reword { target, name } => {
                words.extend(["reword".into(), sid(target)]);
                if let Some(name) = name {
                    words.extend(["-m".into(), name.clone()]);
                }
            }
        }
        words.join(" ")
    }

    /// Run `action` on a worker thread; the TUI keeps going and answers its
    /// prompts.
    fn start_action(&mut self, action: Action) {
        let command = self.command_line(&action);
        self.log.push(LogEntry {
            command: command.clone(),
            lines: Vec::new(),
        });
        self.selected.clear();
        self.mode = Mode::Normal;
        self.notice = None;

        let tx = self.request_tx.clone();
        let git_dir = self.snapshot.git_dir.clone();
        let theme = self.graph_theme.clone();
        let trace_name = command.clone();
        let handle = std::thread::spawn(move || {
            ui::install(tx);
            let result = execute_action(action, &trace_name, &git_dir, &theme);
            ui::uninstall();
            result
        });
        self.running = Some(Running {
            handle,
            command,
            spinner: None,
            ticks: 0,
        });
    }

    /// One request from the running action. `Suspend` is the shell's to
    /// handle and never reaches here.
    fn handle_request(&mut self, request: Request) {
        match request {
            Request::Prompt {
                kind,
                prompt,
                error,
                reply,
            } => {
                self.popup = Some(Popup::Prompt {
                    prompt: Prompt::new(kind, prompt, error),
                    reply,
                });
            }
            Request::Message { level, text } => {
                if let Some(entry) = self.log.last_mut() {
                    entry.lines.push((level, text));
                }
            }
            Request::Spinner(text) => {
                if let Some(running) = &mut self.running {
                    running.spinner = text;
                }
            }
            Request::Suspend(_) | Request::Resume => {}
        }
    }

    /// The worker is done: report in the status bar (success), a popup
    /// (failure), or nothing (cancelled prompt). Returns whether the repo may
    /// have changed, so the caller runs [`App::after_action`].
    fn finish_action(&mut self, result: Result<()>) -> bool {
        if result.is_err() {
            // The row the action would have renamed still has its old key.
            self.next_cursor = None;
        }
        match result {
            Ok(()) => {
                let last_success = self.log.last().and_then(|entry| {
                    entry
                        .lines
                        .iter()
                        .rev()
                        .find(|(level, _)| *level == Level::Success)
                        .and_then(|(_, text)| text.lines().next())
                        .map(str::to_string)
                });
                self.notice = Some(format!("✓ {}", last_success.as_deref().unwrap_or("done")));
                true
            }
            Err(e) if e.downcast_ref::<Cancelled>().is_some() => {
                self.log_line(Level::Warn, "Cancelled");
                self.notice = Some("cancelled".to_string());
                false
            }
            Err(e) => {
                let text = e.to_string();
                self.log_line(Level::Error, &text);
                // An open log already shows the error line, so reload behind
                // it instead of replacing what the reader is looking at.
                if matches!(self.popup, Some(Popup::Log { .. })) {
                    return true;
                }
                self.show_error(&text, AfterNotice::Reload);
                false
            }
        }
    }

    fn log_line(&mut self, level: Level, text: &str) {
        if let Some(entry) = self.log.last_mut() {
            entry.lines.push((level, text.to_string()));
        }
    }

    /// After an action changed the repo: a conflict pause ends the session
    /// (every other command is blocked until it's resolved), otherwise the
    /// tree reloads.
    fn after_action(&mut self) {
        match transaction::load(&self.snapshot.git_dir) {
            Ok(Some(state)) => {
                self.popup = Some(Popup::Notice {
                    notice: Notice::new("Paused", Level::Warn, &paused_message(&state.command)),
                    then: AfterNotice::Quit,
                });
            }
            _ => self.reload(),
        }
    }

    fn dismiss_notice(&mut self, then: AfterNotice) {
        match then {
            AfterNotice::Nothing => {}
            AfterNotice::Reload => self.after_action(),
            AfterNotice::Quit => self.outcome = Some(Outcome::Quit),
        }
    }

    fn handle_popup_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        match popup {
            Popup::Prompt { mut prompt, reply } => match prompt.handle_key(code, modifiers) {
                PromptOutcome::Pending => self.popup = Some(Popup::Prompt { prompt, reply }),
                PromptOutcome::Answer(answer) => {
                    let _ = reply.send(Some(answer));
                }
                PromptOutcome::Cancel => {
                    let _ = reply.send(None);
                }
            },
            Popup::Notice { notice, then } => {
                if Notice::dismisses(code) {
                    self.dismiss_notice(then);
                } else {
                    self.popup = Some(Popup::Notice { notice, then });
                }
            }
            Popup::Log { mut scroll } => match code {
                KeyCode::Esc | KeyCode::Char('q' | 'L') => {}
                _ => {
                    match code {
                        KeyCode::Up | KeyCode::Char('k') => scroll.scroll_by(-1),
                        KeyCode::Down | KeyCode::Char('j') => scroll.scroll_by(1),
                        KeyCode::PageUp => scroll.scroll_page(-1),
                        KeyCode::PageDown => scroll.scroll_page(1),
                        _ => {}
                    }
                    self.popup = Some(Popup::Log { scroll });
                }
            },
        }
    }

    fn open_log(&mut self) {
        let mut scroll = DiffPane::new();
        // Start at the newest entry; the render clamps to the content.
        scroll.set_scroll(u16::MAX);
        self.popup = Some(Popup::Log { scroll });
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

    /// A key with no popup open and no action running.
    fn handle_tree_key(&mut self, focused: PaneId, code: KeyCode) {
        let action = match code {
            KeyCode::Esc => {
                self.handle_escape();
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                match focused {
                    PaneId::Left => self.move_cursor(-1),
                    PaneId::Right => self.diff.scroll_by(-1),
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match focused {
                    PaneId::Left => self.move_cursor(1),
                    PaneId::Right => self.diff.scroll_by(1),
                }
                None
            }
            KeyCode::PageUp => {
                self.diff.scroll_page(-1);
                None
            }
            KeyCode::PageDown => {
                self.diff.scroll_page(1);
                None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.expand_current();
                None
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.collapse_current();
                None
            }
            KeyCode::Enter => match &self.mode {
                Mode::FoldTarget { .. } => self.confirm_fold_target(),
                // Rename mode never gets here: it is handled above.
                _ => {
                    self.toggle_current();
                    None
                }
            },
            KeyCode::Char('L') => {
                self.open_log();
                None
            }
            // While picking a fold target only navigation, Enter, and Esc
            // apply — action keys must not fire and discard the pending fold.
            KeyCode::Char(' ' | 'c' | 'f' | 'b' | 'd' | 'r' | 'R') | KeyCode::F(5)
                if matches!(self.mode, Mode::FoldTarget { .. }) =>
            {
                self.notice = Some("fold: Enter to confirm, Esc to cancel".to_string());
                None
            }
            KeyCode::Char(' ') => {
                self.toggle_selection();
                None
            }
            KeyCode::Char('c') => self.action_commit(),
            KeyCode::Char('f') => {
                self.action_fold_start();
                None
            }
            KeyCode::Char('b') => self.action_new_branch(),
            KeyCode::Char('d') => self.action_drop(),
            KeyCode::Char('r') => self.action_reword(),
            KeyCode::Char('R') | KeyCode::F(5) => {
                self.reload();
                None
            }
            _ => None,
        };
        if let Some(action) = action {
            self.start_action(action);
        }
    }

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
    fn action_commit(&mut self) -> Option<Action> {
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
                return None;
            }
        }
        Some(Action::Commit { files })
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

    fn confirm_fold_target(&mut self) -> Option<Action> {
        let row = self.current_row()?;
        let Some(target) = row.target.clone() else {
            self.notice = Some("fold: this row cannot be a target".to_string());
            return None;
        };
        let Mode::FoldTarget { sources } = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return None;
        };
        if sources.contains(&target) {
            self.mode = Mode::FoldTarget { sources };
            self.notice = Some("fold: target is one of the sources".to_string());
            return None;
        }
        Some(Action::Fold { sources, target })
    }

    /// `b`: new branch, using the cursor commit/branch as target when on one.
    fn action_new_branch(&mut self) -> Option<Action> {
        let target = self.current_row().and_then(|row| match row.kind {
            RowKind::Commit { .. } | RowKind::BranchName { .. } => row.target.clone(),
            _ => None,
        });
        Some(Action::NewBranch { target })
    }

    /// `d`: drop the row under the cursor (commit, branch, or local change).
    fn action_drop(&mut self) -> Option<Action> {
        let target = self.current_row().and_then(|row| match row.kind {
            RowKind::Commit { .. } | RowKind::BranchName { .. } | RowKind::WorkingFile { .. } => {
                row.target.clone()
            }
            _ => None,
        });
        if target.is_none() {
            self.notice = Some("drop: move to a commit, branch, or file".to_string());
        }
        target.map(|target| Action::Drop { target })
    }

    /// `r`: reword the commit under the cursor, or start editing the branch
    /// name in place.
    fn action_reword(&mut self) -> Option<Action> {
        let row = self.current_row()?;
        let target = row.target.clone();
        match (&row.kind, target) {
            (RowKind::Commit { .. }, Some(target)) => Some(Action::Reword { target, name: None }),
            (RowKind::BranchName { name, .. }, Some(target)) => {
                let field = TextField::new(name);
                self.mode = Mode::RenameBranch {
                    branch: target,
                    field,
                };
                None
            }
            _ => {
                self.notice = Some("reword: move to a commit or branch".to_string());
                None
            }
        }
    }

    /// A key while the branch name is being edited in the tree. Enter runs
    /// the rename, Esc cancels, everything else edits the field.
    fn handle_rename_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        let Mode::RenameBranch { branch, field } = &mut self.mode else {
            return None;
        };
        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            self.mode = Mode::Normal;
            self.notice = Some("rename cancelled".to_string());
            return None;
        }
        match code {
            KeyCode::Enter => {
                let name = field.value().trim().to_string();
                let branch = branch.clone();
                if name.is_empty() {
                    self.notice = Some("rename: the name cannot be empty".to_string());
                    return None;
                }
                self.mode = Mode::Normal;
                if name == branch {
                    self.notice = Some("rename: name unchanged".to_string());
                    return None;
                }
                // Follow the branch to its new row rather than reloading to
                // the top of the tree.
                self.next_cursor = Some(branch_key(&name));
                Some(Action::Reword {
                    target: branch,
                    name: Some(name),
                })
            }
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.notice = Some("rename cancelled".to_string());
                None
            }
            _ => {
                field.handle_key(code, modifiers);
                None
            }
        }
    }

    // -- rendering ----------------------------------------------------------------

    fn render_tree(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let cursor = self.tree.cursor();
        let editing = match &self.mode {
            Mode::RenameBranch { branch, field } => Some((branch_key(branch), field)),
            _ => None,
        };
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
                    editing
                        .as_ref()
                        .filter(|(key, _)| *key == row.key)
                        .map(|(_, f)| *f),
                ))
            })
            .collect();

        let title = match &self.mode {
            Mode::Normal => " Status ".to_string(),
            Mode::FoldTarget { sources } => {
                format!(" Fold {} item(s) into... ", sources.len())
            }
            Mode::RenameBranch { .. } => " Rename branch ".to_string(),
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
                Some(row) => colorize_diff(&diff_text(&self.snapshot, row), self.theme),
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

// ── Shell integration ────────────────────────────────────────────────────

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
        modifiers: KeyModifiers,
    ) -> KeyResult<Outcome> {
        if self.popup.is_some() {
            self.handle_popup_key(code, modifiers);
        } else if matches!(self.mode, Mode::RenameBranch { .. }) {
            if let Some(action) = self.handle_rename_key(code, modifiers) {
                self.start_action(action);
            }
        } else if self.running.is_some() {
            // The log is read-only; everything else waits for the action.
            match code {
                KeyCode::Char('L') => self.open_log(),
                _ => self.notice = Some("an action is running…".to_string()),
            }
        } else {
            self.handle_tree_key(focused, code);
        }
        // Deep handlers (Esc, notice dismissal) report exits through the
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

    fn render_overlay(&mut self, frame: &mut Frame, area: Rect) {
        match &mut self.popup {
            Some(Popup::Prompt { prompt, .. }) => prompt.render(frame, area, self.theme),
            Some(Popup::Notice { notice, .. }) => notice.render(frame, area, self.theme),
            Some(Popup::Log { scroll }) => {
                popup::render_log(frame, area, &self.log, scroll, self.theme)
            }
            None => {}
        }
    }

    fn modal_active(&self) -> bool {
        // Renaming owns the keyboard too: `q` and Tab must reach the field.
        self.popup.is_some()
            || self.running.is_some()
            || matches!(self.mode, Mode::RenameBranch { .. })
    }

    fn poll_background(&mut self) -> Tick<Outcome> {
        let mut changed = false;
        loop {
            match self.requests.try_recv() {
                Ok(Request::Suspend(ack)) => return Tick::Suspend(ack),
                Ok(request) => {
                    self.handle_request(request);
                    changed = true;
                }
                Err(_) => break,
            }
        }
        if let Some(running) = &mut self.running {
            running.ticks += 1;
            if running.handle.is_finished() {
                let running = self.running.take().expect("checked above");
                let result = running.handle.join().unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "the action crashed — run `loom trace` for what it did"
                    ))
                });
                if self.finish_action(result) {
                    self.after_action();
                }
            }
            changed = true;
        }
        if let Some(outcome) = self.outcome.take() {
            return Tick::Exit(outcome);
        }
        if changed { Tick::Redraw } else { Tick::Idle }
    }

    /// The worker holds the terminal (an editor); its `Resume` ends the wait.
    /// A worker that dies without one ends it too.
    fn wait_for_resume(&mut self) {
        loop {
            match self.requests.recv_timeout(Duration::from_millis(100)) {
                Ok(Request::Resume) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return;
                }
                Ok(request) => self.handle_request(request),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if self.running.as_ref().is_none_or(|r| r.handle.is_finished()) {
                        return;
                    }
                }
            }
        }
    }

    fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    fn clear_notice(&mut self) {
        self.notice = None;
    }

    fn mode_hint(&self) -> Option<String> {
        if let Some(running) = &self.running {
            let frame = SPINNER_FRAMES[(running.ticks / 2) % SPINNER_FRAMES.len()];
            let detail = running
                .spinner
                .as_deref()
                .map(|s| format!(" — {}", s))
                .unwrap_or_default();
            return Some(format!(" {} {}{}", frame, running.command, detail));
        }
        match &self.mode {
            Mode::FoldTarget { .. } => {
                Some(" fold: move to the target, Enter to confirm, Esc to cancel".to_string())
            }
            Mode::RenameBranch { .. } => Some(
                " rename: type the new branch name, Enter to confirm, Esc to cancel".to_string(),
            ),
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
            "Log: L".into(),
            "Refresh: R".into(),
            "Quit: q".into(),
        ]
    }
}

// ── Row rendering ────────────────────────────────────────────────────────

/// Render one tree row as a styled line; `editing` replaces the branch name
/// with the field being typed. The first span is the multi-select gutter.
fn row_line(
    row: &Row,
    theme: &TuiTheme,
    cwd_prefix: &str,
    selected: bool,
    is_cursor: bool,
    editing: Option<&TextField>,
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
            match editing {
                Some(field) => spans.extend(field.spans(theme)),
                None => spans.push(Span::styled(name.clone(), theme.branch)),
            }
            spans.push(Span::styled("]", dim));
            match remote {
                Some(RemoteStatus::Synced) => spans.push(Span::styled(" ✓", theme.remote_synced)),
                Some(RemoteStatus::Different) => spans.push(Span::styled(" ↑", theme.remote_ahead)),
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

// ── Diff pane content ────────────────────────────────────────────────────

/// Produce the raw diff text for a row by shelling out to git.
fn diff_text(snapshot: &Snapshot, row: &Row) -> String {
    let workdir = &snapshot.workdir;
    let result = match &row.kind {
        RowKind::LocalChanges { count } => {
            if *count == 0 {
                return "no changes".to_string();
            }
            git::diff_head_display(workdir)
        }
        RowKind::WorkingFile {
            path,
            index,
            worktree,
        } => {
            if *index == '?' && *worktree == '?' {
                return untracked_file_text(workdir, path);
            }
            git::diff_head_file_display(workdir, path)
        }
        RowKind::BranchName { range, .. } => match range {
            Some((base, tip)) => git::diff_range(workdir, base, tip),
            None => return "branch has no commits of its own".to_string(),
        },
        RowKind::Commit { oid, .. } => git::show_commit_patch(workdir, &oid.to_string()),
        RowKind::CommitFile { oid, path, .. } => {
            git::show_commit_file(workdir, &oid.to_string(), path)
        }
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
        RowKind::Context { short_hash, .. } => git::show_commit_patch(workdir, short_hash),
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
