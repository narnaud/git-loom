use git2::Repository;
use std::process::Command;

use crate::git_commands::git_commit;
use crate::git_commands::git_rebase::{self, Rebase, RebaseAction, RebaseTarget};

/// Reword a commit message or rename a branch.
pub fn run(target: String, message: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let repo = Repository::discover(cwd)?;

    let resolved = crate::git::resolve_target(&repo, &target)?;

    match resolved {
        crate::git::Target::Commit(hash) => reword_commit(&repo, &hash, message),
        crate::git::Target::Branch(name) => {
            let new_name = message.ok_or("Branch renaming requires -m flag with new name")?;
            reword_branch(&repo, &name, &new_name)
        }
        crate::git::Target::File(_) => {
            Err("Cannot reword a file. Use 'git add' to stage file changes.".into())
        }
    }
}

/// Reword a commit message using git's native rebase commands.
///
/// Approach:
/// 1. git rebase --interactive --autostash --keep-empty --no-autosquash --rebase-merges [--root | <hash>^]
/// 2. git commit --allow-empty --amend --only [-m "message"]
/// 3. git rebase --continue
fn reword_commit(
    repo: &Repository,
    commit_hash: &str,
    message: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = repo.workdir().ok_or("Cannot reword in bare repository")?;

    // Check if this is a root commit (has no parent)
    let commit = repo.revparse_single(commit_hash)?.peel_to_commit()?;
    let is_root = commit.parent_count() == 0;

    let short_hash = &commit_hash[..7.min(commit_hash.len())];

    let target = if is_root {
        RebaseTarget::Root
    } else {
        RebaseTarget::Commit(commit_hash.to_string())
    };

    // Step 1: Start interactive rebase
    Rebase::new(workdir, target)
        .action(RebaseAction::Edit {
            short_hash: short_hash.to_string(),
        })
        .run()?;

    // Step 2: Amend the commit message
    if let Err(e) = git_commit::amend(workdir, message.as_deref()) {
        let _ = git_rebase::abort(workdir);
        return Err(e);
    }

    // Step 3: Continue the rebase
    git_rebase::continue_rebase(workdir)?;

    // Get the new commit hash after rebase
    let new_commit = repo.head()?.peel_to_commit()?;
    let new_hash = new_commit.id().to_string();

    // Show success message with old and new hashes
    let old_short = &commit_hash[..7.min(commit_hash.len())];
    let new_short = &new_hash[..7];
    println!(
        "Updated commit message for {} (now {})",
        old_short, new_short
    );

    Ok(())
}

/// Rename a branch using git branch -m.
fn reword_branch(
    repo: &Repository,
    old_name: &str,
    new_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = repo
        .workdir()
        .ok_or("Cannot rename branch in bare repository")?;

    let output = Command::new("git")
        .current_dir(workdir)
        .args(["branch", "-m", old_name, new_name])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to rename branch: {}", stderr).into());
    }

    println!("Renamed branch '{}' to '{}'", old_name, new_name);
    Ok(())
}

#[cfg(test)]
#[path = "reword_test.rs"]
mod tests;
