use git2::Repository;

use crate::git::{self, Target};
use crate::git_commands::git_rebase::{self, Rebase, RebaseAction};
use crate::git_commands::{self, git_branch, git_commit};

/// Reword a commit message or rename a branch.
pub fn run(target: String, message: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let repo = git::open_repo()?;

    let resolved = git::resolve_target(&repo, &target)?;

    match resolved {
        Target::Commit(hash) => reword_commit(&repo, &hash, message),
        Target::Branch(name) => {
            let new_name = match message {
                Some(msg) => msg,
                None => {
                    // Prompt for new branch name with current name as placeholder
                    cliclack::input("New branch name")
                        .placeholder(&name)
                        .interact()?
                }
            };
            let new_name = new_name.trim().to_string();
            git_branch::validate_name(&new_name)?;
            reword_branch(&repo, &name, &new_name)
        }
        Target::File(_) => Err("Cannot reword a file. Use 'git add' to stage file changes.".into()),
    }
}

/// Reword a commit message using git's native rebase commands.
///
/// Approach:
/// 1. git rebase --interactive --autostash --keep-empty --no-autosquash --rebase-merges [--root | <hash>^]
/// 2. git commit --allow-empty --amend --only [-m "message"]
/// 3. git rebase --continue
pub fn reword_commit(
    repo: &Repository,
    commit_hash: &str,
    message: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = git::require_workdir(repo, "reword")?;

    let commit_oid = repo.revparse_single(commit_hash)?.peel_to_commit()?.id();
    let short_hash = git_commands::short_hash(commit_hash);

    let target = git::rebase_target_for_commit(repo, commit_oid)?;

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

    println!(
        "Updated commit message for {} (now {})",
        git_commands::short_hash(commit_hash),
        git_commands::short_hash(&new_hash)
    );

    Ok(())
}

/// Rename a branch using git branch -m.
pub fn reword_branch(
    repo: &Repository,
    old_name: &str,
    new_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = git::require_workdir(repo, "rename branch")?;

    git_branch::rename(workdir, old_name, new_name)?;

    println!("Renamed branch '{}' to '{}'", old_name, new_name);
    Ok(())
}

#[cfg(test)]
#[path = "reword_test.rs"]
mod tests;
