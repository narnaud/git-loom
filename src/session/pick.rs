//! Picking hunks before a commit, a fold or a split: what a pick is for, what
//! it reads, and the action its selection makes.

use std::path::Path;

use anyhow::Result;

use crate::core::hunk_select::{self, HunkArgs};
use crate::core::hunks::FileEntry;
use crate::core::repo::RepoInfo;
use crate::core::staging;
use crate::session::Refusal;
use crate::session::action::{Action, Stamp};
use crate::session::commands;
use crate::session::targets::{self, FoldEffect, FoldTarget};
use crate::session::tree::{Row, RowKind};

/// What a pick's selection goes to.
pub(crate) enum PickFor {
    /// Staged at once, then committed.
    Commit,
    /// Folded into one of `targets`, read off the tree before the hunks so a
    /// pick with nowhere to go never opens. `commit` is the source commit when
    /// the hunks are its own rather than the working tree's.
    Fold {
        sources: Vec<String>,
        targets: Vec<FoldTarget>,
        commit: Option<git2::Oid>,
    },
    /// The hunks of `commit` that make the first of its two halves.
    Split { commit: git2::Oid },
}

/// The read of a pick's hunks: a git call per changed file, so one to run
/// off the front-end's thread.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Read {
    /// The local changes `loom add -p <files>` lists.
    Staging(Vec<String>),
    /// Every local change, stamped: a fold stages its pick only once the
    /// target is chosen, and has to tell that the tree moved meanwhile.
    Worktree,
    Commit(git2::Oid),
}

impl Read {
    /// The hunks, read in `workdir` rather than the process cwd: the tree
    /// they must agree with is the one shown.
    pub(crate) fn run(self, workdir: &Path) -> Result<(Vec<FileEntry>, Stamp)> {
        match self {
            Read::Staging(files) => {
                let repo = git2::Repository::open(workdir)?;
                let filter = staging::filter_paths(&repo, &files)?;
                let entries = staging::collect_file_entries(&repo, workdir, filter.as_deref())?;
                Ok((entries, Vec::new()))
            }
            Read::Worktree => {
                let repo = git2::Repository::open(workdir)?;
                let entries = staging::collect_file_entries(&repo, workdir, None)?;
                let stamp = staging::binary_stamp(workdir, &entries);
                Ok((entries, stamp))
            }
            Read::Commit(oid) => {
                let entries = staging::collect_commit_hunks(workdir, &oid.to_string(), &[])?;
                Ok((entries, Vec::new()))
            }
        }
    }
}

impl PickFor {
    /// A pick to commit, out of every local change whatever the rows: a
    /// picker showing one file would hide the rest of the index it rewrites.
    pub(crate) fn commit(info: &RepoInfo) -> Result<PickFor, Refusal> {
        if info.working_changes.is_empty() {
            return Err("no local changes");
        }
        Ok(PickFor::Commit)
    }

    /// A pick to fold: out of the one commit `rows` are, or, on working
    /// files or local changes, out of every local change, which `all` names.
    /// `tree` and `workdir` are `targets::fold_targets`'s.
    pub(crate) fn fold(
        tree: &[Row],
        workdir: &Path,
        rows: &[&Row],
        all: &str,
    ) -> Result<PickFor, Refusal> {
        let mut sources = commands::fold_sources(rows)?;
        let commit = match rows[0].kind {
            RowKind::Commit { oid, .. } => Some(oid),
            RowKind::LocalChanges { .. } | RowKind::WorkingFile { .. } => None,
            _ => return Err("move to a file, local changes, or a commit"),
        };
        let targets = targets::fold_targets(tree, workdir, rows, true)?;
        if commit.is_none() {
            sources = vec![all.to_string()];
        }
        Ok(PickFor::Fold {
            sources,
            targets,
            commit,
        })
    }

    /// The command the pick is for.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            PickFor::Commit => "commit",
            PickFor::Fold { .. } => "fold",
            PickFor::Split { .. } => "split",
        }
    }

    /// The CLI line the read is logged under. `all` names every local
    /// change; `sid` is `action::command_line`'s.
    pub(crate) fn command_line(&self, all: &str, sid: impl Fn(&str) -> String) -> String {
        match self {
            PickFor::Commit => format!("loom add -p {}", all),
            PickFor::Fold { sources, .. } => {
                let shown: Vec<String> = sources.iter().map(|s| sid(s)).collect();
                format!("loom fold -p {}", shown.join(" "))
            }
            PickFor::Split { commit } => format!("loom split -p {}", sid(&commit.to_string())),
        }
    }

    /// What reads the hunks to pick from; `all` names every local change.
    pub(crate) fn read(&self, all: &str) -> Read {
        match self {
            PickFor::Commit => Read::Staging(vec![all.to_string()]),
            PickFor::Fold { commit: None, .. } => Read::Worktree,
            PickFor::Fold {
                commit: Some(commit),
                ..
            }
            | PickFor::Split { commit } => Read::Commit(*commit),
        }
    }

    /// Why `entries`, as read, leave nothing to pick: the line for the log,
    /// then the refusal. A split needs two hunks to keep one on each side.
    pub(crate) fn nothing_to_pick(&self, entries: &[FileEntry]) -> Option<(&'static str, Refusal)> {
        let none = ("No changes to pick", "no hunks to pick");
        match self {
            PickFor::Commit => None,
            PickFor::Fold { .. } => entries.is_empty().then_some(none),
            PickFor::Split { .. } => match entries.iter().map(|f| f.hunks.len()).sum::<usize>() {
                0 => Some(none),
                1 => Some(("Only one hunk", "one hunk only, nothing to split")),
                _ => None,
            },
        }
    }
}

/// Whether `picked` keeps any hunk; one that keeps none is a pick cancelled.
pub(crate) fn any_picked(picked: &[FileEntry]) -> bool {
    picked.iter().any(|f| f.hunks.iter().any(|h| h.selected))
}

/// The hunks picked for a fold, folded instead of its sources whole.
pub(crate) enum FoldHunks {
    /// Working-tree hunks, staged only once the target is confirmed, so a
    /// cancelled fold leaves the index as it was; `stamp` lets the fold
    /// refuse a pick the tree has moved away from by then.
    Worktree {
        picked: Vec<FileEntry>,
        stamp: Stamp,
    },
    /// Hunks of the source commit, handed to `loom fold -p` by id (Spec 019).
    Commit(Vec<FileEntry>),
}

/// `loom fold` of `sources` into `target`, of `hunks` alone when some were
/// picked. Never a `[NOOP]` target.
pub(crate) fn fold(sources: Vec<String>, target: FoldTarget, hunks: Option<FoldHunks>) -> Action {
    match hunks {
        None => Action::Fold {
            sources,
            target: target.arg,
            hunks: None,
        },
        Some(FoldHunks::Worktree { picked, stamp }) => Action::FoldHunks {
            sources,
            picked,
            stamp,
            target: target.arg,
        },
        Some(FoldHunks::Commit(picked)) => {
            // Numbered against the commit and where its hunks land, as
            // `fold -p` re-reads them; `zz` names no commit.
            let into = (target.effect != FoldEffect::Uncommit).then_some(target.arg.as_str());
            let from = hunk_select::fingerprint(&sources[0], into, &picked);
            let ids = hunk_select::picked_ids(&picked);
            Action::Fold {
                sources,
                target: target.arg,
                hunks: Some(HunkArgs::new(ids, Some(from))),
            }
        }
    }
}

/// `loom split -p` of `commit` at the hunks `picked` keeps, handed over by id
/// (Spec 019). Both halves must keep something, as `split -p` checks: the
/// error is the line for the log, then the refusal.
pub(crate) fn split(
    commit: git2::Oid,
    picked: &[FileEntry],
) -> Result<Action, (&'static str, Refusal)> {
    if !any_picked(picked) {
        return Err((
            "Nothing kept for the first commit",
            "keep at least one hunk for the first commit",
        ));
    }
    if picked.iter().all(|f| f.hunks.iter().all(|h| h.selected)) {
        return Err((
            "Nothing left for the second commit",
            "leave at least one hunk for the second commit",
        ));
    }
    let commit = commit.to_string();
    let from = hunk_select::fingerprint(&commit, None, picked);
    let ids = hunk_select::picked_ids(picked);
    Ok(Action::Split {
        commit,
        files: Vec::new(),
        hunks: Some(HunkArgs::new(ids, Some(from))),
    })
}

#[cfg(test)]
#[path = "pick_test.rs"]
mod tests;
