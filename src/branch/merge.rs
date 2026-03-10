use anyhow::{Result, bail};
use git2::{BranchType, Repository};

use crate::git;
use crate::git_commands::git_branch;
use crate::msg;

/// Weave an existing branch into the integration branch.
///
/// If no branch is specified, shows an interactive picker with local branches
/// not currently woven. With `--all`, also shows remote branches without a
/// local counterpart.
pub fn run(branch: Option<String>, all: bool) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "merge")?;
    let info = git::gather_repo_info(&repo, false, 1)?;

    let branch_name = match branch {
        Some(name) => resolve_non_woven_branch(&repo, &info, &name)?,
        None => pick_branch(&repo, &info, all)?,
    };

    // If this is a remote branch (contains '/'), create a local tracking branch
    let local_name = if branch_name.contains('/') {
        let local = branch_name.split('/').skip(1).collect::<Vec<_>>().join("/");
        git_branch::create(workdir, &local, &branch_name)?;
        // Set up tracking
        let mut local_branch = repo.find_branch(&local, BranchType::Local)?;
        local_branch.set_upstream(Some(&branch_name))?;
        local
    } else {
        branch_name.clone()
    };

    // Merge the branch into integration (--no-ff) so it appears in the topology
    crate::git_commands::git_merge::merge_no_ff(workdir, &local_name)?;

    msg::success(&format!("Woven `{}` into integration branch", local_name));

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
        let local_names: Vec<String> = repo
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
                let short_name = name.split('/').skip(1).collect::<Vec<_>>().join("/");
                if !local_names.contains(&short_name) {
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
