//! An interactive session over the repository, apart from the front-end that
//! draws it: the snapshot of the repo it shows, the rows of its tree, their
//! diffs and where they can go, what each command takes of them, the hunks
//! picked before some, the actions it runs and the worker running them, what
//! stands in for the tree while an operation is paused, and what says the
//! snapshot is stale.

pub(crate) mod action;
pub(crate) mod commands;
pub(crate) mod diff_text;
pub(crate) mod paused;
pub(crate) mod pick;
pub(crate) mod snapshot;
pub(crate) mod targets;
pub(crate) mod tree;
pub(crate) mod watch;
pub(crate) mod worker;

use anyhow::Result;

use crate::core::repo;
use crate::core::transaction;
use crate::session::paused::PausedOp;
use crate::session::snapshot::{Snapshot, load_snapshot};
use crate::session::watch::{RepoStamp, repo_stamp};

/// Why a command takes nothing from the rows it was given, worded to follow
/// the command's name.
pub(crate) type Refusal = &'static str;

/// What a front-end starts from.
pub(crate) struct Opened {
    pub(crate) repo: git2::Repository,
    /// Context depth the tree is loaded with, as `loom status -n <N>` takes it.
    pub(crate) context: usize,
    /// Taken before anything else was read, for the auto-reload watch.
    pub(crate) stamp: RepoStamp,
    /// A placeholder while an operation is paused: HEAD is detached
    /// mid-rebase, so no tree can be gathered until it ends.
    pub(crate) snapshot: Snapshot,
    pub(crate) paused: Option<PausedOp>,
}

/// Open a session on the repository the process runs in.
pub(crate) fn open() -> Result<Opened> {
    let repo = repo::open_repo()?;
    let context = repo::resolve_context(&repo, None);
    let workdir = repo::require_workdir(&repo, "display status")?.to_path_buf();
    let git_dir = repo.path().to_path_buf();
    // `PausedOp::load` reads an unreadable state as no loom command: say so
    // before a front-end takes the screen, as the `cli.rs` guard does.
    transaction::load(&git_dir)?;
    let stamp = repo_stamp(&git_dir);
    let paused = PausedOp::load(&workdir, &git_dir);
    let snapshot = match paused {
        Some(_) => Snapshot::placeholder(workdir, git_dir),
        None => load_snapshot(context)?,
    };
    Ok(Opened {
        repo,
        context,
        stamp,
        snapshot,
        paused,
    })
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
