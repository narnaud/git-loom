//! What the window shows of the repository, apart from the window: the tests
//! drive this, and none opens a window.

use std::collections::HashSet;

use anyhow::Result;

use crate::gui::board::Board;
use crate::session;
use crate::session::paused::PausedOp;
use crate::session::snapshot::Snapshot;
use crate::session::tree::{self, Row};

pub(crate) struct State {
    snapshot: Snapshot,
    /// Set while an operation is paused.
    paused: Option<PausedOp>,
    /// The tree of the session, whose branches and commits the board places.
    rows: Vec<Row>,
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
        let sections = snapshot.sections(None);
        let mut state = State {
            rows: tree::build_rows(&sections, &snapshot.ids, &HashSet::new()),
            snapshot,
            paused,
            board: Board::default(),
            room: 0.0,
        };
        state.lay_out();
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

    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
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

    /// Place the rows in the room there is. A paused operation leaves the
    /// board bare: its snapshot is a placeholder.
    fn lay_out(&mut self) {
        self.board = match self.paused {
            Some(_) => Board::default(),
            None => Board::layout(&self.rows, self.room),
        };
    }
}

#[cfg(test)]
#[path = "state_test.rs"]
pub(crate) mod tests;
