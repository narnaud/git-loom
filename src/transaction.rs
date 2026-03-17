use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::git;
use crate::git_commands::{self, git_commit, git_rebase};

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
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Rollback {
    /// HEAD OID before the operation started.
    pub saved_head: String,
    /// Snapshot of all branch ref OIDs before the operation.
    pub saved_refs: HashMap<String, String>,
    /// Branches created during this operation that should be deleted on abort.
    pub delete_branches: Vec<String>,
    /// Staged diff saved aside during the operation (may be empty).
    pub saved_staged_patch: String,
    /// Full working-tree diff saved before the rebase (may be empty).
    pub saved_worktree_patch: String,
    /// If true, use `git reset --mixed` instead of `--hard` when restoring HEAD.
    /// Set for commands (e.g., `commit`) where the committed content should
    /// remain in the working directory as unstaged changes after abort.
    #[serde(default)]
    pub reset_mixed: bool,
}

/// Convert a `snapshot_branch_refs` result to the string map stored in `Rollback`.
pub fn refs_to_strings(snapshot: &HashMap<String, git2::Oid>) -> HashMap<String, String> {
    snapshot
        .iter()
        .map(|(k, v)| (k.clone(), v.to_string()))
        .collect()
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

/// Apply shared rollback: restore HEAD, branch refs, and patches.
pub fn apply_rollback(workdir: &Path, rollback: &Rollback) -> Result<()> {
    if !rollback.saved_head.is_empty() {
        if rollback.reset_mixed {
            let _ = git_commit::reset_mixed(workdir, &rollback.saved_head);
        } else {
            let _ = git_commit::reset_hard(workdir, &rollback.saved_head);
        }
    }

    // Restore branch refs
    let oid_map: HashMap<String, git2::Oid> = rollback
        .saved_refs
        .iter()
        .filter_map(|(name, oid_str)| {
            git2::Oid::from_str(oid_str)
                .ok()
                .map(|oid| (name.clone(), oid))
        })
        .collect();
    if !oid_map.is_empty() {
        let _ = git::restore_branch_refs(workdir, &oid_map);
    }

    // Delete newly created branches
    for branch in &rollback.delete_branches {
        let _ = git_commands::git_branch::delete(workdir, branch);
    }

    // Restore pre-existing staged changes
    if !rollback.saved_staged_patch.is_empty() {
        let _ = git_commands::apply_cached_patch(workdir, &rollback.saved_staged_patch);
    }

    // Restore working-tree changes
    if !rollback.saved_worktree_patch.is_empty() {
        let _ = git_commands::apply_patch(workdir, &rollback.saved_worktree_patch);
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
/// 1. Aborts any active rebase.
/// 2. Applies shared rollback.
/// 3. Deletes state.
pub fn abort_cmd(workdir: &Path, git_dir: &Path) -> Result<()> {
    let state = load_required(git_dir)?;

    if git_rebase::is_in_progress(git_dir) {
        let _ = git_rebase::abort(workdir);
    }

    apply_rollback(workdir, &state.rollback)?;
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
                saved_head: "abc123".to_string(),
                saved_refs: [("feature".to_string(), "def456".to_string())]
                    .into_iter()
                    .collect(),
                delete_branches: vec!["new-branch".to_string()],
                saved_staged_patch: "--- a/foo\n+++ b/foo\n".to_string(),
                saved_worktree_patch: String::new(),
                reset_mixed: false,
            },
            context: serde_json::json!({ "branch_name": "feature" }),
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        let restored: LoomState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.command, "commit");
        assert_eq!(restored.rollback.saved_head, "abc123");
        assert_eq!(
            restored.rollback.saved_refs.get("feature"),
            Some(&"def456".to_string())
        );
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
