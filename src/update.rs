use anyhow::{Context, Result, bail};
use git2::BranchType;

use crate::git;
use crate::git_commands::{self, git_branch, git_rebase};
use crate::msg;

/// Update the integration branch by fetching and rebasing from upstream.
///
/// Performs `git fetch --tags --force --prune` followed by
/// `git rebase --autostash <upstream>` on the current integration branch,
/// then updates submodules if any are configured. On merge conflict, the error
/// is reported so the user can resolve it manually.
pub fn run(skip_confirm: bool) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "update")?;

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

    let result = git_commands::run_git(
        workdir,
        &[
            "rebase",
            "--autostash",
            "--rebase-merges",
            "--update-refs",
            &upstream_name,
        ],
    );

    match result {
        Ok(()) => {
            spinner.stop("Rebased onto upstream");
        }
        Err(_) => {
            let _ = git_rebase::abort(workdir);
            spinner.error("Rebase failed");
            bail!(
                "Rebase onto `{}` had conflicts — aborted\n\
                 Run `git rebase --rebase-merges --update-refs --autostash {}` to resolve manually",
                upstream_name,
                upstream_name
            );
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

    // Propose removing local branches whose remote tracking branch was pruned
    let gone = find_branches_with_gone_upstream(&repo, &branch_name)?;
    if !gone.is_empty() {
        msg::warn(&format!(
            "{} local {} with a gone upstream:",
            gone.len(),
            if gone.len() == 1 {
                "branch"
            } else {
                "branches"
            }
        ));
        for name in &gone {
            println!("  · {}", name);
        }
        let confirmed = skip_confirm
            || msg::confirm(if gone.len() == 1 {
                "Remove it?"
            } else {
                "Remove them?"
            })?;
        if confirmed {
            for name in &gone {
                git_branch::delete(workdir, name)?;
                msg::success(&format!("Removed branch `{}`", name));
            }
        }
    }

    Ok(())
}

/// Find local branches whose configured upstream tracking ref no longer exists.
///
/// After `git fetch --prune`, remote-tracking refs for deleted remote branches
/// are removed. Any local branch that had an upstream configured pointing to
/// one of those refs is considered "gone".
fn find_branches_with_gone_upstream(
    repo: &git2::Repository,
    current_branch: &str,
) -> Result<Vec<String>> {
    let config = repo.config()?;
    let mut gone = Vec::new();

    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        let Some(name) = branch.name()? else {
            continue;
        };
        let name = name.to_string();
        if name == current_branch {
            continue;
        }

        // Check if an upstream remote is configured for this branch
        let remote_key = format!("branch.{}.remote", name);
        let Ok(remote) = config.get_string(&remote_key) else {
            continue;
        };

        // Check if the merge ref (upstream branch name) is configured
        let merge_key = format!("branch.{}.merge", name);
        let Ok(merge) = config.get_string(&merge_key) else {
            continue;
        };

        // Construct the remote-tracking ref and check if it still exists
        let branch_part = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
        let tracking_ref = format!("refs/remotes/{}/{}", remote, branch_part);
        if repo.find_reference(&tracking_ref).is_err() {
            gone.push(name);
        }
    }

    Ok(gone)
}

#[cfg(test)]
#[path = "update_test.rs"]
mod tests;
