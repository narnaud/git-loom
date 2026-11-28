//! `loom continue` and `loom abort`: finish or undo the operation a resumable
//! command saved in `core::transaction` (Spec 014).

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::core::transaction::{
    LoomState, PauseReason, ROLLBACK_FAILED_HINT, Resumable, delete, load, pause_reason,
    roll_back_failed_rebase, save, warn_paused_at_edit,
};
use crate::git;

/// Emit the still-paused warning after a `loom continue` stopped again.
///
/// `subject` names what is still paused: the loom operation, or the bare
/// `rebase`/`merge` when no state file says which command it belongs to.
/// `auto_merge_before` is the `AUTO_MERGE` id read before the `--continue`, so
/// the conflict just resolved can be told from the one the user was on.
fn warn_still_paused(workdir: &Path, subject: &str, auto_merge_before: Option<&str>) {
    let (note, hint, body) = match pause_reason(workdir, auto_merge_before) {
        PauseReason::Conflicts => (
            format!("Conflicts remain — the {} is still paused", subject),
            "resolve conflicts, stage them, then run: loom continue (or loom abort)",
            "Conflicts remain — resolve them and run `loom continue` again".to_string(),
        ),
        PauseReason::ResolvedConflicts => (
            format!(
                "`rerere` resolved the next conflicts — the {} is still paused",
                subject
            ),
            "review the resolution, then run: loom continue (or loom abort)",
            "`rerere` resolved the next conflicts — review the result and run `loom continue` again"
                .to_string(),
        ),
        PauseReason::Other => (
            format!("The {} stopped again — it is still paused", subject),
            "run loom trace to see why, fix it, then run: loom continue (or loom abort)",
            format!(
                "The {} stopped again — run `loom trace` to see why, then `loom continue`",
                subject
            ),
        ),
    };

    crate::core::agent_mode::note_paused(&note, hint);
    crate::core::msg::warn_reported(&body);
}

/// Run `loom continue` (opens repo internally).
pub fn continue_run() -> Result<()> {
    let repo = crate::core::repo::open_repo()?;
    let workdir = crate::core::repo::require_workdir(&repo, "continue")?.to_path_buf();
    let git_dir = repo.path().to_path_buf();
    continue_cmd(&workdir, &git_dir)
}

/// Run `loom abort` (opens repo internally).
pub fn abort_run() -> Result<()> {
    let repo = crate::core::repo::open_repo()?;
    let workdir = crate::core::repo::require_workdir(&repo, "abort")?.to_path_buf();
    let git_dir = repo.path().to_path_buf();
    abort_cmd(&workdir, &git_dir)
}

/// Implement `loom continue`: finish an active rebase with
/// `git rebase --continue`, report paused again on a fresh conflict, and
/// otherwise dispatch to the command's `after_continue`, deleting the state
/// only once that succeeds.
pub fn continue_cmd(workdir: &Path, git_dir: &Path) -> Result<()> {
    let Some(mut state) = load(git_dir)? else {
        return continue_without_state(workdir, git_dir);
    };

    // Read before continuing: the stop git is on only says which conflict the
    // user was already looking at while it is still the current one.
    let stop_before = git::stop_id(workdir, git_dir);
    let auto_merge_before = stop_before.as_ref().map(git::StopId::auto_merge);
    if git::rebase_is_in_progress(git_dir) {
        keep_emptied_step(workdir, git_dir, &mut state)?;
        // A stop on a commit the new history already contains is not a conflict
        // to resolve (`skip_empty_stops` establishes that before skipping
        // anything). A resumable owner has already made its own rewrite by the
        // time it can pause; `protect` and `targets` cover the other commits it
        // depends on.
        // Only the empty-replay refusal is undone here (Spec 014), and only
        // once it has ended the rebase: the refusal leaves a live one when the
        // tree is dirty, and its message says to save that work before undoing
        // anything. Everything else is the user's to look at, with the state
        // still describing what `loom abort` would undo.
        let outcome = git::carry_past_known_stops(
            workdir,
            git_dir,
            state.protected(),
            stop_before.as_ref(),
            git::continue_rebase(workdir)?,
        )
        .map_err(|e| match git::replayed_empty_hash(&e) {
            Some(_) if !git::rebase_is_in_progress(git_dir) => {
                roll_back_failed_rebase(workdir, git_dir, &state, e)
            }
            _ => e,
        })?;
        match outcome {
            git::RebaseOutcome::Paused => {
                warn_paused_at_edit(Some(state.command));
                return Ok(());
            }
            git::RebaseOutcome::Stopped => {
                warn_still_paused(workdir, "operation", auto_merge_before);
                return Ok(());
            }
            git::RebaseOutcome::Completed => {}
        }
    } else if git::merge_is_in_progress(git_dir) {
        match git::continue_merge(workdir, git_dir)? {
            git::MergeOutcome::Stopped => {
                warn_still_paused(workdir, "operation", auto_merge_before);
                return Ok(());
            }
            git::MergeOutcome::Completed => {}
        }
    }
    // else: no rebase or merge is in progress — the user already ran
    // `git rebase --continue` manually, so move straight to dispatch.

    restore_skipped_fixups(workdir, &state.skipped_fixups)?;
    if dispatch_after_continue(workdir, git_dir, &state)? {
        delete(git_dir)?;
    }
    Ok(())
}

/// Act on a resolution that leaves the stopped step with nothing, before
/// git's `--continue` drops or skips it (Spec 014). A target is committed
/// empty under its own message and authorship, so the step that lands on it
/// still can; a fixup is recorded, so its change goes back to the working
/// tree once the rebase is over.
fn keep_emptied_step(workdir: &Path, git_dir: &Path, state: &mut LoomState) -> Result<()> {
    match git::emptied_stop(workdir, git_dir, &state.targets) {
        Some(git::EmptiedStop::Target(sha)) => {
            let name = crate::core::repo::describe_commit(workdir, &sha);
            git::commit_empty_copy_of(workdir, &sha).with_context(|| {
                format!(
                    "Could not keep {name} as an empty commit\n\
                     Run `loom abort`, or commit it yourself with \
                     `git commit --allow-empty --no-verify -C {}` and run `loom continue` again",
                    git::short_hash(&sha)
                )
            })?;
            crate::core::msg::warn(&format!(
                "Kept {name} as an empty commit — your resolution left it with no changes of its own"
            ));
        }
        // Not reported here: the change is still in the fixup commit, and the
        // restore says what became of it. A `--continue` that failed brings
        // the same stop back here, hence the guard.
        Some(git::EmptiedStop::Fixup(sha)) if !state.skipped_fixups.contains(&sha) => {
            state.skipped_fixups.push(sha);
            save(git_dir, state)?;
        }
        Some(git::EmptiedStop::Fixup(_)) | None => {}
    }
    Ok(())
}

/// Put the change of every fixup the user's resolution skipped back in the
/// working tree, or park it: once the rebase is over the fixup commit is all
/// that holds it (Spec 014).
///
/// Before the command's own `after_continue`: a failed apply checks files out
/// of the index, which must not yet hold what that handler restores. The
/// entries stay in the state for that handler's message, so a `loom continue`
/// retried after it failed comes back here: a change the tree already holds
/// is left as it is rather than applied twice, and one already parked is named
/// again rather than parked twice. A change that cannot be read,
/// or neither put back nor parked, fails the continue: the state keeps its
/// hash for the retry.
fn restore_skipped_fixups(workdir: &Path, fixups: &[String]) -> Result<()> {
    for sha in fixups {
        let short = git::short_hash(sha);
        // A fixup loom made is titled `fixup! <target subject>`; `fold
        // <commit> <commit>` squashes the source commit itself.
        let subject = git::commit_subject(workdir, sha).ok();
        let (what, kept) = match subject.as_deref().and_then(|s| s.strip_prefix("fixup! ")) {
            Some(target) => (format!("The change meant for `{target}`"), "the commit"),
            None => (format!("The change of `{short}`"), "its target"),
        };
        let patch = git::diff_commit(workdir, sha).with_context(|| {
            format!(
                "{what} could not be read back from `{short}`\n\
                 Fix what keeps git from reading it, then run `loom continue` again"
            )
        })?;
        let back = || {
            crate::core::msg::warn(&format!(
                "{what} is back in your working tree — your resolution kept {kept} unchanged"
            ));
        };
        if git::patch_is_applied(workdir, &patch) {
            back();
            continue;
        }
        if let Some(path) = git::find_saved_patch(workdir, "unfolded", &patch) {
            crate::core::msg::warn(&format!(
                "{what} is already saved as a patch — replay it with `{}`",
                git::Replay::Worktree.command(&path)
            ));
            continue;
        }
        match git::apply_patch_to_worktree(workdir, &patch) {
            Ok(()) => back(),
            Err(e) => {
                crate::core::msg::warn(&format!(
                    "{what} could not be put back in your working tree: {e}"
                ));
                if !git::save_or_warn(workdir, "unfolded", &patch, git::Replay::Worktree) {
                    bail!(
                        "{what} could not be saved from `{short}`\n\
                         Fix what keeps loom from saving its patch, then run `loom continue` again"
                    );
                }
            }
        }
    }
    Ok(())
}

/// What to say when the abort itself fails: nothing was rolled back, so running
/// `loom abort` again once git is free finishes the job.
static ABORT_FAILED_HINT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!(
        "the abort failed — the loom state was kept, so run `loom abort` again once git is free\n\
         ({})",
        git::ABORT_FAILED_CAUSE
    )
});

/// Implement `loom abort`: `git rebase --abort` first — it restores HEAD,
/// branch refs via `--update-refs`, and any autostash, and the state file is
/// kept if it fails — then `rollback.apply_abort()` for the cleanup git
/// cannot do on its own, then delete the state.
pub fn abort_cmd(workdir: &Path, git_dir: &Path) -> Result<()> {
    let Some(state) = load(git_dir)? else {
        return abort_without_state(workdir, git_dir);
    };

    // A failed abort leaves the rebase running: rolling back on top of it
    // (`reset --hard`, branch deletions) would make the mess worse, and the
    // state file is the only record of what to undo — so keep both and stop.
    if git::rebase_is_in_progress(git_dir) {
        git::rebase_abort(workdir).context(ABORT_FAILED_HINT.as_str())?;
    } else if git::merge_is_in_progress(git_dir) && state.rollback.reset_hard_to.is_empty() {
        // With a hard reset to follow, which ends the merge too, skipped:
        // `git merge --abort` refuses a file staged and then edited again.
        git::merge_abort(workdir).context(ABORT_FAILED_HINT.as_str())?;
    }

    state
        .rollback
        .apply_abort(workdir)
        .context(ROLLBACK_FAILED_HINT)?;
    delete(git_dir)?;

    crate::core::msg::success(&format!(
        "Aborted `loom {}` and restored original state",
        state.command
    ));
    Ok(())
}

/// Error for `continue`/`abort` with neither a state file nor git work to drive.
const NO_OPERATION: &str = "No loom operation is in progress";

/// The git operation a stateless `continue`/`abort` acts on.
#[derive(Clone, Copy)]
enum GitOp {
    Rebase,
    Merge,
}

impl GitOp {
    fn as_str(self) -> &'static str {
        match self {
            GitOp::Rebase => "rebase",
            GitOp::Merge => "merge",
        }
    }
}

/// `loom continue` with no state file: finish a rebase or merge git still has
/// in progress. A command whose conflict path is not resumable, or one that
/// died before saving state, can leave one behind.
fn continue_without_state(workdir: &Path, git_dir: &Path) -> Result<()> {
    let before = git::auto_merge_id(workdir);
    let before = before.as_deref();
    if git::rebase_is_in_progress(git_dir) {
        // No `skip_empty_stops` here: a rebase with no loom state behind it may
        // well be the user's own, and `rebase -i` halts on an empty replay on
        // purpose, to let them decide.
        match git::continue_rebase(workdir)? {
            git::RebaseOutcome::Paused => warn_paused_at_edit(None),
            git::RebaseOutcome::Stopped => {
                warn_still_paused(workdir, GitOp::Rebase.as_str(), before)
            }
            git::RebaseOutcome::Completed => report_stateless_continue(GitOp::Rebase),
        }
    } else if git::merge_is_in_progress(git_dir) {
        match git::continue_merge(workdir, git_dir)? {
            git::MergeOutcome::Stopped => warn_still_paused(workdir, GitOp::Merge.as_str(), before),
            git::MergeOutcome::Completed => report_stateless_continue(GitOp::Merge),
        }
    } else {
        bail!(NO_OPERATION);
    }
    Ok(())
}

/// Report a stateless `continue`, spelling out that only git's own step ran:
/// with no state file there is no command to finish off, so no saved patch is
/// re-staged, no temp branch removed, and no per-command success line printed.
fn report_stateless_continue(op: GitOp) {
    crate::core::msg::success(&format!(
        "Completed the {} git had in progress (no loom state, so nothing else was done)",
        op.as_str()
    ));
}

/// Report a stateless `abort`, spelling out that only git's own abort ran: with
/// no state file, a commit or temp branch a loom command left behind stays.
fn report_stateless_abort(op: GitOp) {
    crate::core::msg::success(&format!(
        "Canceled the {} git had in progress (no loom state to roll back)",
        op.as_str()
    ));
}

/// What to say when a stateless abort fails. There is no loom state to keep, so
/// unlike [`ABORT_FAILED_HINT`] this only reports git's failure and its cause.
fn stateless_abort_failed(op: GitOp) -> String {
    format!(
        "`git {} --abort` failed ({})",
        op.as_str(),
        git::ABORT_FAILED_CAUSE
    )
}

/// `loom abort` with no state file: cancel the rebase or merge git still has in
/// progress, so the repository never stays stuck mid-rewrite.
fn abort_without_state(workdir: &Path, git_dir: &Path) -> Result<()> {
    let op = if git::rebase_is_in_progress(git_dir) {
        GitOp::Rebase
    } else if git::merge_is_in_progress(git_dir) {
        GitOp::Merge
    } else {
        bail!(NO_OPERATION);
    };

    match op {
        GitOp::Rebase => git::rebase_abort(workdir),
        GitOp::Merge => git::merge_abort(workdir),
    }
    .with_context(|| stateless_abort_failed(op))?;

    report_stateless_abort(op);
    Ok(())
}

/// Run the command's `after_continue`. `Ok(false)`: it paused again, so the
/// state stays (a merge whose set-aside work conflicts with it, Spec 005).
fn dispatch_after_continue(workdir: &Path, git_dir: &Path, state: &LoomState) -> Result<bool> {
    match state.command {
        Resumable::Merge => return crate::merge::after_continue(workdir, git_dir, state),
        Resumable::Update => {
            crate::update::after_continue(workdir, &state.rollback, &state.context)
        }
        Resumable::Commit => {
            crate::commit::after_continue(workdir, &state.rollback, &state.context)
        }
        Resumable::Absorb => crate::absorb::after_continue(
            workdir,
            &state.rollback,
            &state.context,
            &state.skipped_fixups,
        ),
        Resumable::Drop => crate::drop::after_continue(workdir, &state.rollback, &state.context),
        Resumable::Fold => crate::fold::after_continue(
            workdir,
            &state.rollback,
            &state.context,
            &state.skipped_fixups,
        ),
        Resumable::Reword => {
            crate::reword::after_continue(workdir, &state.rollback, &state.context)
        }
        Resumable::Swap => crate::swap::after_continue(workdir, &state.rollback, &state.context),
    }
    .map(|()| true)
}

#[cfg(test)]
#[path = "resume_test.rs"]
mod tests;
