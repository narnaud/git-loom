//! What the window shows of the repository, apart from the window: the tests
//! drive this, and none opens a window.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::gui::board::Board;
use crate::session;
use crate::session::paused::PausedOp;
use crate::session::snapshot::Snapshot;
use crate::session::tree::{self, Row, RowKind};

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
        let sections = self.snapshot.sections(None);
        self.rows = tree::build_rows(&sections, &self.snapshot.ids, &self.expanded);
        self.owned = close_branches(&mut self.rows, &self.collapsed);
        self.lay_out();
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
