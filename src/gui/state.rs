//! What the window shows of the repository, apart from the window: the tests
//! drive this, and none opens a window.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::core::graph;
use crate::core::repo;
use crate::core::ui::{Hint, Level};
use crate::gui::board::{Board, Rect};
use crate::gui::forms::{Draft, Form};
use crate::session;
use crate::session::action::{self, Action, LogEntry, Summary};
use crate::session::diff_text::{self, diff_text};
use crate::session::paused::PausedOp;
use crate::session::snapshot::{Snapshot, load_snapshot};
use crate::session::tree::{self, Row, RowKind, SelectionClass};
use crate::session::worker::{Ended, Event, Worker};

/// What the inspector shows of one row.
pub(crate) struct Inspected<'a> {
    /// The key of the row, which tells the window when to build the lines of
    /// another diff.
    pub key: &'a str,
    pub title: String,
    pub diff: &'a str,
}

/// A line of the status bar.
struct Notice {
    level: Level,
    text: String,
}

pub(crate) struct State {
    pub(super) snapshot: Snapshot,
    /// Set while an operation is paused.
    pub(super) paused: Option<PausedOp>,
    /// Context depth the tree is read with, as `loom status -n <N>` takes it.
    context: usize,
    /// The tree of the session: the local changes pane lists its local
    /// changes, the board places its branches and commits. The commits of a
    /// closed branch are not in it.
    pub(super) rows: Vec<Row>,
    /// Keys of the commit rows open onto their files, and the local changes',
    /// which the pane lists whole.
    expanded: HashSet<String>,
    /// Keys of the branch rows closed onto their name.
    collapsed: HashSet<String>,
    /// How many commits each branch row owns, in the tree or not.
    owned: HashMap<String, usize>,
    /// Keys of the selected rows, all of one class (Spec 020): no command
    /// takes a mix.
    selected: HashSet<String>,
    /// Key of the row the selection was last put on, by a click, a `Ctrl`
    /// click that adds or an arrow key: `Shift` selects from it, and the
    /// arrow keys move from it.
    pub(super) cursor: Option<String>,
    /// Where the rows are, in a pane `room` wide.
    board: Board,
    room: f32,
    /// The diff of each row the inspector has shown, by row key: git is
    /// asked once.
    diffs: HashMap<String, String>,
    /// How many times the repository was read: the diff of a row the
    /// inspector still shows may have changed with it.
    reads: u64,
    /// Spaces each tab of a diff shows as.
    tab_width: usize,
    /// The command running, and the log of those that ran.
    pub(super) worker: Worker,
    /// How the last command went, or what was refused since one started.
    notice: Option<Notice>,
    /// The question asked over everything else.
    pub(super) form: Option<Form>,
    pub(super) forms_opened: u64,
    /// Whether the log is shown over everything else.
    log_open: bool,
    /// What the next commit's dialog starts from.
    pub(super) draft: Draft,
    /// The row the reload after a command selects: the branch it creates or
    /// renames.
    pub(super) next_cursor: Option<String>,
}

impl State {
    /// The repository the process runs in, as it stands.
    pub(crate) fn load() -> Result<State> {
        let opened = session::open()?;
        let tab_width = diff_text::tab_width(&opened.repo);
        Ok(State::new(
            opened.snapshot,
            opened.paused,
            opened.context,
            tab_width,
        ))
    }

    fn new(
        snapshot: Snapshot,
        paused: Option<PausedOp>,
        context: usize,
        tab_width: usize,
    ) -> State {
        let mut state = State {
            snapshot,
            paused,
            context,
            rows: Vec::new(),
            expanded: HashSet::from([tree::LOCAL_CHANGES_KEY.to_string()]),
            collapsed: HashSet::new(),
            owned: HashMap::new(),
            selected: HashSet::new(),
            cursor: None,
            board: Board::default(),
            room: 0.0,
            diffs: HashMap::new(),
            reads: 0,
            tab_width,
            worker: Worker::new("gui", hint_words),
            notice: None,
            form: None,
            forms_opened: 0,
            log_open: false,
            draft: Draft::new(),
            next_cursor: None,
        };
        state.rebuild_rows();
        state
    }

    /// The repository as the window's title names it: the directory of its
    /// working tree.
    pub(crate) fn repository(&self) -> String {
        let workdir = &self.snapshot.workdir;
        match workdir.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => workdir.display().to_string(),
        }
    }

    /// What the window is open on: the integration branch, or the operation
    /// paused on it.
    pub(crate) fn heading(&self) -> String {
        match &self.paused {
            Some(paused) => paused.title(),
            None => self.integration_branch().to_string(),
        }
    }

    pub(crate) fn integration_branch(&self) -> &str {
        &self.snapshot.info.branch_name
    }

    /// The directory the paths the window shows are relative to.
    pub(crate) fn cwd_prefix(&self) -> &str {
        &self.snapshot.cwd_prefix
    }

    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// How many commits the branch `row` names owns.
    pub(crate) fn owned(&self, row: &Row) -> usize {
        self.owned.get(&row.key).copied().unwrap_or(0)
    }

    pub(crate) fn board(&self) -> &Board {
        &self.board
    }

    /// The board's pane is `room` wide: its frames follow.
    pub(crate) fn set_room(&mut self, room: f32) {
        if room != self.room {
            self.room = room;
            self.lay_out();
        }
    }

    pub(crate) fn is_selected(&self, row: &Row) -> bool {
        self.selected.contains(&row.key)
    }

    /// The selected rows, in tree order.
    pub(super) fn selection(&self) -> Vec<&Row> {
        self.rows
            .iter()
            .filter(|row| self.is_selected(row))
            .collect()
    }

    /// What the selected rows are: `None` with none selected, `Some(None)`
    /// for the upstream, which is of no class.
    fn selected_class(&self) -> Option<Option<SelectionClass>> {
        let selected = self.rows.iter().find(|row| self.is_selected(row));
        selected.map(Row::selection_class)
    }

    fn cursor_index(&self) -> Option<usize> {
        let key = self.cursor.as_ref()?;
        self.rows.iter().position(|row| row.key == *key)
    }

    /// Where the row the selection was last put on is while it is selected,
    /// for the window to keep it in view.
    pub(crate) fn cursor_rect(&self) -> Option<Rect> {
        let cursor = self.cursor_index()?;
        if !self.is_selected(&self.rows[cursor]) {
            return None;
        }
        let mut pieces = self.board.pieces.iter();
        pieces
            .find(|piece| piece.row == Some(cursor))
            .map(|piece| piece.rect)
    }

    /// Where the row the selection was last put on is among the files of the
    /// local changes pane while it is selected, for the window to keep it in
    /// view.
    pub(crate) fn cursor_change(&self) -> Option<usize> {
        let cursor = self.cursor_index()?;
        if !self.is_selected(&self.rows[cursor]) {
            return None;
        }
        self.local_files().position(|row| row == cursor)
    }

    /// The row heading the local changes pane, which stands for them all.
    /// None while an operation is paused, whose snapshot is a placeholder.
    pub(crate) fn local_changes(&self) -> Option<usize> {
        let header = self
            .rows
            .iter()
            .position(|row| matches!(row.kind, RowKind::LocalChanges { .. }));
        header.filter(|_| self.paused.is_none())
    }

    /// The rows of the files the local changes pane lists; none while an
    /// operation is paused.
    pub(crate) fn local_files(&self) -> impl Iterator<Item = usize> + '_ {
        let paused = self.paused.is_some();
        let rows = self.rows.iter().enumerate();
        let files =
            rows.filter(move |(_, row)| !paused && matches!(row.kind, RowKind::WorkingFile { .. }));
        files.map(|(index, _)| index)
    }

    /// The rows the local changes pane lists: its header, then each file.
    fn listed(&self) -> impl Iterator<Item = usize> + '_ {
        self.local_changes().into_iter().chain(self.local_files())
    }

    /// The rows the board has a piece for.
    fn on_board(&self) -> impl Iterator<Item = usize> + '_ {
        self.board.pieces.iter().filter_map(|piece| piece.row)
    }

    /// The rows a click or a key can select: those the local changes pane
    /// lists, and those the board has a piece for.
    pub(super) fn selectable(&self) -> impl Iterator<Item = usize> + '_ {
        self.listed().chain(self.on_board())
    }

    pub(super) fn select_only(&mut self, index: usize) {
        let key = self.rows[index].key.clone();
        self.selected = HashSet::from([key.clone()]);
        self.cursor = Some(key);
    }

    /// `Ctrl` click: add the row to the selection or take it out. A row of
    /// another class than the selection's is left out.
    fn toggle_selected(&mut self, index: usize) {
        let row = &self.rows[index];
        let key = row.key.clone();
        if self.selected.remove(&key) {
            return;
        }
        if self
            .selected_class()
            .is_some_and(|class| class != row.selection_class())
        {
            return;
        }
        self.selected.insert(key.clone());
        self.cursor = Some(key);
    }

    /// `Shift` click: select from the row the selection was last put on to
    /// the one at `index`, keeping to the class of the first.
    fn extend_selection(&mut self, index: usize) {
        let class = |row: usize| self.rows[row].selection_class();
        let Some(from) = self.cursor_index().filter(|from| class(*from).is_some()) else {
            return self.select_only(index);
        };
        let (low, high) = (from.min(index), from.max(index));
        let between = self.rows[low..=high].iter();
        let of_its_class = between.filter(|row| row.selection_class() == class(from));
        self.selected = of_its_class.map(|row| row.key.clone()).collect();
    }

    /// A press on what stands for the row at `index`: a line of the local
    /// changes pane or a piece of the board.
    pub(crate) fn row_pressed(&mut self, index: usize, control: bool, shift: bool) {
        if !self.selectable().any(|row| row == index) {
            return;
        }
        if control {
            self.toggle_selected(index);
        } else if shift {
            self.extend_selection(index);
        } else {
            self.select_only(index);
        }
    }

    /// `↓` and `↑`: select the next row the panes show, or the previous one,
    /// alone; at either end, the one the selection was last put on.
    pub(crate) fn move_cursor(&mut self, down: bool) {
        let next = match (self.cursor_index(), down) {
            (Some(from), true) => self.selectable().filter(|row| *row > from).min(),
            (Some(from), false) => self.selectable().filter(|row| *row < from).max(),
            (None, true) => self.selectable().min(),
            (None, false) => self.selectable().max(),
        };
        if let Some(next) = next.or(self.cursor_index()) {
            self.select_only(next);
        }
    }

    /// `→`: open the row the selection was last put on.
    pub(crate) fn expand_cursor(&mut self) {
        if let Some(cursor) = self.cursor_index().filter(|i| !self.rows[*i].expanded) {
            self.row_toggled(cursor);
        }
    }

    /// `←`: close the row the selection was last put on, or go from a file
    /// up to its commit or to the local changes.
    pub(crate) fn collapse_cursor(&mut self) {
        let Some(cursor) = self.cursor_index() else {
            return;
        };
        let row = &self.rows[cursor];
        if row.expandable && row.expanded {
            return self.row_toggled(cursor);
        }
        if matches!(
            row.kind,
            RowKind::CommitFile { .. } | RowKind::WorkingFile { .. }
        ) && let Some(holder) = self.rows[..cursor].iter().rposition(|r| r.expandable)
        {
            self.select_only(holder);
        }
    }

    /// `Esc`: select nothing.
    pub(crate) fn clear_selection(&mut self) {
        self.selected.clear();
    }

    pub(crate) fn tab_width(&self) -> usize {
        self.tab_width
    }

    /// The row the inspector shows, its diff read from git the first time:
    /// the one the selection was last put on while it is selected, else the
    /// first selected one.
    pub(crate) fn inspected(&mut self) -> Option<Inspected<'_>> {
        let selected = |index: &usize| self.is_selected(&self.rows[*index]);
        let last_put_on = self.cursor_index().filter(selected);
        let index = last_put_on.or_else(|| (0..self.rows.len()).find(selected))?;
        let row = &self.rows[index];
        if !self.diffs.contains_key(&row.key) {
            let diff = diff_text(&self.snapshot, row);
            self.diffs.insert(row.key.clone(), diff);
        }
        Some(Inspected {
            key: &row.key,
            title: self.title(row),
            diff: &self.diffs[&row.key],
        })
    }

    /// How the inspector names `row`.
    fn title(&self, row: &Row) -> String {
        match &row.kind {
            RowKind::LocalChanges { .. } => "Local changes".to_string(),
            RowKind::WorkingFile { path, .. } => {
                repo::cwd_relative_path(path, &self.snapshot.cwd_prefix)
            }
            RowKind::BranchName { name, .. } => format!("Branch {name}"),
            RowKind::Commit { message, .. } => format!("{}  {}", row.sid, message),
            RowKind::CommitFile { path, oid, .. } => {
                let path = repo::cwd_relative_path(path, &self.snapshot.cwd_prefix);
                format!("{}  in {}", path, self.snapshot.ids.get_commit(*oid))
            }
            RowKind::Upstream { label, .. } => format!("Upstream {label}"),
            // No other row is shown to select.
            _ => String::new(),
        }
    }

    /// Open or close the row at `index`: a commit onto its files, a branch
    /// onto its name.
    pub(crate) fn row_toggled(&mut self, index: usize) {
        let Some(row) = self.rows.get(index).filter(|r| r.expandable) else {
            return;
        };
        let key = row.key.clone();
        match row.kind {
            RowKind::Commit { .. } => {
                if !self.expanded.remove(&key) {
                    self.expanded.insert(key);
                }
            }
            RowKind::BranchName { .. } if row.expanded => {
                self.collapsed.insert(key);
            }
            RowKind::BranchName { .. } => {
                // Any of the names over these commits may be what hides them.
                let is_name = |r: &&Row| matches!(r.kind, RowKind::BranchName { .. });
                let over = self.rows[..index].iter().rev().take_while(is_name);
                let under = self.rows[index..].iter().take_while(is_name);
                for name in over.chain(under) {
                    self.collapsed.remove(&name.key);
                }
            }
            _ => return,
        }
        self.rebuild_rows();
    }

    fn rebuild_rows(&mut self) {
        // The cursor's row and those above it, nearest first: where the
        // arrow keys go on from if that row goes.
        let above: Vec<String> = match self.cursor_index() {
            Some(cursor) => self.rows[..=cursor]
                .iter()
                .rev()
                .map(|row| row.key.clone())
                .collect(),
            None => Vec::new(),
        };
        let sections = self.snapshot.sections(None);
        let (ids, prefix) = (&self.snapshot.ids, &self.snapshot.cwd_prefix);
        self.rows = tree::build_rows(&sections, ids, prefix, &self.expanded);
        self.owned = close_branches(&mut self.rows, &self.collapsed);
        // A row that went away takes its selection with it.
        let present: HashSet<&str> = self.rows.iter().map(|row| row.key.as_str()).collect();
        self.selected.retain(|key| present.contains(key.as_str()));
        self.lay_out();
        let shown: HashSet<&str> = self
            .selectable()
            .map(|row| self.rows[row].key.as_str())
            .collect();
        let cursor = above.into_iter().find(|key| shown.contains(key.as_str()));
        self.cursor = cursor;
    }

    /// Place the rows in the room there is. A paused operation leaves the
    /// board bare: its snapshot is a placeholder.
    fn lay_out(&mut self) {
        self.board = match self.paused {
            Some(_) => Board::default(),
            None => Board::layout(&self.rows, self.room),
        };
    }

    // ── Commands ──

    /// Whether a command runs: no other starts until it ends.
    pub(crate) fn busy(&self) -> bool {
        self.worker.running().is_some()
    }

    /// What the status bar says: what was refused since the last command
    /// started, else the command running, else how the last one went.
    pub(crate) fn status(&self) -> (Level, String) {
        if let Some(notice) = &self.notice {
            return (notice.level, notice.text.clone());
        }
        let Some(running) = self.worker.running() else {
            return (Level::Plain, String::new());
        };
        let text = match &running.spinner {
            Some(step) => format!("{} — {}", running.command, step),
            None => running.command.clone(),
        };
        (Level::Plain, text)
    }

    pub(super) fn say(&mut self, level: Level, text: impl Into<String>) {
        let text = text.into();
        self.notice = Some(Notice { level, text });
    }

    /// Run `action` on the worker: the window goes on, and asks what the
    /// command asks.
    pub(super) fn start(&mut self, action: Action) {
        if self.busy() {
            return;
        }
        let command = action::command_line(&action, |target| tree::short_id(&self.rows, target));
        self.notice = None;
        // What the theme is for, a picker drawn in a terminal, no command
        // the window runs draws.
        let theme = graph::Theme::dark();
        let git_dir = &self.snapshot.git_dir;
        self.worker.start(action, command, git_dir, &theme);
    }

    /// Take in what the running command sent, and how it ended; whether
    /// anything shown changed.
    pub(crate) fn tick(&mut self) -> bool {
        let mut changed = false;
        while let Some(event) = self.worker.poll() {
            match event {
                Event::Ask(ask) => self.open_form(Form::prompt(ask)),
                // Every message is typed in the window: no command it runs
                // opens an editor, which would find no terminal to take.
                Event::Suspend(ack) => {
                    let _ = ack.send(());
                }
                Event::Filed => {}
            }
            changed = true;
        }
        if let Some(ended) = self.worker.reap() {
            if ended != Ended::Done {
                self.next_cursor = None;
            }
            let committed = std::mem::take(&mut self.draft.running) && ended == Ended::Done;
            // What a command that failed did on the way is shown too. A
            // repository that cannot be read is said in place of how the
            // command went, which the log has.
            if ended == Ended::Cancelled || self.reload() {
                self.report(ended);
            }
            if committed {
                self.draft.message.clear();
            }
            changed = true;
        }
        changed
    }

    /// Say how the command ended: in the status bar, and over everything
    /// when it failed.
    pub(super) fn report(&mut self, ended: Ended) {
        match ended {
            Ended::Done => {
                let (level, text) = match self.worker.summary() {
                    Summary::Done => (Level::Success, "Done".to_string()),
                    Summary::Warned(warning) => (Level::Warn, warning.to_string()),
                    Summary::Succeeded { last, more: 0 } => (Level::Success, last.to_string()),
                    Summary::Succeeded { last, more } => (
                        Level::Success,
                        format!("{last} (and {more} more in the log)"),
                    ),
                };
                self.say(level, text);
            }
            Ended::Cancelled => self.say(Level::Plain, "Cancelled"),
            Ended::Failed(text) => {
                self.say(Level::Error, text.lines().next().unwrap_or_default());
                // An open log already shows it, under what is being read.
                if !self.log_open {
                    let entry = self.worker.log.last();
                    let command = entry.map_or("", |entry| entry.command.as_str());
                    self.open_form(Form::notice(command, &text));
                }
            }
        }
    }

    /// Read the repository again, as a command left it. What is open stays
    /// open; a selection goes to the row of the cursor, else to the one now
    /// in its place or the nearest above it, and a branch the command
    /// created or renamed is selected. False when it could not be read, and
    /// the window shows it as it was.
    fn reload(&mut self) -> bool {
        let workdir = self.snapshot.workdir.clone();
        let git_dir = self.snapshot.git_dir.clone();
        if let Some(paused) = PausedOp::load(&workdir, &git_dir) {
            self.paused = Some(paused);
            self.apply(Snapshot::placeholder(workdir, git_dir));
            return true;
        }
        match load_snapshot(self.context) {
            Ok(snapshot) => {
                self.paused = None;
                self.apply(snapshot);
                true
            }
            Err(e) => {
                self.next_cursor = None;
                self.cannot_read(&e.to_string());
                false
            }
        }
    }

    /// Say over everything that the repository could not be read: `text`.
    fn cannot_read(&mut self, text: &str) {
        self.say(Level::Error, text.lines().next().unwrap_or_default());
        self.open_form(Form::notice("", text));
    }

    /// Show `snapshot` in place of the one shown.
    fn apply(&mut self, snapshot: Snapshot) {
        let followed = self.next_cursor.take();
        let selecting = followed.is_some() || !self.selected.is_empty();
        let key = followed.or_else(|| self.cursor.clone());
        let place = self.cursor_index();
        self.snapshot = snapshot;
        self.diffs.clear();
        self.selected.clear();
        self.reads += 1;
        self.rebuild_rows();
        if !selecting {
            return;
        }
        let shown: Vec<usize> = self.selectable().collect();
        let same = key.and_then(|key| shown.iter().copied().find(|i| self.rows[*i].key == key));
        let above = place.and_then(|place| shown.iter().copied().filter(|i| *i <= place).max());
        let first = shown.iter().copied().min();
        if let Some(index) = same.or(above).or(first) {
            self.select_only(index);
        }
    }

    /// How many times the repository was read since the window opened.
    pub(crate) fn reads(&self) -> u64 {
        self.reads
    }

    pub(crate) fn log(&self) -> &[LogEntry] {
        &self.worker.log
    }

    pub(crate) fn log_open(&self) -> bool {
        self.log_open
    }

    pub(crate) fn open_log(&mut self) {
        self.log_open = true;
    }

    pub(crate) fn close_log(&mut self) {
        self.log_open = false;
    }

    /// The window is asked to close: not while a command runs, whose git
    /// would be killed mid-way.
    pub(crate) fn may_close(&mut self) -> bool {
        if self.busy() {
            self.say(Level::Warn, "A command is running: wait for it to end");
        }
        !self.busy()
    }
}

/// How the window words what a command hints at.
fn hint_words(hint: Hint) -> &'static str {
    match hint {
        Hint::ForcePush => "pick Force push in its menu",
        Hint::Reload => "The window shows it as it is now",
    }
}

/// Take the commits of the branches named in `collapsed` out of `rows`, and
/// mark each branch row open or closed, which only one owning commits can
/// be; returns how many each owns, by row key. Names at the same commit close
/// together.
fn close_branches(rows: &mut Vec<Row>, collapsed: &HashSet<String>) -> HashMap<String, usize> {
    let mut owned = HashMap::new();
    let mut kept: Vec<Row> = Vec::with_capacity(rows.len());
    // The names over the commits being read, as indices into `kept`.
    let mut names: Vec<usize> = Vec::new();
    let mut count = 0;
    let mut hidden = false;
    // The `None` after the last row settles the names still open.
    for row in rows.drain(..).map(Some).chain([None]) {
        let kind = row.as_ref().map(|r| &r.kind);
        if !names.is_empty()
            && matches!(
                kind,
                Some(RowKind::Commit { .. } | RowKind::CommitFile { .. })
            )
        {
            if matches!(kind, Some(RowKind::Commit { .. })) {
                count += 1;
            }
            if let Some(row) = row.filter(|_| !hidden) {
                kept.push(row);
            }
            continue;
        }
        let name = matches!(kind, Some(RowKind::BranchName { .. }));
        if !name || count > 0 {
            for i in names.drain(..) {
                kept[i].expandable = count > 0;
                kept[i].expanded = !hidden;
                owned.insert(kept[i].key.clone(), count);
            }
            count = 0;
            hidden = false;
        }
        let Some(row) = row else {
            break;
        };
        if name {
            hidden |= collapsed.contains(&row.key);
            names.push(kept.len());
        }
        kept.push(row);
    }
    *rows = kept;
    owned
}

#[cfg(test)]
#[path = "state_test.rs"]
pub(crate) mod tests;
