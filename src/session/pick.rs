//! Picking hunks before a commit, a fold or a split: what a pick is for, what
//! it reads, and the action its selection makes.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

use crate::core::hunk_select::{self, HunkArgs};
use crate::core::hunks::FileEntry;
use crate::core::repo::RepoInfo;
use crate::core::staging;
use crate::session::Refusal;
use crate::session::action::{Action, Stamp};
use crate::session::commands;
use crate::session::snapshot::CommitDest;
use crate::session::targets::{self, FoldEffect, FoldTarget};
use crate::session::tree::{self, Row, RowKind};

/// What a pick's selection goes to.
pub(crate) enum PickFor {
    /// Staged at once, then committed: the TUI's `C`.
    Commit,
    /// Handed to `commit -p` by id, staging nothing: what the pick leaves
    /// out stays as it was, staged or not (Spec 006).
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    CommitHunks,
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
    /// Every local change, staged at once once picked: nothing to stamp.
    Staging,
    /// Every local change, stamped: what is made of the pick once it is
    /// confirmed, a commit by id or a fold once its target is chosen, has to
    /// tell that the tree moved meanwhile (Spec 019).
    Worktree,
    Commit(git2::Oid),
}

impl Read {
    /// The hunks, read in `workdir` rather than the process cwd: the tree
    /// they must agree with is the one shown.
    pub(crate) fn run(self, workdir: &Path) -> Result<(Vec<FileEntry>, Stamp)> {
        match self {
            Read::Staging | Read::Worktree => {
                let repo = git2::Repository::open(workdir)?;
                let entries = staging::collect_file_entries(&repo, workdir, None)?;
                let stamp = match self {
                    Read::Worktree => staging::binary_stamp(workdir, &entries),
                    _ => Vec::new(),
                };
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

    /// A pick to hand to `commit -p` by id, out of every local change
    /// whatever the rows: the ids number the listing it makes of them all.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub(crate) fn commit_hunks(info: &RepoInfo) -> Result<PickFor, Refusal> {
        if info.working_changes.is_empty() {
            return Err("no local changes");
        }
        Ok(PickFor::CommitHunks)
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
        let (mut sources, commit) = fold_source(rows)?;
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

    /// Why [`PickFor::fold`] refuses `rows`, short of the graph walk of its
    /// targets (`targets::can_fold`).
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub(crate) fn can_fold(tree: &[Row], rows: &[&Row]) -> Result<(), Refusal> {
        fold_source(rows)?;
        targets::can_fold(tree, rows)
    }

    /// The command the pick is for.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            PickFor::Commit | PickFor::CommitHunks => "commit",
            PickFor::Fold { .. } => "fold",
            PickFor::Split { .. } => "split",
        }
    }

    /// The CLI line the read is logged under. `all` names every local
    /// change; `sid` is `action::command_line`'s.
    pub(crate) fn command_line(&self, all: &str, sid: impl Fn(&str) -> String) -> String {
        match self {
            PickFor::Commit => format!("loom add -p {}", all),
            PickFor::CommitHunks => "loom commit -p".to_string(),
            PickFor::Fold { sources, .. } => {
                let shown: Vec<String> = sources.iter().map(|s| sid(s)).collect();
                format!("loom fold -p {}", shown.join(" "))
            }
            PickFor::Split { commit } => format!("loom split -p {}", sid(&commit.to_string())),
        }
    }

    /// What reads the hunks to pick from.
    pub(crate) fn read(&self) -> Read {
        match self {
            PickFor::Commit => Read::Staging,
            PickFor::CommitHunks | PickFor::Fold { commit: None, .. } => Read::Worktree,
            PickFor::Fold {
                commit: Some(commit),
                ..
            }
            | PickFor::Split { commit } => Read::Commit(*commit),
        }
    }

    /// Why `entries`, as read, leave nothing to pick: the line for the log,
    /// then the refusal. A split needs two hunks to keep one on each side. A
    /// read of the local changes leaves conflicted files out, so it can find
    /// nothing.
    pub(crate) fn nothing_to_pick(&self, entries: &[FileEntry]) -> Option<(&'static str, Refusal)> {
        let none = ("No changes to pick", "no hunks to pick");
        match self {
            PickFor::Commit => None,
            PickFor::CommitHunks | PickFor::Fold { .. } => entries.is_empty().then_some(none),
            PickFor::Split { .. } => match entries.iter().map(|f| f.hunks.len()).sum::<usize>() {
                0 => Some(none),
                1 => Some(("Only one hunk", "one hunk only, nothing to split")),
                _ => None,
            },
        }
    }
}

/// What a fold of the hunks of `rows` names as its sources, and the commit
/// they are of, none for local changes.
fn fold_source(rows: &[&Row]) -> Result<(Vec<String>, Option<git2::Oid>), Refusal> {
    let sources = commands::fold_sources(rows)?;
    let commit = match rows[0].kind {
        RowKind::Commit { oid, .. } => Some(oid),
        RowKind::LocalChanges { .. } | RowKind::WorkingFile { .. } => None,
        _ => return Err("move to a file, local changes, or a commit"),
    };
    Ok((sources, commit))
}

/// Whether `picked` keeps any hunk; one that keeps none is a pick cancelled.
pub(crate) fn any_picked(picked: &[FileEntry]) -> bool {
    picked.iter().any(|f| f.hunks.iter().any(|h| h.selected))
}

/// Where the hunks `picked` keeps come from, as row keys: the working files
/// they are of, or the commit `commit`, then the files of that commit they
/// are of.
pub(crate) fn sources(
    info: &RepoInfo,
    commit: Option<git2::Oid>,
    picked: &[FileEntry],
) -> (HashSet<String>, HashSet<String>) {
    let paths: HashSet<&str> = (picked.iter())
        .filter(|f| f.hunks.iter().any(|h| h.selected))
        .map(|f| f.path.as_str())
        .collect();
    let Some(oid) = commit else {
        let named = paths.iter().map(|p| tree::working_file_key(p)).collect();
        return (named, HashSet::new());
    };
    let commit = info.commits.iter().find(|c| c.oid == oid);
    let files = commit.map_or(&[][..], |c| &c.files);
    let covered = (files.iter().enumerate())
        .filter(|(_, f)| paths.contains(f.path.as_str()))
        .map(|(i, _)| tree::commit_file_key(oid, i))
        .collect();
    (HashSet::from([oid.to_string()]), covered)
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

/// `loom commit -p` to `dest` of the working-tree hunks `picked` keeps,
/// read every local change at once and stamped `stamp`, handed over by id
/// (Spec 019): the commit refuses them if the tree moved since. The error
/// is the line for the log, then the refusal.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub(crate) fn commit(
    dest: CommitDest,
    message: String,
    picked: &[FileEntry],
    stamp: &[Option<git2::Oid>],
) -> Result<Action, (&'static str, Refusal)> {
    if !any_picked(picked) {
        return Err(("Nothing kept for the commit", "keep at least one hunk"));
    }
    let from = hunk_select::fingerprint(&staging::stamp_source(stamp), None, picked);
    let ids = hunk_select::picked_ids(picked);
    Ok(Action::CommitHunks {
        dest,
        message,
        hunks: HunkArgs::new(ids, Some(from)),
    })
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
        message: None,
    })
}

#[cfg(test)]
#[path = "pick_test.rs"]
mod tests;
