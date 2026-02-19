use git2::BranchType;

use crate::git;
use crate::git_commands;

/// Update the integration branch by fetching and rebasing from upstream.
///
/// Performs `git fetch --tags --force --prune` followed by
/// `git rebase --autostash <upstream>` on the current integration branch,
/// then updates submodules if any are configured. On merge conflict, the error
/// is reported so the user can resolve it manually.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let repo = git::open_repo()?;

    // Validate that we're on a branch with an upstream tracking ref
    let head = repo.head()?;
    if !head.is_branch() {
        return Err("HEAD is detached. Please switch to an integration branch.".into());
    }

    let branch_name = head
        .shorthand()
        .ok_or("Could not determine current branch name")?
        .to_string();

    let local_branch = repo.find_branch(&branch_name, BranchType::Local)?;
    let upstream = local_branch.upstream().map_err(|e| {
        format!(
            "Branch '{}' has no upstream tracking branch.\n\
             Run 'git-loom init' to set up an integration branch.\n\
             Cause: {}",
            branch_name, e
        )
    })?;
    let upstream_name = upstream
        .name()?
        .ok_or("Upstream branch name is not valid UTF-8")?
        .to_string();

    let workdir = git::require_workdir(&repo, "update")?;

    // Fetch with tags, force-update, and prune deleted remote branches
    let spinner = cliclack::spinner();
    spinner.start("Fetching latest changes...");

    let result = git_commands::run_git(workdir, &["fetch", "--tags", "--force", "--prune"]);

    match result {
        Ok(()) => {
            spinner.stop("Fetched latest changes");
        }
        Err(e) => {
            spinner.error("Fetch failed");
            return Err(e);
        }
    }

    // Rebase onto upstream with autostash
    let spinner = cliclack::spinner();
    spinner.start("Rebasing onto upstream...");

    let result = git_commands::run_git(workdir, &["rebase", "--autostash", &upstream_name]);

    match result {
        Ok(()) => {
            spinner.stop("Rebased onto upstream");
        }
        Err(e) => {
            spinner.error("Rebase failed");
            return Err(e);
        }
    }

    // Update submodules if .gitmodules exists
    if workdir.join(".gitmodules").exists() {
        let spinner = cliclack::spinner();
        spinner.start("Updating submodules...");

        let result =
            git_commands::run_git(workdir, &["submodule", "update", "--init", "--recursive"]);

        match result {
            Ok(()) => {
                spinner.stop("Updated submodules");
            }
            Err(e) => {
                spinner.error("Submodule update failed");
                return Err(e);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "update_test.rs"]
mod tests;
