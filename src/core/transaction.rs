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
    /// Apply the rollback after `git rebase --abort` has run.
    ///
    /// Acts on whichever fields are populated:
    /// - `reset_mixed_to` → `reset --mixed` to undo a pre-rebase commit
    /// - `reset_hard_to` → `reset --hard` to undo pre-rebase commits (e.g. fixup commits)
    /// - `delete_branches` → delete temporary branches
    /// - `saved_staged_patch` → re-stage saved changes
    /// - `saved_worktree_patch` → re-apply saved working-tree changes
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
        git::restore_staged_patch(workdir, &self.saved_staged_patch)?;
        if !self.saved_worktree_patch.is_empty()
            && let Err(e) = git::apply_patch(workdir, &self.saved_worktree_patch)
        {
            eprintln!("Warning: could not re-apply working-tree changes: {}", e);
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
    // file that both `loom continue` and `loom abort` would refuse to read.
    // The temp file has a random name and is removed on drop, so a crash
    // between the two steps litters nothing and two loom processes saving at
    // once cannot overwrite each other's.
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

/// Emit the pause warning for a resumable command whose rebase stopped.
///
/// A conflict is the usual reason, but not the only one (an untracked file in
/// the way, a stale `index.lock`), so the message follows what the index
/// actually says.
pub fn warn_conflict_paused(workdir: &Path, command: &str) {
    if !git::has_unmerged_paths(workdir) {
        crate::core::agent_mode::note_paused(
            &format!(
                "The `loom {}` is paused — the rebase stopped part-way",
                command
            ),
            "run loom trace to see why, fix it, then run: loom continue (or loom abort)",
        );
        crate::core::msg::warn_reported(&format!(
            "The rebase stopped part-way — run `loom trace` to see why, then:\n\
             `loom continue`   to complete the {}\n\
             `loom abort`      to cancel and restore original state",
            command
        ));
        return;
    }

    crate::core::agent_mode::note_paused(
        &format!("Conflicts detected — the `loom {}` is paused", command),
        "resolve conflicts, stage them, then run: loom continue (or loom abort)",
    );
    crate::core::msg::warn_reported(&format!(
        "Conflicts detected — resolve them with git, then run:\n\
         `loom continue`   to complete the {}\n\
         `loom abort`      to cancel and restore original state",
        command
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
fn warn_still_paused(workdir: &Path, subject: &str) {
    if !git::has_unmerged_paths(workdir) {
        crate::core::agent_mode::note_paused(
            &format!("The {} stopped again — it is still paused", subject),
            "run loom trace to see why, fix it, then run: loom continue (or loom abort)",
        );
        crate::core::msg::warn_reported(&format!(
            "The {} stopped again — run `loom trace` to see why, then `loom continue`",
            subject
        ));
        return;
    }

    crate::core::agent_mode::note_paused(
        &format!("Conflicts remain — the {} is still paused", subject),
        "resolve conflicts, stage them, then run: loom continue (or loom abort)",
    );
    crate::core::msg::warn_reported(
        "Conflicts remain — resolve them and run `loom continue` again",
    );
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

/// Implement `loom continue`.
///
/// 1. If a rebase is still active, runs `git rebase --continue`.
/// 2. If `--continue` produces another conflict, keeps the state and reports paused.
/// 3. Otherwise dispatches to the command-specific `after_continue` handler.
/// 4. Deletes state only after dispatch succeeds.
pub fn continue_cmd(workdir: &Path, git_dir: &Path) -> Result<()> {
    let Some(state) = load(git_dir)? else {
        return continue_without_state(workdir, git_dir);
    };

    if git::rebase_is_in_progress(git_dir) {
        match git::continue_rebase(workdir)? {
            git::RebaseOutcome::Paused => {
                warn_paused_at_edit(Some(&state.command));
                return Ok(());
            }
            git::RebaseOutcome::Stopped => {
                warn_still_paused(workdir, "operation");
                return Ok(());
            }
            git::RebaseOutcome::Completed => {}
        }
    } else if git::merge_is_in_progress(git_dir) {
        match git::continue_merge(workdir, git_dir)? {
            git::MergeOutcome::Stopped => {
                warn_still_paused(workdir, "operation");
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

/// Implement `loom abort`.
///
/// 1. Aborts any active rebase (`git rebase --abort` restores HEAD, branch
///    refs via `--update-refs`, and any autostashed working-tree changes).
///    If that fails, stops there and keeps the state file.
/// 2. Calls `rollback.apply_abort()` for any cleanup `git rebase --abort`
///    cannot do on its own (un-committing staged changes, deleting temp branches,
///    restoring saved patches).
/// 3. Deletes state.
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
    if git::rebase_is_in_progress(git_dir) {
        match git::continue_rebase(workdir)? {
            git::RebaseOutcome::Paused => warn_paused_at_edit(None),
            git::RebaseOutcome::Stopped => warn_still_paused(workdir, GitOp::Rebase.as_str()),
            git::RebaseOutcome::Completed => report_stateless_continue(GitOp::Rebase),
        }
    } else if git::merge_is_in_progress(git_dir) {
        match git::continue_merge(workdir, git_dir)? {
            git::MergeOutcome::Stopped => warn_still_paused(workdir, GitOp::Merge.as_str()),
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

/// Dispatch to the command-specific `after_continue` handler.
fn dispatch_after_continue(workdir: &Path, state: &LoomState) -> Result<()> {
    match state.command.as_str() {
        "update" => crate::update::after_continue(workdir, &state.context),
        "commit" => crate::commit::after_continue(workdir, &state.rollback, &state.context),
        "absorb" => crate::absorb::after_continue(workdir, &state.rollback, &state.context),
        "drop" => crate::drop::after_continue(workdir, &state.context),
        "fold" => crate::fold::after_continue(workdir, &state.context),
        "reword" => crate::reword::after_continue(workdir, &state.context),
        "swap" => crate::swap::after_continue(workdir, &state.context),
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
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        let restored: LoomState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.command, "commit");
        assert_eq!(restored.rollback.reset_mixed_to, "abc123");
        assert_eq!(restored.rollback.delete_branches, vec!["new-branch"]);
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
