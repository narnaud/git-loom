//! What the rows picked in a session can be asked to do: the rows each
//! command takes, the `Action` it makes of them, and why a pick has none.
//!
//! `rows` is the selection in tree order, else the row under the cursor;
//! `selecting` says which of the two. Where the rows can go from there is
//! `targets`; a place walked to or a name typed is the front-end's to add
//! to the `Action`.

use crate::core::repo::{self, RepoInfo};
use crate::session::Refusal;
use crate::session::action::Action;
use crate::session::paused::PausedOp;
use crate::session::snapshot::Snapshot;
use crate::session::targets;
use crate::session::tree::{Row, RowKind};

/// The working files `loom commit` takes from `rows`, as its arguments.
pub(crate) fn commit_files(rows: &[&Row]) -> Result<Vec<String>, Refusal> {
    if rows.is_empty()
        || !rows.iter().all(|r| {
            matches!(
                r.kind,
                RowKind::WorkingFile { .. } | RowKind::LocalChanges { .. }
            )
        })
    {
        return Err("move to local changes or select files");
    }
    if rows
        .iter()
        .any(|r| matches!(r.kind, RowKind::LocalChanges { count: 0 }))
    {
        return Err("no local changes");
    }
    // A row of these kinds always carries a target; refuse rather than
    // fall through to an argument-less `loom commit`, which would commit
    // the index — the one thing a commit of files must never do. An unnamed
    // row is the same refusal: `loom commit -l ""` is not an improvement.
    rows.iter()
        .map(|r| r.target.clone().filter(|t| !t.is_empty()))
        .collect::<Option<Vec<String>>>()
        .ok_or("this row has nothing to commit")
}

/// What `loom fold` takes from `rows` as its sources; where they can go is
/// `targets::fold_targets`.
pub(crate) fn fold_sources(rows: &[&Row]) -> Result<Vec<String>, Refusal> {
    let sources: Vec<String> = rows.iter().filter_map(|r| r.target.clone()).collect();
    if sources.is_empty() || sources.len() != rows.len() {
        return Err("select source rows first");
    }
    Ok(sources)
}

/// The one commit among `rows` to move; where it can go is
/// `targets::move_slots`.
pub(crate) fn move_source(rows: &[&Row]) -> Result<git2::Oid, Refusal> {
    match rows {
        [] => Err("move to a commit"),
        [row] => match row.kind {
            RowKind::Commit { oid, .. } if row.target.is_some() => Ok(oid),
            _ => Err("move to a commit"),
        },
        _ => Err("one commit at a time"),
    }
}

/// Where `loom branch` starts a branch from `row`: its `--on` argument and
/// the commit the branch will point at, which is the row's commit or branch
/// tip, else the base with no argument.
pub(crate) fn branch_base(info: &RepoInfo, row: &Row) -> (Option<String>, git2::Oid) {
    let base = info.upstream.merge_base_oid;
    match &row.kind {
        RowKind::Commit { oid, .. } => (row.target.clone(), *oid),
        RowKind::BranchName { name, .. } => {
            let tip = info
                .branches
                .iter()
                .find(|b| b.name == *name)
                .map_or(base, |b| b.tip_oid);
            (row.target.clone(), tip)
        }
        _ => (None, base),
    }
}

/// How `rows` split their commit.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Split {
    /// By file: the commit files among the rows make the first commit, the
    /// rest of their commit the second.
    Files(Action),
    /// A commit row names no files to take out: its hunks are to be picked.
    Hunks(git2::Oid),
}

pub(crate) fn split(snapshot: &Snapshot, rows: &[&Row]) -> Result<Split, Refusal> {
    let commit = targets::split_source(rows)?;
    let files: Vec<String> = rows
        .iter()
        .filter_map(|r| match &r.kind {
            RowKind::CommitFile { path, .. } => {
                Some(repo::cwd_relative_path(path, &snapshot.cwd_prefix))
            }
            _ => None,
        })
        .collect();
    if files.is_empty() {
        return Ok(Split::Hunks(commit));
    }
    let total = snapshot
        .info
        .commits
        .iter()
        .find(|c| c.oid == commit)
        .map_or(0, |c| c.files.len());
    if total < 2 {
        return Err("one file only, split its hunks with S");
    }
    if files.len() >= total {
        return Err("leave at least one file for the second commit");
    }
    Ok(Split::Files(Action::Split {
        commit: commit.to_string(),
        files,
        hunks: None,
    }))
}

/// `loom drop` of `rows`: working files together, else one commit, branch,
/// working file, or every local change. Only files go together, as on the
/// CLI.
pub(crate) fn drop(rows: &[&Row]) -> Result<Action, Refusal> {
    let droppable = |row: &Row| {
        matches!(
            row.kind,
            RowKind::Commit { .. }
                | RowKind::BranchName { .. }
                | RowKind::WorkingFile { .. }
                | RowKind::LocalChanges { .. }
        ) && row.target.is_some()
    };
    if rows.is_empty() || !rows.iter().all(|r| droppable(r)) {
        return Err("move to a commit, branch, file, or local changes");
    }
    if rows.len() > 1
        && !rows
            .iter()
            .all(|r| matches!(r.kind, RowKind::WorkingFile { .. }))
    {
        return Err("only files can be dropped together");
    }
    let targets = rows.iter().filter_map(|r| r.target.clone()).collect();
    Ok(Action::Drop { targets })
}

/// `loom absorb` of `rows`: the working files they are, else every local
/// change. A selection of anything else is refused rather than widened.
pub(crate) fn absorb(info: &RepoInfo, rows: &[&Row], selecting: bool) -> Result<Action, Refusal> {
    if info.working_changes.is_empty() {
        return Err("no local changes");
    }
    let is_file = |r: &&Row| matches!(r.kind, RowKind::WorkingFile { .. });
    // An empty selection, or an unnamed file, would stand for every change.
    if selecting
        && (rows.is_empty()
            || !rows
                .iter()
                .all(|r| is_file(r) || matches!(r.kind, RowKind::LocalChanges { .. })))
    {
        return Err("select files or local changes");
    }
    let files = if !rows.is_empty() && rows.iter().all(is_file) {
        rows.iter()
            .map(|r| r.target.clone().filter(|t| !t.is_empty()))
            .collect::<Option<Vec<String>>>()
            .ok_or("this row has nothing to absorb")?
    } else {
        Vec::new()
    };
    Ok(Action::Absorb { files })
}

/// The one branch a command takes from `rows`: the branch they are, else
/// none, which leaves the command to ask. A selection of anything else is
/// refused.
pub(crate) fn one_branch(rows: &[&Row], selecting: bool) -> Result<Option<String>, Refusal> {
    match rows {
        [row] if matches!(row.kind, RowKind::BranchName { .. }) => Ok(row.target.clone()),
        _ if selecting => Err("select one branch"),
        _ => Ok(None),
    }
}

/// `loom push` of `branch`; a force names the branch it overwrites.
pub(crate) fn push(branch: Option<String>, force: bool) -> Result<Action, Refusal> {
    match (branch, force) {
        (branch, false) => Ok(Action::Push { branch }),
        (Some(branch), true) => Ok(Action::ForcePush { branch }),
        (None, true) => Err("move to a branch"),
    }
}

/// The branch `rows` are when it has an upstream, whose remote version
/// `loom update <branch>` can take.
pub(crate) fn remote_branch(rows: &[&Row]) -> Option<String> {
    match rows {
        [row]
            if matches!(
                row.kind,
                RowKind::BranchName {
                    remote: Some(_),
                    ..
                }
            ) =>
        {
            row.target.clone()
        }
        _ => None,
    }
}

/// What rewording `row` takes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reword {
    /// A commit: the command asks for the message itself.
    Commit(Action),
    /// A branch, `branch` as the command names it: the new name is asked
    /// for first, starting from `name`.
    Branch { branch: String, name: String },
}

pub(crate) fn reword(row: &Row) -> Result<Reword, Refusal> {
    match (&row.kind, row.target.clone()) {
        (RowKind::Commit { .. }, Some(target)) => {
            Ok(Reword::Commit(Action::Reword { target, name: None }))
        }
        (RowKind::BranchName { name, .. }, Some(branch)) => Ok(Reword::Branch {
            branch,
            name: name.clone(),
        }),
        _ => Err("move to a commit or branch"),
    }
}

/// `loom continue` of the operation `op` shows.
pub(crate) fn resume(op: &PausedOp) -> Action {
    Action::Continue {
        paused: op.id.clone(),
    }
}

/// `loom abort` of the operation `op` shows, once confirmed.
pub(crate) fn abort(op: &PausedOp) -> Action {
    Action::Abort {
        question: op.abort_question(),
        paused: op.id.clone(),
    }
}

#[cfg(test)]
#[path = "commands_test.rs"]
mod tests;
