use std::path::Path;

use anyhow::{Context, Result};
use git2::Repository;
use serde::{Deserialize, Serialize};

use crate::branch;
use crate::core::repo::{self, Target};

use crate::core::agent_mode;
use crate::core::changeid;
use crate::core::msg;
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave;
use crate::git;

/// Resume context for a `reword` paused by a conflict.
#[derive(Serialize, Deserialize)]
struct RewordContext {
    /// Short hash of the reworded commit, as it was before the rebase.
    display: String,
    /// Hash the reworded commit now has; the alias reads state files that
    /// store it abbreviated, which names the commit just as well.
    #[serde(alias = "new_display")]
    new_hash: String,
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

    let commit = repo.revparse_single(commit_hash)?.peel_to_commit()?;
    let commit_oid = commit.id();
    // The identity survives the reword: re-stamped on the new message, or
    // restored after an editor that dropped it (Spec 002).
    let keep = commit.message().ok().and_then(changeid::from_message);
    let message = match &message {
        Some(m) => Some(changeid::for_message(repo, workdir, m, keep.as_deref())?),
        None => None,
    };

    // Restored whichever way the rebase ends (Spec 014).
    let saved_staged = git::diff_cached(workdir)?;

    // Step 1: Start interactive rebase with edit at target
    // No state file exists yet, so a rebase still running after a failed abort
    // leaves nobody to put the staging back: it is parked instead.
    weave::start_edit_rebase(repo, workdir, commit_oid)
        .inspect_err(|e| git::restore_or_park_after_abort(workdir, &saved_staged, e))?;

    // Step 2: Amend the commit message
    let amend = git::commit_amend(workdir, message.as_deref()).and_then(|()| match message {
        Some(_) => Ok(()),
        None => changeid::ensure_on_head(repo, workdir, keep.as_deref()),
    });
    if let Err(e) = amend {
        // Outside the cleanup closure, for the reason given on step 1: a failed
        // abort skips it, and there is still no state file.
        let e = git::rebase_abort_then_cleanup(workdir, e, || {});
        git::restore_or_park_after_abort(workdir, &saved_staged, &e);
        return Err(e);
    }

    // Capture the new hash right after amending (before rebase --continue moves HEAD)
    let new_hash = repo.head()?.peel_to_commit()?.id().to_string();

    // Step 3: Save resume state, then continue the rebase.
    let ctx = RewordContext {
        display: git::short_hash(commit_hash).to_string(),
        new_hash,
    };
    let git_dir = repo.path().to_path_buf();
    transaction::save(
        &git_dir,
        &LoomState {
            command: "reword".to_string(),
            // Nothing else to undo: `git rebase --abort` discards the amend
            // along with the rebase, and reword creates no commits or branches
            // of its own.
            rollback: Rollback {
                saved_staged_patch: saved_staged.clone(),
                ..Default::default()
            },
            context: serde_json::to_value(&ctx)?,
            protect: Vec::new(),
            targets: Vec::new(),
        },
    )?;

    // The rebase runs with `--empty=stop`, so a commit above the target whose
    // changes are already in the base halts it: drop it and carry on, the way
    // `--empty=drop` did, rather than report it as a conflict.
    let outcome = git::skip_empty_stops(
        workdir,
        &git_dir,
        git::Protected::default(),
        git::continue_rebase(workdir)?,
    )
    .inspect_err(|_| {
        // The refusal aborted the rebase itself, so there is none to abort
        // here — but that abort can fail, and then the state file and the
        // index are both still `loom abort`'s to deal with.
        if git::rebase_is_over(workdir) {
            let _ = transaction::delete(&git_dir);
            git::restore_staged_after_rebase(workdir, &saved_staged);
        }
    })?;
    match outcome {
        git::RebaseOutcome::Completed => {
            git::restore_staged_after_rebase(workdir, &saved_staged);
            transaction::delete(&git_dir)?;
            report_reworded(workdir, &ctx);
        }
        git::RebaseOutcome::Stopped => {
            transaction::warn_paused(workdir, "reword");
        }
        git::RebaseOutcome::Paused => {
            transaction::warn_paused_at_edit(Some("reword"));
        }
    }

    Ok(())
}

/// Resume a `reword` after a conflict has been resolved.
pub fn after_continue(
    workdir: &Path,
    rollback: &Rollback,
    context: &serde_json::Value,
) -> Result<()> {
    let ctx: RewordContext =
        serde_json::from_value(context.clone()).context("Failed to parse reword resume context")?;
    git::restore_staged_after_rebase(workdir, &rollback.saved_staged_patch);
    report_reworded(workdir, &ctx);
    Ok(())
}

fn report_reworded(workdir: &Path, ctx: &RewordContext) {
    msg::success(&format!(
        "Updated commit message for `{}` (now {})",
        ctx.display,
        repo::describe_commit(workdir, &ctx.new_hash)
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
