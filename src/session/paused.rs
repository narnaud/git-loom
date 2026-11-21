//! A paused operation as a session reads it off the repo: HEAD is detached
//! mid-rebase, so no tree can be gathered until the operation ends.

use std::path::Path;
use std::time::SystemTime;

use crate::core::graph;
use crate::core::repo::{self, FileChange};
use crate::core::transaction;
use crate::git;

/// What tells one paused operation from the next, even of the same command:
/// the commit git started it from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PausedId {
    pub(crate) command: Option<String>,
    pub(crate) git_op: Option<&'static str>,
    /// `orig-head` of a rebase, `MERGE_HEAD` of a merge.
    pub(crate) origin: Option<String>,
    /// When the loom state and the origin were written: a restart of the same
    /// command from the same commit writes them anew.
    written: [Option<SystemTime>; 2],
}

impl PausedId {
    /// The paused operation `cli.rs` blocks other commands on, if any.
    pub(crate) fn load(git_dir: &Path) -> Option<PausedId> {
        let command = transaction::load(git_dir).ok().flatten().map(|s| s.command);
        let (git_op, origin) = if git::rebase_is_in_progress(git_dir) {
            let origin = ["rebase-merge", "rebase-apply"]
                .iter()
                .map(|d| git_dir.join(d).join("orig-head"))
                .find(|f| f.exists());
            (Some("rebase"), origin)
        } else if git::merge_is_in_progress(git_dir) {
            (Some("merge"), Some(git_dir.join("MERGE_HEAD")))
        } else {
            (None, None)
        };
        if command.is_none() && git_op.is_none() {
            return None;
        }
        let written = |file: &Path| std::fs::metadata(file).and_then(|m| m.modified()).ok();
        let origin = origin.as_deref();
        Some(PausedId {
            command,
            git_op,
            origin: origin
                .and_then(|f| std::fs::read_to_string(f).ok())
                .map(|o| o.trim().to_string()),
            written: [
                written(&transaction::state_path(git_dir)),
                origin.and_then(written),
            ],
        })
    }
}

/// Where a paused operation stands, for the headline a front-end words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PausedState {
    /// Unmerged paths are left to resolve.
    Conflicts,
    /// Git has neither a rebase nor a merge left: the user finished it by hand.
    NoGitOp,
    /// No conflict left, `AUTO_MERGE` present: resolved, by `rerere` or by hand.
    Resolved,
    /// Stopped part-way for another reason; the trace says which.
    Stopped,
}

/// A paused operation and the working files it leaves to look at.
pub(crate) struct PausedOp {
    /// The command the loom state names; `None` for a git rebase or merge no
    /// state describes.
    pub(crate) command: Option<String>,
    /// `rebase` or `merge`; `None` when git has neither left.
    git_op: Option<&'static str>,
    step: Option<(usize, usize)>,
    pub(crate) state: PausedState,
    pub(crate) files: Vec<FileChange>,
    pub(crate) id: PausedId,
}

impl PausedOp {
    pub(crate) fn load(workdir: &Path, git_dir: &Path) -> Option<PausedOp> {
        let id = PausedId::load(git_dir)?;
        let (command, git_op) = (id.command.clone(), id.git_op);
        let step = match git_op {
            Some("rebase") => git::rebase_progress(git_dir),
            _ => None,
        };
        let files = git2::Repository::open(workdir)
            .ok()
            .and_then(|r| repo::get_working_changes(&r).ok())
            .unwrap_or_default();
        // `AUTO_MERGE` outlives the resolution until the step is committed, so
        // with no conflict left it covers `rerere` and the user's own alike.
        // Conflicts first: a `merge` pauses on its own replay with no git
        // operation behind it (Spec 005).
        let state = if files.iter().any(is_conflicted) {
            PausedState::Conflicts
        } else if git_op.is_none() {
            PausedState::NoGitOp
        } else if git::auto_merge_id(workdir).is_some() {
            PausedState::Resolved
        } else {
            PausedState::Stopped
        };
        Some(PausedOp {
            command,
            git_op,
            step,
            state,
            files,
            id,
        })
    }

    /// What is paused, as the title and the abort question name it.
    pub(crate) fn subject(&self) -> String {
        match (&self.command, self.git_op) {
            (Some(command), _) => format!("`loom {}`", command),
            (None, op) => format!("git {}", op.unwrap_or("rebase")),
        }
    }

    pub(crate) fn title(&self) -> String {
        let step = self
            .step
            .map(|(current, total)| format!(" — step {}/{}", current, total))
            .unwrap_or_default();
        format!("Paused: {}{}", self.subject().replace('`', ""), step)
    }

    pub(crate) fn abort_question(&self) -> String {
        match &self.command {
            Some(_) => format!(
                "Abort the paused {}?\nIt rolls back to where the command started.",
                self.subject()
            ),
            None => format!(
                "Abort the {} in progress?\nNo loom state describes it: only git's own abort runs.",
                self.subject()
            ),
        }
    }
}

pub(crate) fn is_conflicted(change: &FileChange) -> bool {
    graph::file_group(change.index, change.worktree) == graph::FileGroup::Conflicted
}

/// A paused repository, for the tests of what opens on one.
#[cfg(test)]
pub(crate) mod test_support {
    use crate::core::test_helpers::TestRepo;

    /// Dropping `one` pauses: `two` edits the file `one` created, so replaying it
    /// without `one` conflicts. Returns the repo and its HEAD before the drop.
    pub(crate) fn repo_with_a_paused_drop() -> (TestRepo, git2::Oid) {
        let repo = TestRepo::new_with_remote();
        repo.write_file("f.txt", "one\n");
        repo.stage_files(&["f.txt"]);
        repo.commit_staged("one");
        let one = repo.head_oid();
        repo.write_file("f.txt", "two\n");
        repo.stage_files(&["f.txt"]);
        repo.commit_staged("two");
        let head = repo.head_oid();
        repo.in_dir(|| crate::drop::run(vec![one.to_string()], true))
            .unwrap();
        assert!(crate::git::rebase_is_in_progress(repo.repo.path()));
        (repo, head)
    }
}
