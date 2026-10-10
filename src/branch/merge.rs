use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use git2::{BranchType, Oid, Repository};
use serde::{Deserialize, Serialize};

use crate::core::msg;
use crate::core::repo;
use crate::core::transaction::{self, LoomState, Rollback};
use crate::git::{self, MergeOutcome};

#[derive(Serialize, Deserialize)]
struct MergeContext {
    branch_name: String,
}

/// Merge an existing branch into the integration branch.
///
/// If no branch is specified, shows an interactive picker with local branches
/// not currently woven. With `--all`, also shows remote branches without a
/// local counterpart.
pub fn run(branch: Option<String>, all: bool) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "merge")?;
    let git_dir = repo.path().to_path_buf();
    let info = repo::gather_repo_info(&repo, false, 1)?;
    let others = other_integrations(&repo, workdir, &info.branch_name)?;

    let branch_name = match branch {
        Some(name) => resolve_non_woven_branch(&repo, &info, &others, &name)?,
        None => pick_branch(&repo, &info, &others, all)?,
    };

    // Remote branches need a local tracking branch before they can be merged.
    let local_name = if repo.find_branch(&branch_name, BranchType::Local).is_ok() {
        branch_name.clone()
    } else {
        let local = repo::upstream_local_branch(&branch_name);
        git::branch_create(workdir, &local, &branch_name)?;
        // Set up tracking
        let mut local_branch = repo.find_branch(&local, BranchType::Local)?;
        local_branch.set_upstream(Some(&branch_name))?;
        local
    };

    // Merge the branch into integration (--no-ff) so it appears in the topology
    match crate::git::merge_no_ff(workdir, &git_dir, &local_name)? {
        MergeOutcome::Completed => {
            msg::success(&format!("Merged `{}` into integration branch", local_name));
        }
        MergeOutcome::Stopped => {
            let state = LoomState {
                command: "merge".to_string(),
                rollback: Rollback::default(),
                context: serde_json::to_value(MergeContext {
                    branch_name: local_name,
                })?,
                protect: Vec::new(),
                targets: Vec::new(),
            };
            transaction::save(&git_dir, &state)?;
            transaction::warn_paused(workdir, "merge");
        }
    }

    Ok(())
}

/// Resume a `loom merge` after conflicts have been resolved.
pub fn after_continue(context: &serde_json::Value) -> anyhow::Result<()> {
    let ctx: MergeContext =
        serde_json::from_value(context.clone()).context("Failed to parse merge resume context")?;
    msg::success(&format!(
        "Merged `{}` into integration branch",
        ctx.branch_name
    ));
    Ok(())
}

/// The integration branch of another worktree: its checked-out branch, when
/// that branch has an upstream.
struct OtherIntegration {
    branch: String,
    path: PathBuf,
    tip: Oid,
    upstream_tip: Oid,
}

/// Every other worktree's integration branch. `current` is skipped by name, as
/// a branch is checked out in one worktree at most.
fn other_integrations(
    repo: &Repository,
    workdir: &Path,
    current: &str,
) -> Result<Vec<OtherIntegration>> {
    let mut result = Vec::new();
    for checkout in git::git_worktree::worktree_checkouts(workdir)? {
        if checkout.branch == current {
            continue;
        }
        let Ok(local) = repo.find_branch(&checkout.branch, BranchType::Local) else {
            continue;
        };
        let (Some(tip), Some(upstream_tip)) = (
            local.get().target(),
            local.upstream().ok().and_then(|u| u.get().target()),
        ) else {
            continue;
        };
        result.push(OtherIntegration {
            branch: checkout.branch,
            path: checkout.path,
            tip,
            upstream_tip,
        });
    }
    Ok(result)
}

/// The other integration branch `branch` is woven into: its tip is in that
/// branch's history but not yet in its upstream (Spec 005).
fn woven_elsewhere<'a>(
    repo: &Repository,
    others: &'a [OtherIntegration],
    branch: &str,
) -> Result<Option<&'a OtherIntegration>> {
    let Some(tip) = repo
        .find_branch(branch, BranchType::Local)
        .ok()
        .and_then(|local| local.get().target())
    else {
        return Ok(None);
    };
    for other in others {
        if other.branch != branch
            && repo::contains(repo, other.tip, tip)?
            && !repo::contains(repo, other.upstream_tip, tip)?
        {
            return Ok(Some(other));
        }
    }
    Ok(None)
}

/// Resolve a branch argument, ensuring it's NOT already woven, here or in
/// another worktree's integration branch.
fn resolve_non_woven_branch(
    repo: &Repository,
    info: &repo::RepoInfo,
    others: &[OtherIntegration],
    branch_arg: &str,
) -> Result<String> {
    // Check if it's already woven
    if info.branches.iter().any(|b| b.name == branch_arg) {
        bail!(
            "Branch '{}' is already woven into the integration branch",
            branch_arg
        );
    }

    // Weaving it would weave everything woven there as well.
    if let Some(other) = others.iter().find(|o| o.branch == branch_arg) {
        bail!(
            "Branch `{}` is checked out in the worktree at `{}`",
            branch_arg,
            other.path.display()
        );
    }

    if let Some(other) = woven_elsewhere(repo, others, branch_arg)? {
        bail!(
            "Branch `{}` is already woven into `{}` at `{}`\n\
             Unmerge it there first: `loom unmerge {}`",
            branch_arg,
            other.branch,
            other.path.display(),
            branch_arg
        );
    }

    // Check if it's a local branch
    if repo.find_branch(branch_arg, BranchType::Local).is_ok() {
        return Ok(branch_arg.to_string());
    }

    // Check if it's a remote branch
    if repo.find_branch(branch_arg, BranchType::Remote).is_ok() {
        return Ok(branch_arg.to_string());
    }

    bail!("Branch '{}' not found", branch_arg)
}

/// Interactive picker: list non-woven local branches, optionally with remotes.
fn pick_branch(
    repo: &Repository,
    info: &repo::RepoInfo,
    others: &[OtherIntegration],
    include_remote: bool,
) -> Result<String> {
    let woven_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    let current_branch = &info.branch_name;

    let mut items: Vec<String> = Vec::new();

    // Local branches not woven and not the current branch
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        if let Some(name) = branch.name()?
            && name != current_branch
            && !woven_names.contains(&name)
            && !others.iter().any(|o| o.branch == name)
            && woven_elsewhere(repo, others, name)?.is_none()
        {
            items.push(name.to_string());
        }
    }

    // Remote branches without a local counterpart
    if include_remote {
        let local_names: std::collections::HashSet<String> = repo
            .branches(Some(BranchType::Local))?
            .filter_map(|b| b.ok())
            .filter_map(|(b, _)| b.name().ok().flatten().map(|n| n.to_string()))
            .collect();

        let upstream_label = &info.upstream.label;

        for branch_result in repo.branches(Some(BranchType::Remote))? {
            let (branch, _) = branch_result?;
            if let Some(name) = branch.name()? {
                // Skip the upstream branch (e.g. origin/main)
                if name == upstream_label {
                    continue;
                }
                // Skip HEAD pointer
                if name.ends_with("/HEAD") {
                    continue;
                }
                // Skip if a local branch with the same short name exists
                if !local_names.contains(&repo::upstream_local_branch(name)) {
                    items.push(name.to_string());
                }
            }
        }
    }

    if items.is_empty() {
        bail!("No branches available to merge");
    }

    msg::select(
        "Select branch to merge",
        items,
        "re-run with: loom merge <branch>",
    )
}
