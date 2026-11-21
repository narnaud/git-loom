//! What the window shows of the repository, apart from the window: the tests
//! drive this, and none opens a window.

use anyhow::Result;

use crate::session;
use crate::session::paused::PausedOp;
use crate::session::snapshot::Snapshot;

pub(crate) struct State {
    snapshot: Snapshot,
    /// Set while an operation is paused.
    paused: Option<PausedOp>,
}

impl State {
    /// The repository the process runs in, as it stands.
    pub(crate) fn load() -> Result<State> {
        let opened = session::open()?;
        Ok(State {
            snapshot: opened.snapshot,
            paused: opened.paused,
        })
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
            None => self.snapshot.info.branch_name.clone(),
        }
    }
}

#[cfg(test)]
#[path = "state_test.rs"]
mod tests;
