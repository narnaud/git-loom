use anyhow::{Result, bail};
use git2::{BranchType, Repository, StatusOptions};

use crate::git;
use crate::git_commands::git_branch;
use crate::msg;

/// Switch to any branch (local or remote) for testing without weaving it into
/// the integration branch. Remote-only branches detach HEAD at the remote ref.
/// Fails if the working tree has staged or unstaged changes to tracked files.
pub fn run(branch: Option<String>) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "switch")?;

    check_clean(&repo)?;

    let (branch_name, is_remote) = match branch {
        Some(arg) => resolve_branch(&repo, &arg)?,
        None => pick_branch(&repo)?,
    };

    if is_remote {
        git_branch::switch_detach(workdir, &branch_name)?;
        msg::success(&format!("Detached HEAD at `{}`", branch_name));
    } else {
        git_branch::switch(workdir, &branch_name)?;
        msg::success(&format!("Switched to `{}`", branch_name));
    }

    Ok(())
}

/// Fail if the working tree has staged or unstaged changes to tracked files.
fn check_clean(repo: &Repository) -> Result<()> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    if !statuses.is_empty() {
        bail!(
            "Working tree has uncommitted changes.\n\
             Stash or commit your changes before switching branches."
        );
    }
    Ok(())
}

/// Resolve a branch argument to a name and whether it is remote-only.
///
/// Tries in order: local branch name → remote branch name → short ID
/// (best-effort, only works when on an integration branch).
fn resolve_branch(repo: &Repository, arg: &str) -> Result<(String, bool)> {
    if repo.find_branch(arg, BranchType::Local).is_ok() {
        return Ok((arg.to_string(), false));
    }

    if repo.find_branch(arg, BranchType::Remote).is_ok() {
        return Ok((arg.to_string(), true));
    }

    // Short ID resolution via gather_repo_info (best-effort)
    if let Ok(git::Target::Branch(name)) = git::resolve_arg(repo, arg, &[git::TargetKind::Branch]) {
        return Ok((name, false));
    }

    bail!("Branch '{}' not found", arg)
}

/// Interactive picker: all local branches (except current) plus remote-only
/// branches (those without a local counterpart).
///
/// Returns `(branch_name, is_remote_only)`.
fn pick_branch(repo: &Repository) -> Result<(String, bool)> {
    let current = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()));

    let mut items: Vec<(String, bool)> = Vec::new();
    let mut local_names: Vec<String> = Vec::new();

    // Local branches, skip the current branch; collect all names for deduplication
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        if let Some(name) = branch.name()? {
            local_names.push(name.to_string());
            if Some(name) != current.as_deref() {
                items.push((name.to_string(), false));
            }
        }
    }

    // Remote-only branches (no local counterpart, skip /HEAD pointers)
    for branch_result in repo.branches(Some(BranchType::Remote))? {
        let (branch, _) = branch_result?;
        if let Some(name) = branch.name()? {
            if name.ends_with("/HEAD") {
                continue;
            }
            let short_name = name.split('/').skip(1).collect::<Vec<_>>().join("/");
            if !local_names.contains(&short_name) {
                items.push((name.to_string(), true));
            }
        }
    }

    if items.is_empty() {
        bail!("No branches available to switch to");
    }

    let display: Vec<String> = items.iter().map(|(n, _)| n.clone()).collect();
    let selected = msg::select("Select branch to switch to", display)?;
    let is_remote = items
        .iter()
        .find(|(n, _)| n == &selected)
        .is_some_and(|(_, r)| *r);
    Ok((selected, is_remote))
}
