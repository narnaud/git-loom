//! Where a session's rows can go: the rows a fold lands on, the places a
//! commit moves to, the commit a split takes.

use std::collections::HashSet;
use std::path::Path;

use crate::core::repo::RepoInfo;
use crate::session::Refusal;
use crate::session::snapshot::{MoveSlot, Preview, Snapshot};
use crate::session::tree::{self, Row, RowKind};

/// What folding the sources into a target does, tagged on the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FoldEffect {
    /// Working changes or a fixup go into the target commit.
    Amend,
    /// The target is a source, or the commit a source file already sits in.
    Noop,
    /// The source commit or commit file goes back to the working tree.
    Uncommit,
    /// A commit file, or a commit's picked hunks, move into the target commit.
    MoveFile,
}

impl FoldEffect {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            FoldEffect::Amend => "[AMEND]",
            FoldEffect::Noop => "[NOOP]",
            FoldEffect::Uncommit => "[UNCOMMIT]",
            FoldEffect::MoveFile => "[MOVE]",
        }
    }
}

/// A row a pending fold can land on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoldTarget {
    pub(crate) key: String,
    /// The row's command argument.
    pub(crate) arg: String,
    pub(crate) effect: FoldEffect,
}

/// The rows `rows` can fold into, in tree order, as `loom fold` accepts
/// them (Spec 007), out of `tree`; the error says why sources have none. Moving
/// commits to a branch is not a fold here: the tree keeps fold to one
/// thing going into another.
///
/// With `hunks`, a commit's picked hunks move out of it rather than the
/// whole commit folding: `[MOVE]` where `f` would say `[AMEND]`.
pub(crate) fn fold_targets(
    tree: &[Row],
    workdir: &Path,
    rows: &[&Row],
    hunks: bool,
) -> Result<Vec<FoldTarget>, Refusal> {
    let target = |row: &Row, effect| {
        row.target.clone().map(|arg| FoldTarget {
            key: row.key.clone(),
            arg,
            effect,
        })
    };
    let targets: Vec<FoldTarget> = match rows[0].kind {
        RowKind::LocalChanges { count: 0 } => return Err("no local changes"),
        RowKind::LocalChanges { .. } | RowKind::WorkingFile { .. } => tree
            .iter()
            .filter(|r| matches!(r.kind, RowKind::Commit { .. }))
            .filter_map(|r| target(r, FoldEffect::Amend))
            .collect(),
        RowKind::Commit { oid: source, .. } => {
            if rows.len() > 1 {
                return Err("one commit at a time");
            }
            // Opened once for the fixup check; a repo that cannot answer
            // leaves the check to the command.
            let repo = git2::Repository::open(workdir).ok();
            let older = |oid: git2::Oid| {
                repo.as_ref()
                    .is_none_or(|repo| repo.graph_descendant_of(source, oid).unwrap_or(true))
            };
            tree.iter()
                .filter_map(|r| match r.kind {
                    RowKind::LocalChanges { .. } => target(r, FoldEffect::Uncommit),
                    RowKind::Commit { oid, .. } if oid == source => target(r, FoldEffect::Noop),
                    // A fixup goes into a commit the source descends from.
                    RowKind::Commit { oid, .. } if older(oid) => target(
                        r,
                        if hunks {
                            FoldEffect::MoveFile
                        } else {
                            FoldEffect::Amend
                        },
                    ),
                    _ => None,
                })
                .collect()
        }
        RowKind::CommitFile { oid: owner, .. } => {
            if rows.len() > 1 {
                return Err("one commit file at a time");
            }
            tree.iter()
                .filter_map(|r| match r.kind {
                    RowKind::LocalChanges { .. } => target(r, FoldEffect::Uncommit),
                    RowKind::Commit { oid, .. } if oid == owner => target(r, FoldEffect::Noop),
                    RowKind::Commit { .. } => target(r, FoldEffect::MoveFile),
                    _ => None,
                })
                .collect()
        }
        RowKind::BranchName { .. } => return Err("a branch cannot be folded"),
        _ => return Err("move to a file, commit, or commit file"),
    };
    if targets.iter().all(|t| t.effect == FoldEffect::Noop) {
        return Err("nothing to fold into");
    }
    Ok(targets)
}

/// The one commit a split of `rows` takes: the commit the rows are, or the one
/// the commit files among them belong to.
pub(crate) fn split_source(rows: &[&Row]) -> Result<git2::Oid, Refusal> {
    let mut commits = HashSet::new();
    for row in rows {
        match row.kind {
            RowKind::Commit { oid, .. } | RowKind::CommitFile { oid, .. }
                if row.target.is_some() =>
            {
                commits.insert(oid);
            }
            _ => return Err("move to a commit or commit file"),
        }
    }
    match commits.into_iter().collect::<Vec<_>>()[..] {
        [commit] => Ok(commit),
        [] => Err("move to a commit or commit file"),
        _ => Err("one commit at a time"),
    }
}

/// Whether `loom fold` moves `commit` to `slot`: it refuses the commit's own
/// place, even where the move would carry a branch name along.
pub(crate) fn can_move(info: &RepoInfo, commit: git2::Oid, slot: &MoveSlot) -> bool {
    let parent_of = |oid: git2::Oid| {
        info.commits
            .iter()
            .find(|c| c.oid == oid)
            .and_then(|c| c.parent_oid)
    };
    match slot {
        MoveSlot::Stay => false,
        MoveSlot::Above(oid) => *oid != commit && parent_of(commit) != Some(*oid),
        MoveSlot::Below(oid) => *oid != commit && parent_of(*oid) != Some(commit),
        MoveSlot::Branch(name) => !info
            .branches
            .iter()
            .any(|b| b.name == *name && b.tip_oid == commit),
    }
}

/// Every distinct place `commit` can be moved to, in tree order, `Stay`
/// among them. The candidates are `--above`/`--below` every other commit
/// and every branch's tip, ranked by the row they sit next to; candidates
/// drawing the same tree are one place, spelled the first way.
pub(crate) fn move_slots(
    snapshot: &Snapshot,
    tree: &[Row],
    expanded: &HashSet<String>,
    commit: git2::Oid,
) -> Vec<MoveSlot> {
    let info = &snapshot.info;
    // Three ranks per row: above it, the row itself, below it. A branch
    // tip ranks with the row after its name, behind an `--above` there.
    let mut candidates: Vec<(usize, MoveSlot)> = Vec::new();
    for (i, row) in tree.iter().enumerate() {
        let offered = match &row.kind {
            RowKind::Commit { oid, .. } if *oid == commit => {
                candidates.push((3 * i + 1, MoveSlot::Stay));
                continue;
            }
            RowKind::Commit { oid, .. } if row.target.is_some() => vec![
                (3 * i, MoveSlot::Above(*oid)),
                (3 * i + 2, MoveSlot::Below(*oid)),
            ],
            RowKind::BranchName { name, .. } => vec![(3 * i + 3, MoveSlot::Branch(name.clone()))],
            _ => continue,
        };
        candidates.extend(
            offered
                .into_iter()
                .filter(|(_, slot)| can_move(info, commit, slot)),
        );
    }
    candidates.sort_by_key(|(rank, slot)| (*rank, matches!(slot, MoveSlot::Branch(_))));

    let signature = |slot: &MoveSlot| move_signature(snapshot, expanded, commit, slot);
    let mut seen = HashSet::from([signature(&MoveSlot::Stay)]);
    candidates
        .into_iter()
        .map(|(_, slot)| slot)
        .filter(|slot| *slot == MoveSlot::Stay || seen.insert(signature(slot)))
        .collect()
}

/// The tree `commit` at `slot` draws, as row keys and connectors: two
/// slots with the same one are the same move as far as the tree shows.
fn move_signature(
    snapshot: &Snapshot,
    expanded: &HashSet<String>,
    commit: git2::Oid,
    slot: &MoveSlot,
) -> Vec<String> {
    let sections = snapshot.sections(Some(Preview::Move {
        oid: commit,
        slot: slot.clone(),
    }));
    tree::build_rows(&sections, &snapshot.ids, expanded)
        .into_iter()
        .map(|r| match r.kind {
            RowKind::Spacer(text) => text.to_string(),
            _ => r.key,
        })
        .collect()
}

#[cfg(test)]
#[path = "targets_test.rs"]
mod tests;
