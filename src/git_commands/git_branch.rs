use std::path::Path;
use std::process::Command;

use super::run_git;

/// Validate a branch name using `git check-ref-format`.
pub fn validate_name(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["check-ref-format", "--branch", name])
        .output()
        .map_err(|e| format!("Failed to run git check-ref-format: {}", e))?;

    if !output.status.success() {
        return Err(format!("'{}' is not a valid branch name", name).into());
    }
    Ok(())
}

/// Create a branch at a specific commit.
///
/// Wraps `git branch <name> <commit_hash>`.
pub fn create(
    workdir: &Path,
    name: &str,
    commit_hash: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    run_git(workdir, &["branch", name, commit_hash])
        .map_err(|e| format!("Failed to create branch: {}", e))?;

    Ok(())
}

/// Delete a local branch (force, to handle branches whose commits were dropped).
///
/// Wraps `git branch -D <name>`.
pub fn delete(workdir: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    run_git(workdir, &["branch", "-D", name])
        .map_err(|e| format!("Failed to delete branch '{}': {}", name, e))?;

    Ok(())
}

/// Create a new branch at a remote tracking ref and switch to it.
///
/// Wraps `git switch -c <name> --track <upstream>`.
pub fn switch_create_tracking(
    workdir: &Path,
    name: &str,
    upstream: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    run_git(workdir, &["switch", "-c", name, "--track", upstream])
        .map_err(|e| format!("Failed to create tracking branch: {}", e))?;

    Ok(())
}

/// Rename a branch using git branch -m.
///
/// Wraps `git branch -m <old_name> <new_name>`.
pub fn rename(
    workdir: &Path,
    old_name: &str,
    new_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    run_git(workdir, &["branch", "-m", old_name, new_name])
        .map_err(|e| format!("Failed to rename branch: {}", e))?;

    Ok(())
}
