//! The status graph as JSON, for agent mode (spec 019).
//!
//! Built from the same [`graph::Section`] list the renderer and the TUI use,
//! so the three surfaces cannot drift: section order here is the render order
//! of spec 001. These are wire DTOs, deliberately separate from
//! [`repo::RepoInfo`] — that model holds `git2::Oid` and is free to change
//! shape, while this one is a published contract.

use std::collections::HashMap;

use serde::Serialize;

use crate::core::graph::{self, FileGroup, Section};
use crate::core::repo::{self, CommitInfo, ContextCommit, FileChange, RemoteStatus, UpstreamInfo};
use crate::core::shortid::{self, IdAllocator};

/// Bumped on any breaking change to the shape below.
const SCHEMA: u32 = 1;

/// The whole `loom status` graph.
#[derive(Serialize, Debug)]
pub struct StatusGraph {
    pub schema: u32,
    pub integration_branch: String,
    /// Directory the paths below are relative to, as `git status` reports
    /// them; empty at the repo root.
    pub cwd_prefix: String,
    pub local_changes: LocalChanges,
    /// Branch groups in render order: empty ones first, then top of stack
    /// down. See [`BranchGroup::stacked_on`].
    pub branches: Vec<BranchGroup>,
    pub loose_commits: Vec<Commit>,
    pub upstream: Upstream,
    pub context_commits: Vec<ContextEntry>,
}

#[derive(Serialize, Debug)]
pub struct LocalChanges {
    /// The `zz` short ID.
    pub id: String,
    pub files: Vec<WorkingFile>,
}

#[derive(Serialize, Debug)]
pub struct WorkingFile {
    pub id: String,
    pub path: String,
    pub index: char,
    pub worktree: char,
    pub state: FileState,
}

/// The wire spelling of [`graph::FileGroup`].
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    Conflicted,
    Tracked,
    Untracked,
}

/// One or more branches sharing a tip, plus the commits they own.
#[derive(Serialize, Debug)]
pub struct BranchGroup {
    /// Several names when branches are co-located, alphabetically last first.
    pub names: Vec<BranchName>,
    /// The group directly below this one in the stack, else null: the edge a
    /// stacked push follows, even where the branch below owns no commits and
    /// the tree draws no `││` (spec 011). Null too when that branch is hidden
    /// from this graph.
    pub stacked_on: Option<String>,
    /// The branch below is hidden by `loom.hideBranchPattern`, so `push`
    /// refuses this group (spec 011).
    pub stacked_on_hidden: bool,
    pub commits: Vec<Commit>,
}

#[derive(Serialize, Debug)]
pub struct BranchName {
    pub id: String,
    pub name: String,
    /// Null when the branch was never pushed.
    pub remote: Option<Remote>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Remote {
    Synced,
    Different,
    Gone,
}

#[derive(Serialize, Debug)]
pub struct Commit {
    /// Persistent letters when the commit has a Change-Id, else a hash prefix.
    pub id: String,
    /// Abbreviated hash, honoring `core.abbrev`.
    pub hash: String,
    pub oid: String,
    pub subject: String,
    pub change_id: Option<String>,
    /// Populated only under `-f`; ids are `<commit id>:<n>` counting from 0.
    pub files: Vec<CommitFile>,
}

#[derive(Serialize, Debug)]
pub struct CommitFile {
    pub id: String,
    pub path: String,
    pub index: char,
    pub worktree: char,
}

#[derive(Serialize, Debug)]
pub struct Upstream {
    pub label: String,
    pub base_hash: String,
    pub base_oid: String,
    pub base_subject: String,
    pub base_date: String,
    /// 0 when the upstream sits at the merge-base.
    pub commits_ahead: usize,
}

#[derive(Serialize, Debug)]
pub struct ContextEntry {
    pub hash: String,
    pub date: String,
    pub subject: String,
}

/// A branch's edge to the one below it in the stack.
#[derive(Debug, Default, Clone)]
pub struct StackEdge {
    /// Null when there is none, or when it is hidden from the graph.
    pub below: Option<String>,
    pub below_hidden: bool,
}

/// Build the JSON model from the section list the renderer draws, so the two
/// describe one graph rather than two builds of it. `stacks` is keyed by
/// branch name and must be computed before hiding, which cuts the edge to a
/// hidden branch it has to report.
///
/// Panics if `sections` has no upstream: [`graph::build_sections`] always
/// emits one.
pub fn build(
    sections: &[Section],
    integration_branch: &str,
    stacks: &HashMap<String, StackEdge>,
    ids: &IdAllocator,
    cwd_prefix: &str,
) -> StatusGraph {
    let mut local_changes = LocalChanges {
        id: ids.get_unstaged().to_string(),
        files: Vec::new(),
    };
    let mut branches = Vec::new();
    let mut loose_commits = Vec::new();
    let mut context_commits = Vec::new();
    let mut upstream = None;

    for section in sections {
        match section {
            Section::WorkingChanges(changes) => {
                local_changes.files = changes
                    .iter()
                    .map(|f| working_file(f, ids, cwd_prefix))
                    .collect();
            }
            Section::Branch { names, commits } => {
                // Co-located names share one edge, so the first answers.
                let edge = names
                    .first()
                    .and_then(|(n, _)| stacks.get(n))
                    .cloned()
                    .unwrap_or_default();
                branches.push(BranchGroup {
                    names: names.iter().map(|(n, r)| branch_name(n, r, ids)).collect(),
                    stacked_on: edge.below,
                    stacked_on_hidden: edge.below_hidden,
                    commits: commits.iter().map(|c| commit(c, ids, cwd_prefix)).collect(),
                });
            }
            Section::Loose(commits) => {
                loose_commits.extend(commits.iter().map(|c| commit(c, ids, cwd_prefix)));
            }
            Section::Context(entries) => {
                context_commits.extend(entries.iter().map(context_entry));
            }
            Section::Upstream(up) => upstream = Some(upstream_dto(up)),
        }
    }

    StatusGraph {
        schema: SCHEMA,
        integration_branch: integration_branch.to_string(),
        cwd_prefix: cwd_prefix.to_string(),
        local_changes,
        branches,
        loose_commits,
        upstream: upstream.expect("build_sections always emits the upstream"),
        context_commits,
    }
}

fn working_file(f: &FileChange, ids: &IdAllocator, cwd_prefix: &str) -> WorkingFile {
    WorkingFile {
        id: ids.get_file(&f.path).to_string(),
        path: repo::cwd_relative_path(&f.path, cwd_prefix),
        index: f.index,
        worktree: f.worktree,
        state: file_state(f),
    }
}

/// The wire name for the group the renderer puts this file in (spec 001).
fn file_state(f: &FileChange) -> FileState {
    match graph::file_group(f.index, f.worktree) {
        FileGroup::Conflicted => FileState::Conflicted,
        FileGroup::Tracked => FileState::Tracked,
        FileGroup::Untracked => FileState::Untracked,
    }
}

fn branch_name(name: &str, remote: &Option<RemoteStatus>, ids: &IdAllocator) -> BranchName {
    BranchName {
        id: ids.get_branch(name).to_string(),
        name: name.to_string(),
        remote: remote.as_ref().map(|r| match r {
            RemoteStatus::Synced => Remote::Synced,
            RemoteStatus::Different => Remote::Different,
            RemoteStatus::Gone => Remote::Gone,
        }),
    }
}

fn commit(c: &CommitInfo, ids: &IdAllocator, cwd_prefix: &str) -> Commit {
    let sid = ids.get_commit(c.oid);
    Commit {
        id: sid.to_string(),
        hash: c.short_id.clone(),
        oid: c.oid.to_string(),
        subject: c.message.clone(),
        change_id: c.change_id.clone(),
        files: c
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| CommitFile {
                id: shortid::commit_file_id(sid, i),
                path: repo::cwd_relative_path(&f.path, cwd_prefix),
                index: f.index,
                worktree: f.worktree,
            })
            .collect(),
    }
}

fn upstream_dto(up: &UpstreamInfo) -> Upstream {
    Upstream {
        label: up.label.clone(),
        base_hash: up.base_short_id.clone(),
        base_oid: up.merge_base_oid.to_string(),
        base_subject: up.base_message.clone(),
        base_date: up.base_date.clone(),
        commits_ahead: up.commits_ahead,
    }
}

fn context_entry(c: &ContextCommit) -> ContextEntry {
    ContextEntry {
        hash: c.short_hash.clone(),
        date: c.date.clone(),
        subject: c.message.clone(),
    }
}

#[cfg(test)]
#[path = "status_json_test.rs"]
mod tests;
