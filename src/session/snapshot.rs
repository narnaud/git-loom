//! Everything a session gathers from the repo for one round, and the previews
//! faked into it: a branch being named, a commit being placed or moved.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::core::graph::{self, Section};
use crate::core::hidden;
use crate::core::repo::{self, BranchInfo, CommitInfo, FileChange, RepoInfo};
use crate::core::shortid::IdAllocator;
use crate::session::tree::PENDING_COMMIT_OID;

/// Everything gathered from the repo for one TUI round.
pub(crate) struct Snapshot {
    pub(crate) workdir: PathBuf,
    pub(crate) git_dir: PathBuf,
    pub(crate) cwd_prefix: String,
    /// Kept as gathered (hidden branches removed) so the graph can be rebuilt
    /// with a branch that does not exist yet.
    pub(crate) info: RepoInfo,
    pub(crate) ids: IdAllocator,
}

/// Placeholder name of a branch being created. Git rejects a ref component
/// starting with a dot, so it clashes with no real branch; keeping it
/// non-empty keeps it distinct from "no branch here" sentinels in `graph`.
pub(crate) const NEW_BRANCH_NAME: &str = ".new";

/// Where `c` puts its commit: loose on the integration line (`-l`) or at the
/// tip of a woven branch (`-b`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommitDest {
    Integration,
    Branch(String),
}

/// A row that exists only on screen, or one drawn where it is not: a branch
/// being named, a commit being placed, or a commit being moved.
pub(crate) enum Preview {
    Branch(BranchInfo),
    Commit { dest: CommitDest, file_count: usize },
    Move { oid: git2::Oid, slot: MoveSlot },
}

/// Where `m` puts its commit, as `loom fold` spells it (Spec 007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MoveSlot {
    /// Where it already is: the walk starts here, and nothing runs.
    Stay,
    /// `--above <commit>`.
    Above(git2::Oid),
    /// `--below <commit>`.
    Below(git2::Oid),
    /// `fold <commit> <branch>`: the branch's tip, advancing only that name.
    /// Never shown as a branch pick: it is the one way to reach an empty
    /// branch, or to split a co-located group.
    Branch(String),
}

impl Snapshot {
    /// The graph sections with `preview` faked in — a branch at its tip, or a
    /// commit at its destination — so the tree shows where it will land:
    /// split ownership, co-located names, stacking and all.
    ///
    /// A commit being placed or moved keeps every branch where the real tree
    /// draws it, so one it fills or empties does not jump between the empty
    /// and the owning groups under `↑`/`↓`; the tree regroups once it runs.
    pub(crate) fn sections(&self, preview: Option<Preview>) -> Vec<Section> {
        let mut info = self.info.clone();
        let pinned = match preview {
            Some(Preview::Branch(pending)) => {
                info.branches.push(pending);
                false
            }
            Some(Preview::Commit { dest, file_count }) => {
                place_pending_commit(&mut info, &dest, file_count);
                true
            }
            Some(Preview::Move { oid, slot }) => {
                place_moved_commit(&mut info, oid, &slot);
                true
            }
            None => false,
        };
        let sections = graph::build_sections(info);
        if pinned {
            keep_branch_places(&graph::build_sections(self.info.clone()), sections)
        } else {
            sections
        }
    }

    /// Whether `files` (short IDs) is the `zz` that stands for every change.
    pub(crate) fn is_all_changes(&self, files: &[String]) -> bool {
        files.iter().any(|f| f == self.ids.get_unstaged())
    }

    /// How many files the index holds a change for — what a `C` commit takes.
    pub(crate) fn staged_count(&self) -> usize {
        self.info
            .working_changes
            .iter()
            .filter(|c| is_staged(c))
            .count()
    }
}

impl Snapshot {
    /// Stands in until a tree can be gathered, for a session that opened on a
    /// paused operation; never drawn, since the paused view replaces the tree.
    pub(crate) fn placeholder(workdir: PathBuf, git_dir: PathBuf) -> Snapshot {
        let info = RepoInfo {
            branch_name: String::new(),
            upstream: repo::UpstreamInfo {
                label: String::new(),
                remote: String::new(),
                branch: String::new(),
                tip_oid: git2::Oid::ZERO_SHA1,
                merge_base_oid: git2::Oid::ZERO_SHA1,
                base_short_id: String::new(),
                base_message: String::new(),
                base_date: String::new(),
                commits_ahead: 0,
            },
            commits: Vec::new(),
            branches: Vec::new(),
            working_changes: Vec::new(),
            context_commits: Vec::new(),
        };
        Snapshot {
            workdir,
            git_dir,
            cwd_prefix: String::new(),
            ids: IdAllocator::new(info.collect_entities()),
            info,
        }
    }
}

/// Gather repo info, exactly like `loom status` with files enabled.
pub(crate) fn load_snapshot(context: usize) -> Result<Snapshot> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "display status")?.to_path_buf();
    let git_dir = repo.path().to_path_buf();
    let cwd_prefix = repo::cwd_relative_to_repo(&repo).unwrap_or_default();

    let (mut info, ids) = repo::gather_status(&repo, true, context)?;
    hidden::apply_hidden_branches(&repo, &mut info);

    Ok(Snapshot {
        workdir,
        git_dir,
        cwd_prefix,
        info,
        ids,
    })
}

/// Whether the index holds something of `change` for a `C` commit to take.
/// The one spelling of it: the placeholder's file count and the rows marked
/// as covered must not be able to disagree.
pub(crate) fn is_staged(change: &FileChange) -> bool {
    matches!(change.index, 'A' | 'M' | 'D' | 'R')
}

/// Reorder the branch sections of `preview` into the order `base` draws them
/// in. Each is ranked by the first of its names `base` places, one it lacks
/// by the section before it; the sort is stable, so a group or stack a
/// preview splits keeps its own order. An empty group is split by place.
pub(crate) fn keep_branch_places(base: &[Section], mut preview: Vec<Section>) -> Vec<Section> {
    let place: HashMap<&str, usize> = base
        .iter()
        .enumerate()
        .flat_map(|(i, s)| match s {
            Section::Branch { names, .. } => names.iter().map(|(n, _)| (n.as_str(), i)).collect(),
            _ => Vec::new(),
        })
        .collect();
    let is_branch = |s: &Section| matches!(s, Section::Branch { .. });
    let Some(start) = preview.iter().position(is_branch) else {
        return preview;
    };
    let end = start + preview[start..].iter().take_while(|s| is_branch(s)).count();
    let taken: Vec<Section> = preview.drain(start..end).collect();
    let rank_of = |name: &str| place.get(name).copied();
    let mut rank = 0;
    let mut run: Vec<(usize, Section)> = Vec::new();
    for section in taken {
        let Section::Branch { names, commits } = section else {
            unreachable!("a run of branch sections");
        };
        if !commits.is_empty() {
            rank = names
                .iter()
                .filter_map(|(n, _)| rank_of(n))
                .min()
                .unwrap_or(rank);
            run.push((rank, Section::Branch { names, commits }));
            continue;
        }
        // A branch the preview empties is co-located with those at the base,
        // but keeps its own place: nothing is drawn under the group anyway.
        for (name, remote) in names {
            rank = rank_of(&name).unwrap_or(rank);
            match run.last_mut() {
                Some((r, Section::Branch { names, commits }))
                    if *r == rank && commits.is_empty() =>
                {
                    names.push((name, remote))
                }
                _ => run.push((
                    rank,
                    Section::Branch {
                        names: vec![(name, remote)],
                        commits: Vec::new(),
                    },
                )),
            }
        }
    }
    run.sort_by_key(|(rank, _)| *rank);
    preview.splice(start..start, run.into_iter().map(|(_, s)| s));
    preview
}

/// Insert the pending commit into `info` where `loom commit` would put it:
/// newest on the integration line, or at the branch's tip with the branch
/// advanced onto it and whatever was stacked on that tip re-parented, so the
/// ownership walk in `graph` draws the stack as the relocation will leave it.
pub(crate) fn place_pending_commit(info: &mut RepoInfo, dest: &CommitDest, file_count: usize) {
    let oid = PENDING_COMMIT_OID;
    let mut pending = CommitInfo {
        oid,
        short_id: String::new(),
        message: format!(
            "new commit ({} file{})",
            file_count,
            if file_count == 1 { "" } else { "s" }
        ),
        parent_oid: None,
        change_id: None,
        files: Vec::new(),
    };
    match dest {
        CommitDest::Integration => {
            pending.parent_oid = Some(
                info.commits
                    .first()
                    .map_or(info.upstream.merge_base_oid, |c| c.oid),
            );
            info.commits.insert(0, pending);
        }
        // Every destination was read off a drawn branch row of this same
        // snapshot, so the branch is there.
        CommitDest::Branch(name) => push_onto_branch(info, pending, name),
    }
}

/// Put `commit` on top of branch `name`, advancing that name alone, with
/// whatever was stacked on the old tip re-parented onto it. A no-op when the
/// branch is not there.
pub(crate) fn push_onto_branch(info: &mut RepoInfo, mut commit: CommitInfo, name: &str) {
    let Some(branch) = info.branches.iter_mut().find(|b| b.name == name) else {
        return;
    };
    let oid = commit.oid;
    let tip = branch.tip_oid;
    branch.tip_oid = oid;
    commit.parent_oid = Some(tip);
    // A tip outside the range is the base: the branches forking from it stay
    // parallel, only a stack on an in-range tip follows.
    let at = info.commits.iter().position(|c| c.oid == tip);
    if at.is_some() {
        reparent_children(info, tip, oid);
    }
    info.commits
        .insert(at.unwrap_or(info.commits.len()), commit);
}

pub(crate) fn reparent_children(info: &mut RepoInfo, from: git2::Oid, to: git2::Oid) {
    for commit in &mut info.commits {
        if commit.parent_oid == Some(from) {
            commit.parent_oid = Some(to);
        }
    }
}

/// Move commit `oid` of `info` to `slot`, as `loom fold` would (Spec 007):
/// taken out, its children and the branches ending at it fall back onto its
/// parent; `--above` hands it the target's branches, `--below` leaves them.
pub(crate) fn place_moved_commit(info: &mut RepoInfo, oid: git2::Oid, slot: &MoveSlot) {
    let anchor_known = |x: &git2::Oid| *x != oid && info.commits.iter().any(|c| c.oid == *x);
    let known = match slot {
        MoveSlot::Stay => false,
        MoveSlot::Above(x) | MoveSlot::Below(x) => anchor_known(x),
        MoveSlot::Branch(name) => info.branches.iter().any(|b| b.name == *name),
    };
    let Some(from) = info
        .commits
        .iter()
        .position(|c| c.oid == oid)
        .filter(|_| known)
    else {
        return;
    };
    let mut moved = info.commits.remove(from);
    if let Some(parent) = moved.parent_oid {
        reparent_children(info, oid, parent);
        for branch in &mut info.branches {
            if branch.tip_oid == oid {
                branch.tip_oid = parent;
            }
        }
    }
    let position = |info: &RepoInfo, x: git2::Oid| {
        info.commits
            .iter()
            .position(|c| c.oid == x)
            .expect("checked above")
    };
    match slot {
        MoveSlot::Stay => {}
        MoveSlot::Above(x) => {
            let at = position(info, *x);
            reparent_children(info, *x, oid);
            for branch in &mut info.branches {
                if branch.tip_oid == *x {
                    branch.tip_oid = oid;
                }
            }
            moved.parent_oid = Some(*x);
            info.commits.insert(at, moved);
        }
        MoveSlot::Below(x) => {
            let at = position(info, *x);
            moved.parent_oid = info.commits[at].parent_oid;
            info.commits[at].parent_oid = Some(oid);
            info.commits.insert(at + 1, moved);
        }
        MoveSlot::Branch(name) => push_onto_branch(info, moved, name),
    }
}
