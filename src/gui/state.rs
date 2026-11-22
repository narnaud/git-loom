//! What the window shows of the repository, apart from the window: the tests
//! drive this, and none opens a window.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::gui::board::{Board, Rect};
use crate::session;
use crate::session::paused::PausedOp;
use crate::session::snapshot::Snapshot;
use crate::session::tree::{self, Row, RowKind, SelectionClass};

pub(crate) struct State {
    snapshot: Snapshot,
    /// Set while an operation is paused.
    paused: Option<PausedOp>,
    /// The tree of the session, whose branches and commits the board places:
    /// the commits of a closed branch are not in it.
    rows: Vec<Row>,
    /// Keys of the commit rows open onto their files.
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
}

impl State {
    /// The repository the process runs in, as it stands.
    pub(crate) fn load() -> Result<State> {
        let opened = session::open()?;
        Ok(State::new(opened.snapshot, opened.paused))
    }

    fn new(snapshot: Snapshot, paused: Option<PausedOp>) -> State {
        let mut state = State {
            snapshot,
            paused,
            rows: Vec::new(),
            expanded: HashSet::new(),
            collapsed: HashSet::new(),
            owned: HashMap::new(),
            selected: HashSet::new(),
            cursor: None,
            board: Board::default(),
            room: 0.0,
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

    /// The rows the board has a piece for, which are those a click or a key
    /// can select.
    fn on_board(&self) -> impl Iterator<Item = usize> + '_ {
        self.board.pieces.iter().filter_map(|piece| piece.row)
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

    /// A press on the piece of the row at `index`.
    pub(crate) fn row_pressed(&mut self, index: usize, control: bool, shift: bool) {
        if !self.on_board().any(|row| row == index) {
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

    /// `↓` and `↑`: select the next piece down the board, or up it, alone;
    /// at either end, the one the selection was last put on.
    pub(crate) fn move_cursor(&mut self, down: bool) {
        let next = match (self.cursor_index(), down) {
            (Some(from), true) => self.on_board().filter(|row| *row > from).min(),
            (Some(from), false) => self.on_board().filter(|row| *row < from).max(),
            (None, true) => self.on_board().min(),
            (None, false) => self.on_board().max(),
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
    /// up to its commit.
    pub(crate) fn collapse_cursor(&mut self) {
        let Some(cursor) = self.cursor_index() else {
            return;
        };
        let row = &self.rows[cursor];
        if row.expandable && row.expanded {
            return self.row_toggled(cursor);
        }
        if matches!(row.kind, RowKind::CommitFile { .. })
            && let Some(commit) = self.rows[..cursor].iter().rposition(|r| r.expandable)
        {
            self.select_only(commit);
        }
    }

    /// `Esc`: select nothing.
    pub(crate) fn clear_selection(&mut self) {
        self.selected.clear();
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
        self.rows = tree::build_rows(&sections, &self.snapshot.ids, &self.expanded);
        self.owned = close_branches(&mut self.rows, &self.collapsed);
        // A row that went away takes its selection with it.
        let present: HashSet<&str> = self.rows.iter().map(|row| row.key.as_str()).collect();
        self.selected.retain(|key| present.contains(key.as_str()));
        self.lay_out();
        let on_board: HashSet<&str> = self
            .on_board()
            .map(|row| self.rows[row].key.as_str())
            .collect();
        let cursor = above
            .into_iter()
            .find(|key| on_board.contains(key.as_str()));
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
