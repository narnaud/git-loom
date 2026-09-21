use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::git;

/// Persistent state saved when a loom command is paused due to a rebase conflict.
#[derive(Debug, Serialize, Deserialize)]
pub struct LoomState {
    /// The name of the interrupted command (e.g., "update", "commit").
    pub command: String,
    /// Shared rollback information for `loom abort`.
    pub rollback: Rollback,
    /// Commits `loom continue` must not let the rebase drop as empty, because
    /// a ref this command reads back follows them (see
    /// [`crate::core::weave::run_rebase_protecting`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protect: Vec<String>,
    /// Command-specific resume context (opaque JSON).
    pub context: serde_json::Value,
}

/// Rollback information captured before the rebase step starts.
///
/// Only fields that are actually consumed by a command's `after_abort` handler
/// belong here. `git rebase --abort` already restores HEAD, all branch refs
/// (via `--update-refs`), and autostashed working-tree changes — so those do
/// not need to be saved.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Rollback {
    /// HEAD OID to `reset --mixed` to on abort.
    #[serde(default)]
    pub reset_mixed_to: String,
    /// HEAD OID to `reset --hard` to on abort.
    #[serde(default)]
    pub reset_hard_to: String,
    /// Branches created during this operation that should be deleted on abort.
    #[serde(default)]
    pub delete_branches: Vec<String>,
    /// Staged diff saved aside during the operation (may be empty).
    #[serde(default)]
    pub saved_staged_patch: String,
    /// Working-tree diff saved before the rebase (may be empty).
    #[serde(default)]
    pub saved_worktree_patch: String,
}

impl Rollback {
    /// Whether this undo takes HEAD back, which only a caller that moved it
    /// itself, before the rebase, records. A caller that unstages before the
    /// rebase without moving HEAD must keep its own guard instead (Spec 014).
    pub fn takes_head_back(&self) -> bool {
        !self.reset_mixed_to.is_empty() || !self.reset_hard_to.is_empty()
    }

    /// Remove the refs the operation created, and nothing else.
    ///
    /// For a failure that rewrote nothing: the temp branches are still loom's
    /// to clean up, while the index and worktree were never touched.
    pub fn delete_temp_branches(&self, workdir: &Path) {
        for branch in &self.delete_branches {
            let _ = git::branch_delete(workdir, branch);
        }
    }

    /// Apply the rollback after `git rebase --abort` has run, acting on whichever
    /// fields are populated.
    pub fn apply_abort(&self, workdir: &Path) -> Result<()> {
        if !self.reset_mixed_to.is_empty() {
            git::reset_mixed(workdir, &self.reset_mixed_to)?;
        }
        if !self.reset_hard_to.is_empty() {
            git::reset_hard(workdir, &self.reset_hard_to)?;
        }
        for branch in &self.delete_branches {
            let _ = git::branch_delete(workdir, branch);
        }
        if self.reset_mixed_to.is_empty() && self.reset_hard_to.is_empty() {
            git::restore_staged_after_rebase(workdir, &self.saved_staged_patch);
        } else {
            // A reset above already put the index at HEAD, which is what the
            // saved HEAD-to-index patch applies over. Those resets take a commit
            // back, so they run even over the unmerged index a conflicted
            // autostash replay leaves — skipping them would strand the undo
            // half-done.
            git::restore_staged_patch(workdir, &self.saved_staged_patch);
        }
        if !self.saved_worktree_patch.is_empty()
            && let Err(e) = git::apply_patch(workdir, &self.saved_worktree_patch)
        {
            // Parked for the same reason as the staged half above: the state
            // file holding this patch is deleted as soon as the abort reports
            // success, so whatever a reset here did or did not take, nothing
            // else keeps a copy.
            crate::core::msg::warn(&format!("could not re-apply working-tree changes: {e}"));
            git::save_or_warn(
                workdir,
                "unrestored",
                &self.saved_worktree_patch,
                git::Replay::Worktree,
            );
        }
        Ok(())
    }
}

/// Return the path to the state file: `<git_dir>/loom/state.json`.
pub fn state_path(git_dir: &Path) -> PathBuf {
    git_dir.join("loom").join("state.json")
}

/// Save `state` to `.git/loom/state.json`.
///
/// Creates `.git/loom/` if it does not exist.
pub fn save(git_dir: &Path, state: &LoomState) -> Result<()> {
    let path = state_path(git_dir);
    let parent = path
        .parent()
        .context("State file path has no parent directory")?;
    std::fs::create_dir_all(parent).with_context(|| {
        format!(
            "Failed to create loom state directory '{}'",
            parent.display()
        )
    })?;

    // Write beside the real file and rename over it, so a process killed
    // mid-write leaves either the old state or the new one — never a truncated
    // file both `loom continue` and `loom abort` would refuse to read. The temp
    // name is random and removed on drop, so two loom processes saving at once
    // cannot overwrite each other's.
    let json = serde_json::to_string_pretty(state)?;
    (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(json.as_bytes())?;
        tmp.persist(&path)?;
        Ok(())
    })()
    .with_context(|| format!("Failed to write state file '{}'", path.display()))
}

/// Load the state file. Returns `None` if the file does not exist.
pub fn load(git_dir: &Path) -> Result<Option<LoomState>> {
    let path = state_path(git_dir);
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read state file '{}'", path.display()))?;
    let state: LoomState = serde_json::from_str(&json).with_context(|| {
        // Never suggest deleting it: the file is the only record of what to
        // undo, and without it `loom abort` runs `git rebase --abort` and
        // nothing else — a temp branch, a pre-rebase commit and a saved staged
        // patch would all be stranded with no way back.
        format!(
            "State file '{}' is corrupted or invalid\n\
             Move it aside (keep it — it is the only record of what `loom abort` would undo),\n\
             then run `loom abort` to cancel whatever git still has in progress",
            path.display()
        )
    })?;
    Ok(Some(state))
}

/// Delete the state file.
///
/// No-ops if the file does not exist.
pub fn delete(git_dir: &Path) -> Result<()> {
    let path = state_path(git_dir);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to delete state file '{}'", path.display()))?;
    }
    Ok(())
}

/// Turn a failed rebase into an error, clearing the state it would leave behind.
///
/// Aborts a rebase that did start, re-stages what its autostash replay left
/// unstaged, then removes the state file — left behind, it reports a paused
/// operation to every later loom command. A rebase can also fail before
/// starting at all (a branch checked out in another worktree): that index was
/// never touched.
///
/// An abort that fails is the exception: the rebase is still on disk, so the
/// state file stays for `loom abort`, patch and all.
///
/// Takes the saved patch rather than the whole `Rollback`, which would suggest
/// the rest of it gets applied here — that is `apply_abort`'s job.
pub fn discard_state_after(
    workdir: &Path,
    git_dir: &Path,
    saved_staged_patch: &str,
    cause: anyhow::Error,
) -> anyhow::Error {
    // Only the pre-flight closure is tagged, so anything else — the spawn, a
    // failure before it — counts as autostashed. Erring that way costs nothing
    // visible: re-applying a patch the index already holds is not itself a
    // no-op — a staged deletion whose entry is gone makes git refuse the whole
    // patch — but `restore_staged_after_rebase` tests for that index first.
    let autostashed = !git::rebase_never_started(&cause);
    git::rebase_abort_then_cleanup(workdir, cause, || {
        if autostashed {
            git::restore_staged_after_rebase(workdir, saved_staged_patch);
        }
        if let Err(e) = delete(git_dir) {
            crate::core::msg::warn(&format!("could not remove the loom state file: {e}"));
        }
    })
}

/// Why a rebase or merge stopped, as far as git's leftover state can tell.
#[derive(Debug, PartialEq, Eq)]
enum PauseReason {
    /// Unmerged paths: there is something for the user to resolve.
    Conflicts,
    /// A conflict the user never had to touch: `rerere` replayed a recorded
    /// resolution, and `rerere.autoUpdate` staged it.
    ResolvedConflicts,
    /// Anything else: an untracked file in the way, a failing `exec`, a stale
    /// `index.lock`, a hook rejecting the commit a `--continue` tried to make.
    Other,
}

/// Classify a stop from the state git left behind.
///
/// `before` is the `AUTO_MERGE` id from before the step that just stopped, if
/// there was one. A resolved conflict is only news when that id changed: an
/// unchanged one is the conflict the user was already on, so nothing resolved
/// it for them and the step failed for some other reason.
fn pause_reason(workdir: &Path, before: Option<&str>) -> PauseReason {
    if git::has_unmerged_paths(workdir) {
        PauseReason::Conflicts
    } else if git::auto_merge_id(workdir).is_some_and(|id| Some(id.as_str()) != before) {
        PauseReason::ResolvedConflicts
    } else {
        PauseReason::Other
    }
}

/// Emit the pause warning for a resumable command whose rebase or merge
/// stopped.
///
/// A conflict to resolve is the usual reason, but not the only one, so the
/// message follows what git left behind.
pub fn warn_paused(workdir: &Path, command: &str) {
    let (note, hint, cause) = match pause_reason(workdir, None) {
        PauseReason::Conflicts => (
            format!("Conflicts detected — the `loom {}` is paused", command),
            "resolve conflicts, stage them, then run: loom continue (or loom abort)",
            "Conflicts detected — resolve them with git, then run:",
        ),
        PauseReason::ResolvedConflicts => (
            format!(
                "`rerere` resolved the conflicts — the `loom {}` is paused",
                command
            ),
            "review the resolution, then run: loom continue (or loom abort)",
            "`rerere` resolved the conflicts for you — review the result, then run:",
        ),
        PauseReason::Other => (
            format!("The `loom {}` is paused — it stopped part-way", command),
            "run loom trace to see why, fix it, then run: loom continue (or loom abort)",
            "The operation stopped part-way — run `loom trace` to see why, then:",
        ),
    };

    crate::core::agent_mode::note_paused(&note, hint);
    crate::core::msg::warn_reported(&format!(
        "{}\n\
         `loom continue`   to complete the {}\n\
         `loom abort`      to cancel and restore original state",
        cause, command
    ));
}

/// Emit the warning for a rebase that reached an `edit` step: git exits 0
/// there, but the rebase is not over.
///
/// `command` is the loom command the rebase belongs to, or `None` when there is
/// no state file to say which one it was — and then `loom abort` cancels the
/// rebase without rolling anything else back, so it must not promise more.
pub fn warn_paused_at_edit(command: Option<&str>) {
    let (owner, abort_hint) = match command {
        Some(c) => (
            format!("The `loom {}` is paused at an `edit` step", c),
            "to cancel and restore original state",
        ),
        None => (
            "The rebase is paused at an `edit` step".to_string(),
            "to cancel it (no loom state to roll back)",
        ),
    };
    crate::core::agent_mode::note_paused(
        &owner,
        "finish the work there, then run: loom continue (or loom abort)",
    );
    crate::core::msg::warn_reported(&format!(
        "{} — finish the work there, then run:\n\
         `loom continue`   to carry on\n\
         `loom abort`      {}",
        owner, abort_hint
    ));
}

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
    let Some(state) = load(git_dir)? else {
        return continue_without_state(workdir, git_dir);
    };

    // Read before continuing: `AUTO_MERGE` only says which conflict git is on
    // once there is something to compare it against.
    let auto_merge_before = git::auto_merge_id(workdir);
    if git::rebase_is_in_progress(git_dir) {
        // A stop on a commit the new history already contains is not a conflict
        // to resolve (`skip_empty_stops` establishes that before skipping
        // anything). A resumable owner has already made its own rewrite by the
        // time it can pause; `protect` covers the commits it only follows.
        // Only the empty-replay refusal is undone here (Spec 014), and only
        // once it has ended the rebase: the refusal leaves a live one when the
        // tree is dirty, and its message says to save that work before undoing
        // anything. Everything else is the user's to look at, with the state
        // still describing what `loom abort` would undo.
        let outcome = git::skip_empty_stops(
            workdir,
            git_dir,
            &state.protect,
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
                warn_paused_at_edit(Some(&state.command));
                return Ok(());
            }
            git::RebaseOutcome::Stopped => {
                warn_still_paused(workdir, "operation", auto_merge_before.as_deref());
                return Ok(());
            }
            git::RebaseOutcome::Completed => {}
        }
    } else if git::merge_is_in_progress(git_dir) {
        match git::continue_merge(workdir, git_dir)? {
            git::MergeOutcome::Stopped => {
                warn_still_paused(workdir, "operation", auto_merge_before.as_deref());
                return Ok(());
            }
            git::MergeOutcome::Completed => {}
        }
    }
    // else: no rebase or merge is in progress — the user already ran
    // `git rebase --continue` manually, so move straight to dispatch.

    dispatch_after_continue(workdir, &state)?;
    delete(git_dir)?;
    Ok(())
}

/// Undo a saved operation whose rebase failed, and remove its state file.
///
/// The rebase is normally over by the time this runs, so what is left is
/// `loom abort`'s half of the undo; the state goes with it, or `main.rs`
/// refuses every later command as paused. A rebase still in progress is the
/// exception: rolling back on top of one makes the mess worse, so the state
/// stays for `loom abort`. `cause` is returned either way — the top-level
/// handler prints one message, and it must be the reason the command failed.
pub fn roll_back_failed_rebase(
    workdir: &Path,
    git_dir: &Path,
    state: &LoomState,
    cause: anyhow::Error,
) -> anyhow::Error {
    if git::rebase_is_in_progress(git_dir) || git::merge_is_in_progress(git_dir) {
        crate::core::msg::warn(
            "the rebase is still in progress, so nothing was rolled back — run `loom abort`",
        );
        return cause;
    }
    // A pre-flight refusal only has something to undo where loom moved HEAD
    // itself before the rebase existed — `commit`'s commit, `absorb`'s fixups —
    // which is what a recorded reset target means. With none, nothing was
    // autostashed and the index is still the user's: replaying a saved patch
    // over it would put back staging they never lost. The refs loom made are
    // its own to take back either way.
    if git::rebase_never_started(&cause) && !state.rollback.takes_head_back() {
        state.rollback.delete_temp_branches(workdir);
        if let Err(e) = delete(git_dir) {
            crate::core::msg::warn(&format!("could not remove the loom state file: {e}"));
        }
        return cause;
    }
    if let Err(e) = state.rollback.apply_abort(workdir) {
        crate::core::msg::warn(&format!("{ROLLBACK_FAILED_HINT} ({e})"));
        return cause;
    }
    if let Err(e) = delete(git_dir) {
        // Left behind, it reports a paused operation to every later command.
        crate::core::msg::warn(&format!("could not remove the loom state file: {e}"));
    }
    // The rollback may have taken the commit the refusal named with it —
    // `commit`'s does — and `loom drop` cannot find one history no longer has.
    match git::replayed_empty_hash(&cause) {
        Some(sha) if !git::reaches_from_head(workdir, sha) => {
            let sha = sha.to_string();
            empty_replay_rolled_back(cause, &sha, &state.command)
        }
        _ => cause,
    }
}

/// The empty-replay refusal for an operation whose undo removes the commit.
///
/// Built on `cause` so the `ReplayedEmpty` marker stays classifiable above.
fn empty_replay_rolled_back(cause: anyhow::Error, sha: &str, command: &str) -> anyhow::Error {
    let short = git::short_hash(sha);
    cause.context(format!(
        "Commit `{short}` {}\n\
         The `loom {command}` was rolled back, so there is nothing left to drop",
        git::REPLAYS_EMPTY
    ))
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

/// What to say when the abort worked but the rollback after it did not. Blaming
/// git would be wrong here — git is free, and the undo is half-applied.
const ROLLBACK_FAILED_HINT: &str =
    "the rollback is only half-applied — the loom state was kept so `loom abort` can finish it";

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
    } else if git::merge_is_in_progress(git_dir) {
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

fn dispatch_after_continue(workdir: &Path, state: &LoomState) -> Result<()> {
    match state.command.as_str() {
        "update" => crate::update::after_continue(workdir, &state.rollback, &state.context),
        "commit" => crate::commit::after_continue(workdir, &state.rollback, &state.context),
        "absorb" => crate::absorb::after_continue(workdir, &state.rollback, &state.context),
        "drop" => crate::drop::after_continue(workdir, &state.rollback, &state.context),
        "fold" => crate::fold::after_continue(workdir, &state.rollback, &state.context),
        "reword" => crate::reword::after_continue(workdir, &state.rollback, &state.context),
        "swap" => crate::swap::after_continue(workdir, &state.rollback, &state.context),
        "merge" => crate::branch::merge::after_continue(&state.context),
        other => bail!("Unknown command '{}' in loom state file", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip() {
        let state = LoomState {
            command: "commit".to_string(),
            rollback: Rollback {
                reset_mixed_to: "abc123".to_string(),
                delete_branches: vec!["new-branch".to_string()],
                saved_staged_patch: "--- a/foo\n+++ b/foo\n".to_string(),
                ..Default::default()
            },
            context: serde_json::json!({ "branch_name": "feature" }),
            protect: vec!["def456".to_string()],
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        let restored: LoomState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.command, "commit");
        assert_eq!(restored.rollback.reset_mixed_to, "abc123");
        assert_eq!(restored.rollback.delete_branches, vec!["new-branch"]);
        assert_eq!(restored.protect, vec!["def456"]);
    }

    /// A state file written before `protect` existed must still load.
    #[test]
    fn state_without_protect_loads() {
        let json = r#"{"command":"fold","rollback":{},"context":null}"#;
        let restored: LoomState = serde_json::from_str(json).unwrap();
        assert!(restored.protect.is_empty());
    }

    /// `--empty` outlives `git rebase --continue`, so the state file is what
    /// carries the protection across the pause.
    #[test]
    fn continue_refuses_when_a_protected_commit_replays_empty() {
        // Paused at an `edit` rather than a conflict: either way the rebase
        // that continues is the one `run_rebase_protecting` started, and the
        // redundant commit above the stop replays on the continue.
        let (t, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_above();
        let workdir = t.workdir();
        let git_dir = t.repo.path().to_path_buf();
        let redundant = t.get_branch_target("alpha").to_string();

        let mut graph = crate::core::weave::Weave::from_repo(&t.repo).unwrap();
        assert!(graph.edit_commit(keeper));
        let protect = vec![redundant.clone()];
        let outcome = crate::core::weave::run_rebase_protecting(
            &workdir,
            Some(&graph.base_oid.to_string()),
            &graph.to_todo(),
            &protect,
        )
        .unwrap();
        assert_eq!(outcome, git::RebaseOutcome::Paused);

        t.create_branch_at("_loom-track", &redundant);
        save(
            &git_dir,
            &LoomState {
                command: "fold".to_string(),
                rollback: Rollback {
                    delete_branches: vec!["_loom-track".to_string()],
                    ..Default::default()
                },
                context: serde_json::Value::Null,
                protect,
            },
        )
        .unwrap();

        let err = continue_cmd(&workdir, &git_dir).unwrap_err().to_string();

        assert!(err.contains("is redundant"), "{err}");
        assert!(
            err.contains("loom drop"),
            "this rollback keeps the commit, so the hint stands: {err}"
        );
        assert!(!git::rebase_is_in_progress(&git_dir), "{err}");
        assert!(
            !state_path(&git_dir).exists(),
            "the state must go with the rollback, or `loom drop` is refused"
        );
        assert!(
            !t.branch_exists("_loom-track"),
            "the rollback runs too, not just the state removal"
        );
    }

    /// The refusal leaves a live rebase when the tree is dirty, because the
    /// undo is a hard reset: `continue` must leave the work, the rebase and the
    /// state alone. (That it also stops warning "run `loom abort`" over a
    /// message saying to save the work first is not visible from here.)
    #[test]
    fn continue_does_not_undo_over_a_dirty_tree() {
        let (t, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_above();
        let workdir = t.workdir();
        let git_dir = t.repo.path().to_path_buf();
        let mut graph = crate::core::weave::Weave::from_repo(&t.repo).unwrap();
        assert!(graph.edit_commit(keeper));
        // Every commit of the branch, so whichever replays empty first is one
        // the refusal has to catch.
        let base = graph.base_oid.to_string();
        let protect: Vec<String> =
            git::run_git_stdout(&workdir, &["rev-list", &format!("{base}..alpha")])
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
        crate::core::weave::run_rebase_protecting(
            &workdir,
            Some(&base),
            &graph.to_todo(),
            &protect,
        )
        .unwrap();

        t.write_file("three.txt", "edited while paused\n");
        save(
            &git_dir,
            &LoomState {
                command: "commit".to_string(),
                rollback: Rollback {
                    reset_mixed_to: base.clone(),
                    ..Default::default()
                },
                context: serde_json::Value::Null,
                protect,
            },
        )
        .unwrap();

        let err = continue_cmd(&workdir, &git_dir).unwrap_err().to_string();

        assert!(err.contains("is redundant"), "{err}");
        assert!(err.contains("stash"), "{err}");
        assert_eq!(t.read_file("three.txt"), "edited while paused\n", "{err}");
        assert!(state_path(&git_dir).exists(), "nothing was undone: {err}");
        assert!(git::rebase_is_in_progress(&git_dir), "{err}");
        git::rebase_abort(&workdir).unwrap();
    }

    /// An undo that resets past the commit takes it with it, so the refusal
    /// must not send the user after it.
    #[test]
    fn continue_drops_the_drop_hint_when_the_rollback_removes_the_commit() {
        let (t, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_above();
        let workdir = t.workdir();
        let git_dir = t.repo.path().to_path_buf();
        let redundant = t.get_branch_target("alpha").to_string();

        let mut graph = crate::core::weave::Weave::from_repo(&t.repo).unwrap();
        assert!(graph.edit_commit(keeper));
        let protect = vec![redundant];
        crate::core::weave::run_rebase_protecting(
            &workdir,
            Some(&graph.base_oid.to_string()),
            &graph.to_todo(),
            &protect,
        )
        .unwrap();

        // `commit`'s shape: its undo resets past the commit it made, leaving
        // the one the refusal names outside the history `loom drop` searches.
        save(
            &git_dir,
            &LoomState {
                command: "commit".to_string(),
                rollback: Rollback {
                    reset_mixed_to: graph.base_oid.to_string(),
                    ..Default::default()
                },
                context: serde_json::Value::Null,
                protect,
            },
        )
        .unwrap();

        let err = continue_cmd(&workdir, &git_dir).unwrap_err().to_string();

        assert!(err.contains("is redundant"), "{err}");
        assert!(!err.contains("loom drop"), "{err}");
        assert!(err.contains("rolled back"), "{err}");
        assert!(!state_path(&git_dir).exists(), "{err}");
    }

    /// Rolling back on top of a live rebase would make the mess worse, so the
    /// state has to survive for `loom abort`.
    #[test]
    fn a_failed_abort_keeps_the_state_for_loom_abort() {
        let t = repo_stopped_on_conflict();
        let workdir = t.workdir();
        let git_dir = t.repo.path().to_path_buf();
        let state = LoomState {
            command: "fold".to_string(),
            rollback: Rollback {
                delete_branches: vec!["topic".to_string()],
                ..Default::default()
            },
            context: serde_json::Value::Null,
            protect: Vec::new(),
        };
        save(&git_dir, &state).unwrap();

        let err = roll_back_failed_rebase(&workdir, &git_dir, &state, anyhow::anyhow!("boom"));

        assert_eq!(err.to_string(), "boom");
        assert!(state_path(&git_dir).exists());
        assert!(t.branch_exists("topic"), "the rollback must not have run");
        git::rebase_abort(&workdir).unwrap();
    }

    /// The other half of the rule: with no reset recorded, loom moved nothing
    /// before the rebase, so the index is still the user's and replaying the
    /// saved patch over it would double their staging.
    #[test]
    fn a_pre_flight_refusal_leaves_an_untouched_index_alone() {
        let t = crate::core::test_helpers::TestRepo::new();
        let workdir = t.workdir();
        let git_dir = t.repo.path().to_path_buf();
        t.write_file(
            "staged.txt",
            "the user's own
",
        );
        t.stage_files(&["staged.txt"]);
        let staged = git::diff_cached(&workdir).unwrap();

        let state = LoomState {
            command: "fold".to_string(),
            rollback: Rollback {
                saved_staged_patch: staged.clone(),
                ..Default::default()
            },
            context: serde_json::Value::Null,
            protect: Vec::new(),
        };
        save(&git_dir, &state).unwrap();

        let cause = git::before_rebase_starts::<()>(Err(anyhow::anyhow!("checked out elsewhere")))
            .expect_err("tagged as raised before the rebase started");
        roll_back_failed_rebase(&workdir, &git_dir, &state, cause);

        assert_eq!(git::diff_cached(&workdir).unwrap(), staged);
        assert!(!state_path(&git_dir).exists());
    }

    /// A half-applied undo is `loom abort`'s to finish, so the state stays.
    #[test]
    fn a_failed_rollback_keeps_the_state_too() {
        let t = crate::core::test_helpers::TestRepo::new();
        let workdir = t.workdir();
        let git_dir = t.repo.path().to_path_buf();
        let state = LoomState {
            command: "commit".to_string(),
            rollback: Rollback {
                reset_mixed_to: "0".repeat(40),
                ..Default::default()
            },
            context: serde_json::Value::Null,
            protect: Vec::new(),
        };
        save(&git_dir, &state).unwrap();

        let err = roll_back_failed_rebase(&workdir, &git_dir, &state, anyhow::anyhow!("boom"));

        assert_eq!(err.to_string(), "boom");
        assert!(state_path(&git_dir).exists());
    }

    /// Stop a rebase on a conflict and return the repo it happened in.
    fn repo_stopped_on_conflict() -> crate::core::test_helpers::TestRepo {
        use crate::core::test_helpers::TestRepo;
        let test_repo = TestRepo::new();
        let workdir = test_repo.workdir();

        test_repo.write_file("f.txt", "base\n");
        test_repo.stage_files(&["f.txt"]);
        test_repo.commit_staged("base");
        let base = test_repo.head_oid().to_string();

        test_repo.write_file("f.txt", "onto side\n");
        test_repo.stage_files(&["f.txt"]);
        test_repo.commit_staged("onto side");
        let onto = test_repo.head_oid().to_string();

        test_repo.create_branch_at("topic", &base);
        test_repo.switch_branch("topic");
        test_repo.write_file("f.txt", "topic side\n");
        test_repo.stage_files(&["f.txt"]);
        test_repo.commit_staged("topic side");
        git::run_git(&workdir, &["rebase", &onto]).unwrap_err();
        assert!(
            git::has_unmerged_paths(&workdir),
            "the rebase must conflict"
        );
        test_repo
    }

    #[test]
    fn unmerged_paths_are_conflicts_to_resolve() {
        let test_repo = repo_stopped_on_conflict();
        assert_eq!(
            pause_reason(&test_repo.workdir(), None),
            PauseReason::Conflicts
        );
    }

    /// A conflict that is already staged when the operation stops was resolved
    /// without the user — but only if it is a conflict they had not seen yet.
    #[test]
    fn a_staged_conflict_is_resolved_only_when_it_is_a_new_one() {
        let test_repo = repo_stopped_on_conflict();
        let workdir = test_repo.workdir();
        test_repo.write_file("f.txt", "resolved\n");
        git::run_git(&workdir, &["add", "f.txt"]).unwrap();

        assert_eq!(
            pause_reason(&workdir, None),
            PauseReason::ResolvedConflicts,
            "nothing was pending before, so this conflict resolved itself"
        );

        // The same conflict the caller was already on: whatever stopped the
        // step, it was not a conflict being resolved.
        let same = git::auto_merge_id(&workdir).unwrap();
        assert_eq!(
            pause_reason(&workdir, Some(&same)),
            PauseReason::Other,
            "an unchanged AUTO_MERGE must not be credited to rerere"
        );
    }

    /// The abort deletes the state file holding this patch, and a reset has just
    /// taken the working tree, so one that will not apply has to reach the user
    /// as a file instead.
    #[test]
    fn an_abort_that_cannot_restage_parks_the_patch() {
        let test_repo = crate::core::test_helpers::TestRepo::new();
        test_repo.commit("A commit", "file1.txt");
        let workdir = test_repo.workdir();

        let rollback = Rollback {
            reset_hard_to: test_repo.head_oid().to_string(),
            // Nonsense as a patch: it cannot apply, whatever the index holds.
            saved_staged_patch: "not a patch at all\n".to_string(),
            ..Default::default()
        };
        rollback.apply_abort(&workdir).unwrap();

        let parked = git::git_path(&workdir, "loom")
            .unwrap()
            .join("unrestored-staged-0.patch");
        assert_eq!(
            std::fs::read_to_string(&parked).unwrap(),
            "not a patch at all\n",
            "the state file is about to go, so this patch must not go with it"
        );
    }

    /// The working-tree half of the same rollback: `reset_hard` took the files,
    /// so its patch is the only copy of them too.
    #[test]
    fn an_abort_that_cannot_restore_the_worktree_parks_that_patch() {
        let test_repo = crate::core::test_helpers::TestRepo::new();
        test_repo.commit("A commit", "file1.txt");
        let workdir = test_repo.workdir();

        let rollback = Rollback {
            reset_hard_to: test_repo.head_oid().to_string(),
            saved_worktree_patch: "not a patch at all\n".to_string(),
            ..Default::default()
        };
        rollback.apply_abort(&workdir).unwrap();

        let parked = git::git_path(&workdir, "loom")
            .unwrap()
            .join("unrestored-0.patch");
        assert_eq!(
            std::fs::read_to_string(&parked).unwrap(),
            "not a patch at all\n",
            "the reset took these files and the state file is about to go"
        );
    }

    /// With no `reset_*` field the restore goes three-way over whatever the
    /// abort left, so a staged *new* file the autostash kept survives instead
    /// of being discarded by a reset the way it used to be.
    #[test]
    fn an_abort_without_a_reset_keeps_what_the_autostash_left_staged() {
        let test_repo = crate::core::test_helpers::TestRepo::new();
        test_repo.commit("A commit", "file1.txt");
        let workdir = test_repo.workdir();

        test_repo.write_file("file1.txt", "staged edit\n");
        test_repo.write_file("brand-new.txt", "new\n");
        test_repo.stage_files(&["file1.txt", "brand-new.txt"]);
        let patch = git::diff_cached(&workdir).unwrap();
        let before = test_repo.status_porcelain();

        // What the autostash leaves: the modification unstaged, the new file
        // still carrying its index entry.
        git::unstage_files(&workdir, &["file1.txt"]).unwrap();

        Rollback {
            saved_staged_patch: patch,
            ..Default::default()
        }
        .apply_abort(&workdir)
        .unwrap();

        assert_eq!(test_repo.status_porcelain(), before);
    }

    #[test]
    fn a_clean_stop_has_no_conflict_to_report() {
        let test_repo = crate::core::test_helpers::TestRepo::new();
        assert_eq!(pause_reason(&test_repo.workdir(), None), PauseReason::Other);
    }

    #[test]
    fn missing_state_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let result = load(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn corrupted_state_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not valid json").unwrap();
        let result = load(dir.path());
        let err = format!("{:#}", result.unwrap_err());
        assert!(err.contains("state.json"), "{err}");
        assert!(err.contains("loom abort"), "{err}");
    }

    /// The directory a save leaves behind must hold the state file and nothing
    /// else: a temp file that outlived its write would accumulate forever.
    fn loom_dir_entries(git_dir: &Path) -> Vec<String> {
        let dir = state_path(git_dir).parent().unwrap().to_path_buf();
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn save_and_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let state = LoomState {
            command: "update".to_string(),
            rollback: Rollback::default(),
            context: serde_json::Value::Null,
            protect: Vec::new(),
        };
        save(dir.path(), &state).unwrap();
        assert_eq!(
            loom_dir_entries(dir.path()),
            vec!["state.json"],
            "the temp file the write goes through must not survive it"
        );
        delete(dir.path()).unwrap();
        assert!(!state_path(dir.path()).exists());
        // Second delete is a no-op
        delete(dir.path()).unwrap();
    }

    /// `save` writes through a temp file and renames it over the real one, so
    /// it must replace a state file that is already there — and leave no temp
    /// file behind whichever way it goes.
    #[test]
    fn save_replaces_an_existing_state_without_leaving_a_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let make = |command: &str| LoomState {
            command: command.to_string(),
            rollback: Rollback::default(),
            context: serde_json::Value::Null,
            protect: Vec::new(),
        };

        save(dir.path(), &make("update")).unwrap();
        save(dir.path(), &make("commit")).unwrap();

        let loaded = load(dir.path()).unwrap().expect("state should load");
        assert_eq!(
            loaded.command, "commit",
            "the rename must replace the old state"
        );
        assert_eq!(
            loom_dir_entries(dir.path()),
            vec!["state.json"],
            "no temp file may outlive the saves"
        );
    }

    /// A temp file a killed process left behind belongs to no live save, so it
    /// must not stop a later one — and must not be mistaken for the state.
    #[test]
    fn a_stale_temp_file_does_not_break_the_next_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A previous run died between creating its temp file and renaming it.
        // The name is whatever `tempfile` picked for that process.
        let stale = path.with_file_name(".tmpAbC123");
        std::fs::write(&stale, b"truncated {\"comm").unwrap();

        save(
            dir.path(),
            &LoomState {
                command: "commit".to_string(),
                rollback: Rollback::default(),
                context: serde_json::Value::Null,
                protect: Vec::new(),
            },
        )
        .unwrap();

        assert_eq!(
            load(dir.path())
                .unwrap()
                .expect("state should load")
                .command,
            "commit"
        );
        assert!(
            state_path(dir.path()).exists(),
            "the stale file must not have been renamed over the real one"
        );
    }

    /// A truncated state file is what the temp-file-and-rename exists to
    /// prevent. If one does turn up, the error must not tell the user to delete
    /// it: the file is the only record of what `loom abort` would roll back.
    #[test]
    fn corrupt_state_error_never_suggests_deleting_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{\"command\": \"comm").unwrap();

        let err = format!("{:#}", load(dir.path()).unwrap_err());
        assert!(
            !err.to_lowercase().contains("delete"),
            "deleting the state strands the rollback it records, got: {err}"
        );
        assert!(err.contains("Move it aside"), "{err}");
    }

    /// The abort can succeed and the rollback after it still fail. The state
    /// file has to survive that: it is the only record of the half-applied undo,
    /// and the message must not blame git, which is not holding anything.
    #[test]
    fn a_failed_rollback_keeps_the_state_and_does_not_blame_git() {
        let test_repo = crate::core::test_helpers::TestRepo::new();
        test_repo.commit("first", "a.txt");
        let workdir = test_repo.workdir();
        let git_dir = test_repo.repo.path().to_path_buf();

        // No rebase is in progress, so `abort_cmd` goes straight to the
        // rollback — which cannot reset to an OID that is not in the repo.
        save(
            &git_dir,
            &LoomState {
                command: "commit".to_string(),
                rollback: Rollback {
                    reset_mixed_to: "0".repeat(40),
                    ..Default::default()
                },
                context: serde_json::Value::Null,
                protect: Vec::new(),
            },
        )
        .unwrap();

        let err = abort_cmd(&workdir, &git_dir).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("half-applied"), "{msg}");
        assert!(
            !msg.contains("index.lock"),
            "the abort worked — git is not what failed here: {msg}"
        );
        assert!(
            state_path(&git_dir).exists(),
            "the record of what is left to undo must survive"
        );
    }

    /// A rebase paused at an `edit` step is not a finished one: `continue_cmd`
    /// must keep the state file and leave the command's post-rebase work for
    /// later, however successfully `git rebase --continue` exits.
    #[test]
    fn continue_keeps_state_when_the_rebase_only_reaches_an_edit() {
        let test_repo = crate::core::test_helpers::TestRepo::new();
        let first = test_repo.commit("first", "a.txt");
        test_repo.commit("second", "b.txt");
        test_repo.commit("third", "c.txt");
        let workdir = test_repo.workdir();
        let git_dir = test_repo.repo.path().to_path_buf();

        // Every commit an `edit`, so the rebase stops twice: once now, and
        // once more when `loom continue` runs `git rebase --continue`.
        crate::git::run_git(
            &workdir,
            &[
                "-c",
                // `sed -i` is GNU-only; rewrite through a temp file instead.
                "sequence.editor=f() { sed 's/^pick/edit/' \"$1\" > \"$1.new\" && mv \"$1.new\" \"$1\"; }; f",
                "rebase",
                "-i",
                &first.to_string(),
            ],
        )
        .unwrap();

        // The context is null, which `drop::after_continue` cannot parse: if
        // the dispatch runs at all, the test fails.
        save(
            &git_dir,
            &LoomState {
                command: "drop".to_string(),
                rollback: Rollback::default(),
                context: serde_json::Value::Null,
                protect: Vec::new(),
            },
        )
        .unwrap();

        continue_cmd(&workdir, &git_dir).unwrap();

        assert!(
            crate::git::rebase_is_in_progress(&git_dir),
            "the rebase only advanced to the next `edit` step"
        );
        assert!(
            state_path(&git_dir).exists(),
            "the state must survive a rebase that is still in progress"
        );
    }
}
