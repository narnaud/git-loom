use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::git_commands::{self, git_branch, git_commit, git_rebase};

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
    /// - `delete_branches` → delete temporary branches
    /// - `saved_staged_patch` → re-stage saved changes
    /// - `saved_worktree_patch` → re-apply saved working-tree changes
    pub fn apply_abort(&self, workdir: &Path) -> Result<()> {
        if !self.reset_mixed_to.is_empty() {
            git_commit::reset_mixed(workdir, &self.reset_mixed_to)?;
        }
        for branch in &self.delete_branches {
            let _ = git_branch::delete(workdir, branch);
        }
        git_commands::restore_staged_patch(workdir, &self.saved_staged_patch)?;
        if !self.saved_worktree_patch.is_empty()
            && let Err(e) = git_commands::apply_patch(workdir, &self.saved_worktree_patch)
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create loom state directory '{}'",
                parent.display()
            )
        })?;
    }
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write state file '{}'", path.display()))?;
    Ok(())
}

/// Load the state file. Returns `None` if the file does not exist.
pub fn load(git_dir: &Path) -> Result<Option<LoomState>> {
    let path = state_path(git_dir);
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read state file '{}'", path.display()))?;
    let state: LoomState = serde_json::from_str(&json)
        .with_context(|| format!("State file '{}' is corrupted or invalid", path.display()))?;
    Ok(Some(state))
}

/// Load the state file, erroring if it does not exist.
pub fn load_required(git_dir: &Path) -> Result<LoomState> {
    load(git_dir)?.with_context(|| "No loom operation is in progress".to_string())
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

/// Emit the standard conflict-pause warning for a resumable command.
pub fn warn_conflict_paused(command: &str) {
    crate::msg::warn(&format!(
        "Conflicts detected — resolve them with git, then run:\n\
         `loom continue`   to complete the {}\n\
         `loom abort`      to cancel and restore original state",
        command
    ));
}

/// Implement `loom continue`.
///
/// 1. If a rebase is still active, runs `git rebase --continue`.
/// 2. If `--continue` produces another conflict, keeps the state and reports paused.
/// 3. Otherwise dispatches to the command-specific `after_continue` handler.
/// 4. Deletes state only after dispatch succeeds.
pub fn continue_cmd(workdir: &Path, git_dir: &Path) -> Result<()> {
    let state = load_required(git_dir)?;

    if git_rebase::is_in_progress(git_dir) {
        match git_rebase::continue_rebase(workdir)? {
            git_rebase::RebaseOutcome::Conflicted => {
                crate::msg::warn("Conflicts remain — resolve them and run `loom continue` again");
                return Ok(());
            }
            git_rebase::RebaseOutcome::Completed => {}
        }
    }

    dispatch_after_continue(workdir, &state)?;
    delete(git_dir)?;
    Ok(())
}

/// Implement `loom abort`.
///
/// 1. Aborts any active rebase (`git rebase --abort` restores HEAD, branch
///    refs via `--update-refs`, and any autostashed working-tree changes).
/// 2. Calls `rollback.apply_abort()` for any cleanup `git rebase --abort`
///    cannot do on its own (un-committing staged changes, deleting temp branches,
///    restoring saved patches).
/// 3. Deletes state.
pub fn abort_cmd(workdir: &Path, git_dir: &Path) -> Result<()> {
    let state = load_required(git_dir)?;

    if git_rebase::is_in_progress(git_dir) {
        let _ = git_rebase::abort(workdir);
    }

    state.rollback.apply_abort(workdir)?;
    delete(git_dir)?;

    crate::msg::success(&format!(
        "Aborted `loom {}` and restored original state",
        state.command
    ));
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
        "swap" => crate::swap::after_continue(workdir, &state.context),
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
    fn load_required_errors_on_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_required(dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No loom operation is in progress")
        );
    }

    #[test]
    fn corrupted_state_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = state_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not valid json").unwrap();
        let result = load(dir.path());
        assert!(result.is_err());
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
        assert!(state_path(dir.path()).exists());
        delete(dir.path()).unwrap();
        assert!(!state_path(dir.path()).exists());
        // Second delete is a no-op
        delete(dir.path()).unwrap();
    }
}
