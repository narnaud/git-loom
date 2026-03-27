use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::run_git;

/// Validate a branch name using `git check-ref-format`.
pub fn branch_validate_name(name: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["check-ref-format", "--branch", name])
        .output()
        .context("Failed to run git check-ref-format")?;

    if !output.status.success() {
        bail!("Branch name '{}' is not valid", name);
    }
    Ok(())
}

/// Create a branch at a specific commit.
///
/// Wraps `git branch <name> <commit_hash>`.
pub fn branch_create(workdir: &Path, name: &str, commit_hash: &str) -> Result<()> {
    run_git(workdir, &["branch", name, commit_hash]).context("Failed to create branch")?;

    Ok(())
}

/// Create or move a branch to a specific commit (force).
///
/// Wraps `git branch -f <name> <commit_hash>`.
pub fn branch_force_create(workdir: &Path, name: &str, commit_hash: &str) -> Result<()> {
    run_git(workdir, &["branch", "-f", name, commit_hash])
        .context("Failed to force-create branch")?;

    Ok(())
}

/// Delete a local branch (force, to handle branches whose commits were dropped).
///
/// Wraps `git branch -D <name>`.
pub fn branch_delete(workdir: &Path, name: &str) -> Result<()> {
    run_git(workdir, &["branch", "-D", name])
        .with_context(|| format!("Failed to delete branch '{}'", name))?;

    Ok(())
}

/// Switch to an existing local branch.
///
/// Wraps `git switch <name>`.
pub fn branch_switch(workdir: &Path, name: &str) -> Result<()> {
    run_git(workdir, &["switch", name])
        .with_context(|| format!("Failed to switch to branch '{}'", name))?;
    Ok(())
}

/// Detach HEAD at a ref without creating a local branch.
///
/// Wraps `git switch --detach <refspec>`.
pub fn branch_switch_detach(workdir: &Path, refspec: &str) -> Result<()> {
    run_git(workdir, &["switch", "--detach", refspec])
        .with_context(|| format!("Failed to detach HEAD at '{}'", refspec))?;
    Ok(())
}

/// Create a new branch at a remote tracking ref and switch to it.
///
/// Wraps `git switch -c <name> --track <upstream>`.
pub fn branch_switch_create_tracking(workdir: &Path, name: &str, upstream: &str) -> Result<()> {
    run_git(workdir, &["switch", "-c", name, "--track", upstream])
        .context("Failed to create tracking branch")?;

    Ok(())
}

/// Rename a branch using git branch -m.
///
/// Wraps `git branch -m <old_name> <new_name>`.
pub fn branch_rename(workdir: &Path, old_name: &str, new_name: &str) -> Result<()> {
    run_git(workdir, &["branch", "-m", old_name, new_name]).context("Failed to rename branch")?;

    Ok(())
}
