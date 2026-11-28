use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::git;

/// A command `loom continue` can resume, named in the state file in lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resumable {
    Update,
    Commit,
    Absorb,
    Drop,
    Fold,
    Reword,
    Swap,
    Merge,
}

impl std::fmt::Display for Resumable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Resumable::Update => "update",
            Resumable::Commit => "commit",
            Resumable::Absorb => "absorb",
            Resumable::Drop => "drop",
            Resumable::Fold => "fold",
            Resumable::Reword => "reword",
            Resumable::Swap => "swap",
            Resumable::Merge => "merge",
        })
    }
}

/// Persistent state of a resumable loom command: saved when it pauses, or, for
/// `merge`, before it starts (Spec 005).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoomState {
    /// The interrupted command.
    pub command: Resumable,
    /// Shared rollback information for `loom abort`.
    pub rollback: Rollback,
    /// Commits `loom continue` must not let the rebase drop as empty, because
    /// a ref this command reads back follows them (see
    /// [`crate::core::weave::run_rebase_protecting`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protect: Vec<String>,
    /// Protected commits the operation lands on (a fold or absorb target), so
    /// the refusal does not offer to drop them (see [`git::Protected`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    /// Fixup commits whose squash the user's resolution skipped: once the
    /// rebase is over, each one's change goes back to the working tree, and
    /// the command says what was not folded (Spec 014).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_fixups: Vec<String>,
    /// Command-specific resume context (opaque JSON).
    pub context: serde_json::Value,
}

impl LoomState {
    /// `protect` and `targets`, as the rebase takes them.
    pub fn protected(&self) -> git::Protected<'_> {
        git::Protected::named(&self.protect).targeting(&self.targets)
    }
}

/// Rollback information captured before the rebase step starts.
///
/// Only fields [`Rollback::apply_abort`] consumes belong here. `git rebase --abort` already restores HEAD, all branch refs
/// (via `--update-refs`), and autostashed working-tree changes — so those do
/// not need to be saved.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
        // On disk before the caller acts on it: `merge` resets the tree right
        // after, leaving this file the only copy of the user's work.
        tmp.as_file().sync_all()?;
        tmp.persist(&path)?;
        // Best-effort: Windows cannot open a directory as a file.
        let _ = std::fs::File::open(parent).and_then(|d| d.sync_all());
        Ok(())
    })()
    .with_context(|| format!("Failed to write state file '{}'", path.display()))
}

/// Load the state file. Returns `None` if the file does not exist.
///
/// Errors carry their cause in the outermost message, the only one `cli.rs`
/// prints. No `exists()` check first: a file removed in between, by a
/// `loom continue` finishing elsewhere, would read as unreadable.
pub fn load(git_dir: &Path) -> Result<Option<LoomState>> {
    let path = state_path(git_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        // Possibly a valid file held by another process: moving it aside
        // would strand its rollback, so only a parse failure suggests that.
        Err(e) => bail!(
            "State file '{}' cannot be read: {e}\n\
             Fix what keeps loom from reading it, then retry",
            path.display()
        ),
    };
    let state = serde_json::from_slice(&bytes).map_err(|e| {
        anyhow::anyhow!(match unknown_command(&bytes) {
            Some(command) => unknown_command_message(git_dir, &path, &command),
            None => corrupt_state_message(git_dir, &path, &e),
        })
    })?;
    Ok(Some(state))
}

/// The command the state file names, if this loom does not resume it.
fn unknown_command(bytes: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct Named {
        command: String,
    }
    let name = serde_json::from_slice::<Named>(bytes).ok()?.command;
    serde_json::from_value::<Resumable>(serde_json::Value::String(name.clone()))
        .is_err()
        .then_some(name)
}

/// Error for a state file naming a command this loom does not resume: another
/// loom version wrote it, and this one cannot vouch for that command's undo,
/// so `loom abort` refuses it too.
fn unknown_command_message(git_dir: &Path, path: &Path, command: &str) -> String {
    format!(
        "State file '{}' is for a `loom {command}`, which this version of loom cannot resume or undo\n\
         Run `loom continue` or `loom abort` with the version that started it; without that version:\n\
         {}",
        path.display(),
        move_aside_advice(git_dir)
    )
}

/// Error for a state file that does not parse.
fn corrupt_state_message(git_dir: &Path, path: &Path, cause: &serde_json::Error) -> String {
    format!(
        "State file '{}' is corrupted or invalid ({cause})\n{}",
        path.display(),
        move_aside_advice(git_dir)
    )
}

/// How to get past a state file loom cannot use. Never suggests deleting it:
/// without it `loom abort` runs `git rebase --abort` and nothing else,
/// stranding a temp branch, a pre-rebase commit and a saved staged patch.
fn move_aside_advice(git_dir: &Path) -> String {
    let next = if git::rebase_is_in_progress(git_dir) || git::merge_is_in_progress(git_dir) {
        "Then run `loom abort` to cancel what git still has in progress"
    } else {
        "Git has nothing in progress, so there is nothing for `loom abort` to cancel"
    };
    format!(
        "Move it aside and keep the copy: it is the only record of what `loom abort` would undo\n\
         What loom set aside for the operation, such as staged changes, comes back only by hand from it\n\
         {next}"
    )
}

/// Refuse a command while a loom operation is paused or its state file is
/// unreadable, which the command would save over, or while git has a rebase
/// or merge in progress, which it would run into (Spec 014).
pub fn refuse_if_paused(git_dir: &Path) -> Result<()> {
    let rebasing = git::rebase_is_in_progress(git_dir);
    let interrupted = rebasing || git::merge_is_in_progress(git_dir);
    if let Some(state) = load(git_dir)? {
        bail!(paused_state_message(state.command, interrupted));
    }
    if interrupted {
        let step = if rebasing {
            git::rebase_progress(git_dir)
        } else {
            None
        };
        bail!(stray_rebase_message(rebasing, step));
    }
    Ok(())
}

/// Message shown when a command is blocked by a paused loom operation.
///
/// `interrupted` tells whether git still has a rebase or merge going: if not,
/// the user likely finished it by hand, and `loom continue` only has the
/// post-rebase bookkeeping left to do.
fn paused_state_message(command: Resumable, interrupted: bool) -> String {
    // Paused with no git operation behind it: on its own conflict with the
    // set-aside work, or merged by hand (Spec 005).
    if command == Resumable::Merge && !interrupted {
        return "A `loom merge` is paused.\n\
                Resolve any conflicts and stage them, then run `loom continue`, or \
                `loom abort` to return to before the merge."
            .to_string();
    }
    if interrupted {
        format!(
            "A `loom {command}` is paused, with a rebase or merge still in progress.\n\
             Resolve what stopped it, then run `loom continue` to resume, or `loom abort` to cancel."
        )
    } else {
        format!(
            "A `loom {command}` is paused, but no rebase is in progress.\n\
             If you finished it yourself, run `loom continue` to wrap up and clear the state.\n\
             Run `loom abort` to discard it instead."
        )
    }
}

/// Message shown when git has a rebase or merge in progress that no loom state
/// file describes — an operation that failed before it could save state, or one
/// the user started with raw git.
fn stray_rebase_message(rebasing: bool, step: Option<(usize, usize)>) -> String {
    let what = if rebasing { "rebase" } else { "merge" };
    let progress = match step {
        Some((current, total)) => format!(" (stopped at step {current}/{total})"),
        None => String::new(),
    };
    format!(
        "A git {what} is in progress{progress}, but no loom operation is recorded.\n\
         Resolve any conflicts and run `loom continue` to finish it, or `loom abort` to cancel it."
    )
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
pub enum PauseReason {
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
pub fn pause_reason(workdir: &Path, before: Option<&str>) -> PauseReason {
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
pub fn warn_paused(workdir: &Path, command: Resumable) {
    git::discard_mtimes();
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
pub fn warn_paused_at_edit(command: Option<Resumable>) {
    git::discard_mtimes();
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

/// Undo a saved operation whose rebase failed, and remove its state file.
///
/// The rebase is normally over by the time this runs, so what is left is
/// `loom abort`'s half of the undo; the state goes with it, or `cli.rs`
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
            empty_replay_rolled_back(cause, &sha, state.command)
        }
        _ => cause,
    }
}

/// The empty-replay refusal for an operation whose undo removes the commit.
///
/// Built on `cause` so the `ReplayedEmpty` marker stays classifiable above.
fn empty_replay_rolled_back(cause: anyhow::Error, sha: &str, command: Resumable) -> anyhow::Error {
    let short = git::short_hash(sha);
    cause.context(format!(
        "Commit `{short}` {}\n\
         The `loom {command}` was rolled back, so there is nothing left to drop",
        git::REPLAYS_EMPTY
    ))
}

/// What to say when the abort worked but the rollback after it did not. Blaming
/// git would be wrong here — git is free, and the undo is half-applied.
pub const ROLLBACK_FAILED_HINT: &str =
    "the rollback is only half-applied — the loom state was kept so `loom abort` can finish it";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip() {
        let state = LoomState {
            command: Resumable::Commit,
            rollback: Rollback {
                reset_mixed_to: "abc123".to_string(),
                delete_branches: vec!["new-branch".to_string()],
                saved_staged_patch: "--- a/foo\n+++ b/foo\n".to_string(),
                ..Default::default()
            },
            context: serde_json::json!({ "branch_name": "feature" }),
            protect: vec!["def456".to_string()],
            targets: vec!["fed789".to_string()],
            skipped_fixups: Vec::new(),
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        let restored: LoomState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.command, Resumable::Commit);
        assert_eq!(restored.rollback.reset_mixed_to, "abc123");
        assert_eq!(restored.rollback.delete_branches, vec!["new-branch"]);
        assert_eq!(restored.protect, vec!["def456"]);
        assert_eq!(restored.targets, vec!["fed789"]);
    }

    /// State files on disk name the command this way, so they must keep
    /// loading.
    #[test]
    fn a_command_is_named_in_lowercase() {
        for (command, name) in [
            (Resumable::Update, "update"),
            (Resumable::Commit, "commit"),
            (Resumable::Absorb, "absorb"),
            (Resumable::Drop, "drop"),
            (Resumable::Fold, "fold"),
            (Resumable::Reword, "reword"),
            (Resumable::Swap, "swap"),
            (Resumable::Merge, "merge"),
        ] {
            assert_eq!(serde_json::to_value(command).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<Resumable>(name.into()).unwrap(),
                command
            );
            assert_eq!(command.to_string(), name);
        }
    }

    /// Refused on load, before `loom continue` could reach its dispatch; the
    /// file is not corrupted, so the error must not say it is.
    #[test]
    fn a_state_naming_an_unknown_command_does_not_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let load_with = |json: &[u8]| {
            std::fs::write(&path, json).unwrap();
            load(dir.path()).unwrap_err().to_string()
        };

        let unknown = br#"{"command":"frobnicate","rollback":{},"context":null}"#;
        let err = load_with(unknown);
        assert!(err.contains("`loom frobnicate`"), "{err}");
        assert!(err.contains("version that started it"), "{err}");
        assert!(err.contains("Move it aside"), "{err}");
        assert!(err.contains("only by hand"), "{err}");
        assert!(err.contains("nothing for `loom abort` to cancel"), "{err}");
        assert!(!err.contains("corrupted"), "{err}");

        std::fs::create_dir(dir.path().join("rebase-merge")).unwrap();
        let rebasing = load_with(unknown);
        assert!(rebasing.contains("Then run `loom abort`"), "{rebasing}");
        std::fs::remove_dir(dir.path().join("rebase-merge")).unwrap();

        let case = load_with(br#"{"command":"Fold","rollback":{},"context":null}"#);
        assert!(case.contains("`loom Fold`"), "{case}");

        // A known command with a broken field, or a file that is not JSON,
        // is corrupted whatever it names.
        let known = load_with(br#"{"command":"fold","rollback":"x","context":null}"#);
        assert!(known.contains("corrupted or invalid"), "{known}");
        let truncated = load_with(br#"{"command":"frobnicate","rollb"#);
        assert!(truncated.contains("corrupted or invalid"), "{truncated}");
        let not_a_name = load_with(br#"{"command":7,"rollback":{},"context":null}"#);
        assert!(not_a_name.contains("corrupted or invalid"), "{not_a_name}");
    }

    /// A state file written before `protect` existed must still load.
    #[test]
    fn state_without_protect_loads() {
        let json = r#"{"command":"fold","rollback":{},"context":null}"#;
        let restored: LoomState = serde_json::from_str(json).unwrap();
        assert!(restored.protect.is_empty());
        assert!(restored.targets.is_empty());
    }

    /// Rolling back on top of a live rebase would make the mess worse, so the
    /// state has to survive for `loom abort`.
    #[test]
    fn a_failed_abort_keeps_the_state_for_loom_abort() {
        let t = repo_stopped_on_conflict();
        let workdir = t.workdir();
        let git_dir = t.repo.path().to_path_buf();
        let state = LoomState {
            command: Resumable::Fold,
            rollback: Rollback {
                delete_branches: vec!["topic".to_string()],
                ..Default::default()
            },
            context: serde_json::Value::Null,
            protect: Vec::new(),
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
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
            command: Resumable::Fold,
            rollback: Rollback {
                saved_staged_patch: staged.clone(),
                ..Default::default()
            },
            context: serde_json::Value::Null,
            protect: Vec::new(),
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
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
            command: Resumable::Commit,
            rollback: Rollback {
                reset_mixed_to: "0".repeat(40),
                ..Default::default()
            },
            context: serde_json::Value::Null,
            protect: Vec::new(),
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
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
        let idle = load(dir.path()).unwrap_err().to_string();
        assert!(idle.contains("state.json"), "{idle}");
        assert!(idle.contains("line 1 column"), "{idle}");
        assert!(idle.contains("nothing in progress"), "{idle}");

        std::fs::create_dir(dir.path().join("rebase-merge")).unwrap();
        let rebasing = load(dir.path()).unwrap_err().to_string();
        assert!(rebasing.contains("Then run `loom abort`"), "{rebasing}");

        std::fs::write(&path, b"{\"command\":\"\xff\"}").unwrap();
        let not_utf8 = load(dir.path()).unwrap_err().to_string();
        assert!(not_utf8.contains("corrupted or invalid"), "{not_utf8}");
    }

    /// A file that cannot be read may be valid and only held elsewhere:
    /// moving it aside would strand its rollback.
    #[test]
    fn unreadable_state_names_the_cause_without_moving_it_aside() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state_path(dir.path())).unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("cannot be read: "), "{err}");
        assert!(!err.contains("Move it aside"), "{err}");
    }

    #[test]
    fn refuse_if_paused_keeps_a_corrupt_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{\"command\":").unwrap();
        let err = refuse_if_paused(dir.path()).unwrap_err().to_string();
        assert!(err.contains("corrupted or invalid"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"command\":");
    }

    #[test]
    fn stray_rebase_message_names_the_step_and_both_ways_out() {
        let msg = stray_rebase_message(true, Some((32, 35)));
        assert!(msg.contains("rebase is in progress"), "{msg}");
        assert!(msg.contains("step 32/35"), "{msg}");
        assert!(msg.contains("loom continue"), "{msg}");
        assert!(msg.contains("loom abort"), "{msg}");

        let merge = stray_rebase_message(false, None);
        assert!(merge.contains("merge is in progress"), "{merge}");
        assert!(!merge.contains("step"), "{merge}");
    }

    #[test]
    fn paused_message_points_at_continue_and_abort() {
        let paused = paused_state_message(Resumable::Update, true);
        assert!(paused.contains("still in progress"), "{paused}");
        assert!(paused.contains("loom continue"), "{paused}");
        assert!(paused.contains("loom abort"), "{paused}");

        let finished = paused_state_message(Resumable::Update, false);
        assert!(finished.contains("no rebase is in progress"), "{finished}");
        assert!(finished.contains("loom continue"), "{finished}");
        assert!(finished.contains("loom abort"), "{finished}");
        assert_ne!(paused, finished);
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
            command: Resumable::Update,
            rollback: Rollback::default(),
            context: serde_json::Value::Null,
            protect: Vec::new(),
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
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
        let make = |command| LoomState {
            command,
            rollback: Rollback::default(),
            context: serde_json::Value::Null,
            protect: Vec::new(),
            targets: Vec::new(),
            skipped_fixups: Vec::new(),
        };

        save(dir.path(), &make(Resumable::Update)).unwrap();
        save(dir.path(), &make(Resumable::Commit)).unwrap();

        let loaded = load(dir.path()).unwrap().expect("state should load");
        assert_eq!(
            loaded.command,
            Resumable::Commit,
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
                command: Resumable::Commit,
                rollback: Rollback::default(),
                context: serde_json::Value::Null,
                protect: Vec::new(),
                targets: Vec::new(),
                skipped_fixups: Vec::new(),
            },
        )
        .unwrap();

        assert_eq!(
            load(dir.path())
                .unwrap()
                .expect("state should load")
                .command,
            Resumable::Commit
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
}
