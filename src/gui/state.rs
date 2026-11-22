//! What the window shows of the repository, apart from the window: the tests
//! drive this, and none opens a window.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::core::repo;
use crate::gui::board::{Board, Rect};
use crate::session;
use crate::session::diff_text::{self, diff_text};
use crate::session::paused::PausedOp;
use crate::session::snapshot::Snapshot;
use crate::session::tree::{self, Row, RowKind, SelectionClass};

/// What the inspector shows of one row.
pub(crate) struct Inspected<'a> {
    /// The key of the row, which tells the window when to build the lines of
    /// another diff.
    pub key: &'a str,
    pub title: String,
    pub diff: &'a str,
}

pub(crate) struct State {
    snapshot: Snapshot,
    /// Set while an operation is paused.
    paused: Option<PausedOp>,
    /// The tree of the session: the local changes pane lists its local
    /// changes, the board places its branches and commits. The commits of a
    /// closed branch are not in it.
    rows: Vec<Row>,
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
    cursor: Option<String>,
    /// Where the rows are, in a pane `room` wide.
    board: Board,
    room: f32,
    /// The diff of each row the inspector has shown, by row key: git is
    /// asked once.
    diffs: HashMap<String, String>,
    /// Spaces each tab of a diff shows as.
    tab_width: usize,
}

impl State {
    /// The repository the process runs in, as it stands.
    pub(crate) fn load() -> Result<State> {
        let opened = session::open()?;
        let tab_width = diff_text::tab_width(&opened.repo);
        Ok(State::new(opened.snapshot, opened.paused, tab_width))
    }

    fn new(snapshot: Snapshot, paused: Option<PausedOp>, tab_width: usize) -> State {
        let mut state = State {
            snapshot,
            paused,
            rows: Vec::new(),
            expanded: HashSet::from([tree::LOCAL_CHANGES_KEY.to_string()]),
            collapsed: HashSet::new(),
            owned: HashMap::new(),
            selected: HashSet::new(),
            cursor: None,
            board: Board::default(),
            room: 0.0,
            diffs: HashMap::new(),
            tab_width,
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
    fn selectable(&self) -> impl Iterator<Item = usize> + '_ {
        self.listed().chain(self.on_board())
    }

    fn select_only(&mut self, index: usize) {
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
