use std::path::Path;

use anyhow::{Context, Result};
use git2::Repository;
use serde::{Deserialize, Serialize};

use crate::branch;
use crate::core::repo::{self, Target};

use crate::core::agent_mode;
use crate::core::msg;
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave;
use crate::git;

/// Resume context for a `reword` paused by a conflict.
#[derive(Serialize, Deserialize)]
struct RewordContext {
    /// Short hash of the reworded commit, as it was before the rebase.
    display: String,
    /// Short hash the reworded commit now has.
    new_display: String,
}

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
                    msg::input_with_placeholder(
                        "New branch name",
                        &name,
                        "re-run with: loom reword <target> -m <new-name>",
                        |s| {
                            if s.trim().is_empty() {
                                Err("Branch name cannot be empty")
                            } else {
                                Ok(())
                            }
                        },
                    )?
                }
            };
            let new_name = new_name.trim().to_string();
            if new_name == name {
                return Ok(());
            }
            let workdir = repo::require_workdir(&repo, "reword")?;
            git::branch_validate_name(workdir, &new_name)?;
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
///
/// Step 4 can conflict: rewriting the target changes the SHAs above it, so any
/// merge commit in the way has to be rebuilt, and a merge that was resolved by
/// hand conflicts again. That is resumable work, so the reword pauses for
/// `loom continue` rather than throwing the amend away.
pub fn reword_commit(repo: &Repository, commit_hash: &str, message: Option<String>) -> Result<()> {
    // Without -m the amend would open $GIT_EDITOR, which hangs a headless agent.
    if agent_mode::enabled() && message.is_none() {
        return Err(agent_mode::respond_needs_input(
            agent_mode::InputKind::Text,
            "Commit message",
            vec![],
            false,
            "re-run with: loom reword <target> -m <message>",
        ));
    }

    let workdir = repo::require_workdir(repo, "reword")?;

    let commit_oid = repo.revparse_single(commit_hash)?.peel_to_commit()?.id();

    // Step 1: Start interactive rebase with edit at target
    weave::start_edit_rebase(repo, workdir, commit_oid)?;

    // Step 2: Amend the commit message
    if let Err(e) = git::commit_amend(workdir, message.as_deref()) {
        return Err(git::rebase_abort_then_cleanup(workdir, e, || {}));
    }

    // Capture the new hash right after amending (before rebase --continue moves HEAD)
    let new_hash = repo.head()?.peel_to_commit()?.id().to_string();

    // Step 3: Save resume state, then continue the rebase.
    let ctx = RewordContext {
        display: git::short_hash(commit_hash).to_string(),
        new_display: git::short_hash(&new_hash).to_string(),
    };
    let git_dir = repo.path().to_path_buf();
    transaction::save(
        &git_dir,
        &LoomState {
            command: "reword".to_string(),
            // Nothing to undo beyond the rebase itself: `git rebase --abort`
            // discards the amend along with it, and reword creates no commits,
            // branches, or saved patches of its own.
            rollback: Rollback::default(),
            context: serde_json::to_value(&ctx)?,
        },
    )?;

    match git::continue_rebase(workdir)? {
        git::RebaseOutcome::Completed => {
            transaction::delete(&git_dir)?;
            report_reworded(&ctx);
        }
        git::RebaseOutcome::Stopped => {
            transaction::warn_conflict_paused(workdir, "reword");
        }
        git::RebaseOutcome::Paused => {
            transaction::warn_paused_at_edit(Some("reword"));
        }
    }

    Ok(())
}

/// Resume a `reword` after a conflict has been resolved.
pub fn after_continue(_workdir: &Path, context: &serde_json::Value) -> Result<()> {
    let ctx: RewordContext =
        serde_json::from_value(context.clone()).context("Failed to parse reword resume context")?;
    report_reworded(&ctx);
    Ok(())
}

fn report_reworded(ctx: &RewordContext) {
    msg::success(&format!(
        "Updated commit message for `{}` (now `{}`)",
        ctx.display, ctx.new_display
    ));
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
