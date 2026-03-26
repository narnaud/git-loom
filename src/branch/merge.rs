use anyhow::{Context, Result, bail};
use git2::{BranchType, Repository};
use serde::{Deserialize, Serialize};

use crate::git;
use crate::git_commands::{self, MergeOutcome};
use crate::msg;
use crate::transaction::{self, LoomState, Rollback};

#[derive(Serialize, Deserialize)]
struct MergeContext {
    branch_name: String,
}

/// Weave an existing branch into the integration branch.
///
/// If no branch is specified, shows an interactive picker with local branches
/// not currently woven. With `--all`, also shows remote branches without a
/// local counterpart.
pub fn run(branch: Option<String>, all: bool) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "merge")?;
    let git_dir = repo.path().to_path_buf();
    let info = git::gather_repo_info(&repo, false, 1)?;

    let branch_name = match branch {
        Some(name) => resolve_non_woven_branch(&repo, &info, &name)?,
        None => pick_branch(&repo, &info, all)?,
    };

    // If this is a remote branch (contains '/'), create a local tracking branch
    let local_name = if branch_name.contains('/') {
        let local = git::upstream_local_branch(&branch_name);
        git_commands::branch_create(workdir, &local, &branch_name)?;
        // Set up tracking
        let mut local_branch = repo.find_branch(&local, BranchType::Local)?;
        local_branch.set_upstream(Some(&branch_name))?;
        local
    } else {
        branch_name.clone()
    };

    // Merge the branch into integration (--no-ff) so it appears in the topology
    match crate::git_commands::merge_no_ff(workdir, &git_dir, &local_name)? {
        MergeOutcome::Completed => {
            msg::success(&format!("Woven `{}` into integration branch", local_name));
        }
        MergeOutcome::Conflicted => {
            let state = LoomState {
                command: "merge".to_string(),
                rollback: Rollback::default(),
                context: serde_json::to_value(MergeContext {
                    branch_name: local_name,
                })?,
            };
            transaction::save(&git_dir, &state)?;
            transaction::warn_conflict_paused("merge");
        }
    }

    Ok(())
}

/// Resume a `loom branch merge` after conflicts have been resolved.
pub fn after_continue(context: &serde_json::Value) -> anyhow::Result<()> {
    let ctx: MergeContext =
        serde_json::from_value(context.clone()).context("Failed to parse merge resume context")?;
    msg::success(&format!(
        "Woven `{}` into integration branch",
        ctx.branch_name
    ));
    Ok(())
}

/// Resolve a branch argument, ensuring it's NOT already woven.
fn resolve_non_woven_branch(
    repo: &Repository,
    info: &git::RepoInfo,
    branch_arg: &str,
) -> Result<String> {
    // Check if it's already woven
    if info.branches.iter().any(|b| b.name == branch_arg) {
        bail!(
            "Branch '{}' is already woven into the integration branch",
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
fn pick_branch(repo: &Repository, info: &git::RepoInfo, include_remote: bool) -> Result<String> {
    let woven_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    let current_branch = &info.branch_name;

    let mut items: Vec<String> = Vec::new();

    // Local branches not woven and not the current branch
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        if let Some(name) = branch.name()?
            && name != current_branch
            && !woven_names.contains(&name)
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
                if !local_names.contains(&git::upstream_local_branch(name)) {
                    items.push(name.to_string());
                }
            }
        }
    }

    if items.is_empty() {
        bail!("No branches available to merge");
    }

    msg::select("Select branch to weave", items)
}
