use anyhow::{Context, Result, bail};
use git2::BranchType;

use crate::git;
use crate::git_commands::{self, git_rebase};
use crate::msg;

/// Update the integration branch by fetching and rebasing from upstream.
///
/// Performs `git fetch --tags --force --prune` followed by
/// `git rebase --autostash <upstream>` on the current integration branch,
/// then updates submodules if any are configured. On merge conflict, the error
/// is reported so the user can resolve it manually.
pub fn run() -> Result<()> {
    let repo = git::open_repo()?;

    // Validate that we're on a branch with an upstream tracking ref
    let head = repo.head().context("Failed to get HEAD reference")?;
    if !head.is_branch() {
        bail!("HEAD is detached\nSwitch to an integration branch");
    }

    let branch_name = head
        .shorthand()
        .context("Could not determine current branch name")?
        .to_string();

    let local_branch = repo.find_branch(&branch_name, BranchType::Local)?;
    let upstream = local_branch.upstream().with_context(|| {
        format!(
            "Branch `{}` has no upstream tracking branch\n\
             Run `git-loom init` to set up an integration branch",
            branch_name
        )
    })?;
    let upstream_name = upstream
        .name()?
        .context("Upstream branch name is not valid UTF-8")?
        .to_string();

    let workdir = git::require_workdir(&repo, "update")?;

    // Fetch with tags, force-update, and prune deleted remote branches
    let spinner = msg::spinner();
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
    let spinner = msg::spinner();
    spinner.start("Rebasing onto upstream...");

    let result = git_commands::run_git(workdir, &["rebase", "--autostash", &upstream_name]);

    match result {
        Ok(()) => {
            spinner.stop("Rebased onto upstream");
        }
        Err(e) => {
            let _ = git_rebase::abort(workdir);
            spinner.error("Rebase failed");
            return Err(e);
        }
    }

    // Update submodules if .gitmodules exists
    if workdir.join(".gitmodules").exists() {
        let spinner = msg::spinner();
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

    // Show the latest upstream commit
    let upstream_info = repo
        .revparse_single(&upstream_name)
        .ok()
        .and_then(|obj| obj.peel_to_commit().ok())
        .map(|commit| {
            let short_id = &commit.id().to_string()[..7];
            let summary = commit.summary().unwrap_or("");
            format!(" ({} {})", short_id, summary)
        })
        .unwrap_or_default();

    msg::success(&format!(
        "Updated branch `{}` with `{}`{}",
        branch_name, upstream_name, upstream_info
    ));

    Ok(())
}

#[cfg(test)]
#[path = "update_test.rs"]
mod tests;
