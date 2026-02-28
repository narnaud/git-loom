use anyhow::{Result, bail};
use git2::Repository;

use crate::git::{self, Target};
use crate::git_commands::git_rebase;
use crate::git_commands::{self, git_branch, git_commit};
use crate::msg;
use crate::weave;

/// Reword a commit message or rename a branch.
pub fn run(target: String, message: Option<String>) -> Result<()> {
    let repo = git::open_repo()?;

    let resolved = git::resolve_target(&repo, &target)?;

    match resolved {
        Target::Commit(hash) => reword_commit(&repo, &hash, message),
        Target::Branch(name) => {
            let new_name = match message {
                Some(msg) => msg,
                None => {
                    // Prompt for new branch name with current name as placeholder
                    msg::input_with_placeholder("New branch name", &name, |s| {
                        if s.trim().is_empty() {
                            Err("Branch name cannot be empty")
                        } else {
                            Ok(())
                        }
                    })?
                }
            };
            let new_name = new_name.trim().to_string();
            git_branch::validate_name(&new_name)?;
            reword_branch(&repo, &name, &new_name)
        }
        Target::File(_) => bail!("Cannot reword a file\nUse `git add` to stage file changes"),
        Target::Unstaged => bail!("Cannot reword unstaged changes"),
        Target::CommitFile { .. } => bail!("Cannot reword a commit file"),
    }
}

/// Reword a commit message using Weave-based interactive rebase.
///
/// Approach:
/// 1. Build todo (via Weave or linear walk), mark target as `edit`
/// 2. Run rebase (pauses at the target commit)
/// 3. git commit --allow-empty --amend --only [-m "message"]
/// 4. git rebase --continue
pub fn reword_commit(repo: &Repository, commit_hash: &str, message: Option<String>) -> Result<()> {
    let workdir = git::require_workdir(repo, "reword")?;

    let commit_oid = repo.revparse_single(commit_hash)?.peel_to_commit()?.id();

    // Step 1: Start interactive rebase with edit at target
    weave::start_edit_rebase(repo, workdir, commit_oid)?;

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

    msg::success(&format!(
        "Updated commit message for `{}` (now `{}`)",
        git_commands::short_hash(commit_hash),
        git_commands::short_hash(&new_hash)
    ));

    Ok(())
}

/// Rename a branch using git branch -m.
pub fn reword_branch(repo: &Repository, old_name: &str, new_name: &str) -> Result<()> {
    let workdir = git::require_workdir(repo, "rename branch")?;

    git_branch::rename(workdir, old_name, new_name)?;

    msg::success(&format!("Renamed branch `{}` to `{}`", old_name, new_name));
    Ok(())
}

#[cfg(test)]
#[path = "reword_test.rs"]
mod tests;
