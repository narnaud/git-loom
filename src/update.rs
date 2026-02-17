use git2::{BranchType, Repository};

use crate::git_commands;

/// Update the integration branch by pulling with rebase from upstream.
///
/// Performs a `git pull --rebase --autostash` on the current integration branch,
/// then updates submodules if any are configured. On merge conflict, the error
/// is reported so the user can resolve it manually.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let repo = Repository::discover(cwd)?;

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
    local_branch.upstream().map_err(|e| {
        format!(
            "Branch '{}' has no upstream tracking branch.\n\
             Run 'git-loom init' to set up an integration branch.\n\
             Cause: {}",
            branch_name, e
        )
    })?;

    let workdir = repo.workdir().ok_or("Cannot update in a bare repository")?;

    // Pull with rebase and autostash
    let spinner = cliclack::spinner();
    spinner.start("Pulling latest changes...");

    let result = git_commands::run_git(workdir, &["pull", "--rebase", "--autostash"]);

    match result {
        Ok(()) => {
            spinner.stop("Pulled latest changes");
        }
        Err(e) => {
            spinner.error("Pull failed");
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
