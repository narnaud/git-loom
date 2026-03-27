use anyhow::Result;
use git2::Repository;

use crate::branch;
use crate::core::repo::{self, Target};

use crate::core::msg;
use crate::core::weave;
use crate::git;

/// Reword a commit message or rename a branch.
pub fn run(target: String, message: Option<String>) -> Result<()> {
    let repo = repo::open_repo()?;

    let resolved = repo::resolve_arg(
        &repo,
        &target,
        &[repo::TargetKind::Branch, repo::TargetKind::Commit],
    )?;

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
            if new_name == name {
                return Ok(());
            }
            git::branch_validate_name(&new_name)?;
            reword_branch(&repo, &name, &new_name)
        }
        _ => unreachable!(),
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
    let workdir = repo::require_workdir(repo, "reword")?;

    let commit_oid = repo.revparse_single(commit_hash)?.peel_to_commit()?.id();

    // Step 1: Start interactive rebase with edit at target
    weave::start_edit_rebase(repo, workdir, commit_oid)?;

    // Step 2: Amend the commit message
    if let Err(e) = git::commit_amend(workdir, message.as_deref()) {
        let _ = git::rebase_abort(workdir);
        return Err(e);
    }

    // Capture the new hash right after amending (before rebase --continue moves HEAD)
    let new_hash = repo.head()?.peel_to_commit()?.id().to_string();

    // Step 3: Continue the rebase (abort automatically on conflict)
    git::continue_rebase_or_abort(workdir)?;

    msg::success(&format!(
        "Updated commit message for `{}` (now `{}`)",
        git::short_hash(commit_hash),
        git::short_hash(&new_hash)
    ));

    Ok(())
}

/// Rename a branch using git branch -m.
pub fn reword_branch(repo: &Repository, old_name: &str, new_name: &str) -> Result<()> {
    let workdir = repo::require_workdir(repo, "rename branch")?;

    git::branch_rename(workdir, old_name, new_name)?;

    branch::warn_if_hidden(repo, new_name);
    msg::success(&format!("Renamed branch `{}` to `{}`", old_name, new_name));
    Ok(())
}

#[cfg(test)]
#[path = "reword_test.rs"]
mod tests;
