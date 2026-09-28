use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::core::{msg, repo};
use crate::git;

/// Create the worktree `<dir>-<name>` beside the main worktree `<dir>`, on a
/// new `integration-<name>` tracking the main worktree branch's upstream.
pub fn run(name: String) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "create a worktree")?;

    let name = name.trim();
    if name.is_empty() {
        bail!("Worktree name cannot be empty");
    }
    // The name is a directory name as well as part of a branch name.
    if name.contains(['/', '\\']) {
        bail!("Worktree name `{name}` cannot contain a path separator");
    }
    let branch = format!("integration-{name}");
    git::branch_validate_name(workdir, &branch)?;
    repo::ensure_branch_not_exists(&repo, &branch)?;

    let worktrees = git::list_worktrees(workdir)?;
    let main = worktrees
        .first()
        .context("Git lists no worktree for this repository")?;
    // Git lists the main worktree at its git dir minus `/.git`, which is not
    // the checkout for a submodule, a separate git dir or a bare repository.
    let toplevel = git::run_git_stdout(&main.path, &["rev-parse", "--show-toplevel"]).ok();
    if toplevel.map(|t| super::canonical(Path::new(t.trim()))) != Some(super::canonical(&main.path))
    {
        bail!(
            "Cannot place a worktree beside `{}`: it is not the main checkout",
            main.path.display()
        );
    }
    let parent = main
        .path
        .parent()
        .context("The main worktree has no parent directory")?;
    // Joined with `/` rather than `Path::join`: git lists paths with forward
    // slashes on Windows too, and a `\` in the middle reads as a typo.
    let path = PathBuf::from(format!(
        "{}/{}-{}",
        parent.display(),
        super::dir_name(&main.path),
        name
    ));
    if path.exists() {
        bail!("`{}` already exists", path.display());
    }

    // The main worktree's, not the current branch's: that may be a pushed
    // feature branch `loom switch` checked out.
    let upstream = match main
        .branch
        .as_deref()
        .and_then(|b| crate::init::branch_upstream(&repo, b))
    {
        Some(upstream) => upstream,
        None => crate::init::detect_upstream(&repo, main.branch.as_deref())?,
    };

    let path_arg = path.display().to_string();
    let added = git::run_git(
        workdir,
        &[
            "worktree", "add", "--track", "-b", &branch, &path_arg, &upstream,
        ],
    );
    if let Err(e) = added {
        // `worktree add -b` creates the branch before it checks the path, and
        // a leftover would refuse the retry as taken. The branch did not exist
        // above, so this call made it.
        let _ = git::run_git(workdir, &["branch", "-D", &branch]);
        return Err(e).context("Failed to create the worktree");
    }

    msg::success(&format!(
        "Created worktree `{}` on `{}` tracking `{}`",
        path.display(),
        branch,
        upstream
    ));
    Ok(())
}

#[cfg(test)]
#[path = "new_test.rs"]
mod tests;
