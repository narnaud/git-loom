//! Worktree ref safety.
//!
//! Loom rebases move branch refs through explicit `update-ref` todo lines,
//! which bypass git's porcelain check against moving a branch that is checked
//! out in another worktree. Moving such a ref desyncs that worktree: its index
//! and files stay at the old tip, so `git status` there shows the old→new
//! delta as phantom staged changes.
//!
//! This module restores the invariant: before a rebase, branches about to be
//! rewritten are mapped to worktrees. A branch checked out in a dirty external
//! worktree refuses the whole operation up front; clean worktrees are
//! fast-forwarded to the new tip once the rebase completes. Because a rebase
//! can pause (conflict or `edit`), the planned syncs are saved to
//! `<git_dir>/loom/worktree-sync.json` and applied when the rebase machinery
//! finishes — a sync only fires if the branch ref actually moved away from the
//! recorded old tip AND the worktree still matches that old tip, so applying
//! after an abort or a worktree modified mid-pause is safe.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::core::msg;

/// A local branch checked out in a worktree.
///
/// Detached, bare, and prunable (directory gone) worktrees are excluded.
#[derive(Debug, PartialEq, Eq)]
pub struct WorktreeCheckout {
    pub path: PathBuf,
    /// Branch name without the `refs/heads/` prefix.
    pub branch: String,
}

/// List all worktrees that have a local branch checked out.
pub fn worktree_checkouts(workdir: &Path) -> Result<Vec<WorktreeCheckout>> {
    let stdout = super::run_git_stdout(workdir, &["worktree", "list", "--porcelain"])?;
    Ok(parse_worktree_list(&stdout))
}

/// Parse `git worktree list --porcelain` output.
///
/// Each worktree is a block of `attribute [value]` lines separated by a blank
/// line: `worktree <path>`, `HEAD <sha>`, then `branch refs/heads/<name>` or
/// `detached`, optionally `bare`, `locked [reason]`, `prunable [reason]`.
fn parse_worktree_list(porcelain: &str) -> Vec<WorktreeCheckout> {
    let mut result = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;
    let mut skip = false;

    // Trailing sentinel so the last block is flushed even without a blank line.
    for line in porcelain.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let (Some(p), Some(b)) = (path.take(), branch.take())
                && !skip
            {
                result.push(WorktreeCheckout { path: p, branch: b });
            }
            skip = false;
        } else if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            branch = Some(b.to_string());
        } else if line == "bare" || line.starts_with("prunable") {
            skip = true;
        }
    }
    result
}

/// Whether a worktree has local changes that a ref rewrite would desync.
///
/// Dirty means: staged or modified tracked files, unmerged entries, or an
/// operation in progress (rebase/merge/cherry-pick/revert). Untracked files
/// alone are clean — the sync refuses to overwrite them instead.
pub fn worktree_is_dirty(worktree: &Path) -> Result<bool> {
    let status = super::run_git_stdout(worktree, &["status", "--porcelain"])?;
    if status.lines().any(|line| !line.starts_with("??")) {
        return Ok(true);
    }
    let git_dir = absolute_git_dir(worktree)?;
    Ok(super::rebase_is_in_progress(&git_dir)
        || git_dir.join("MERGE_HEAD").exists()
        || git_dir.join("CHERRY_PICK_HEAD").exists()
        || git_dir.join("REVERT_HEAD").exists())
}

/// A worktree to hard-reset once its checked-out branch has been rewritten.
#[derive(Debug, Serialize, Deserialize)]
pub struct PendingSync {
    pub branch: String,
    pub path: PathBuf,
    /// The branch tip before the rewrite. A sync only fires if the branch
    /// moved away from this AND the worktree still matches it.
    pub old_tip: String,
}

/// Map branches about to be rewritten to external worktrees.
///
/// Bails if any of them is checked out in a dirty external worktree (refusing
/// the operation before anything is rewritten). Returns the sync plan for the
/// clean ones. The worktree loom itself runs in is exempt — the rebase moves
/// its ref, index, and files together.
pub fn plan_worktree_syncs(workdir: &Path, branches: &[String]) -> Result<Vec<PendingSync>> {
    if branches.is_empty() {
        return Ok(Vec::new());
    }
    let current = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf());

    let mut syncs = Vec::new();
    let mut dirty = Vec::new();
    for checkout in worktree_checkouts(workdir)? {
        if !branches.contains(&checkout.branch) {
            continue;
        }
        // A locked worktree whose directory is gone is not marked prunable;
        // treat it like a prunable one (not checked out).
        if !checkout.path.exists() {
            continue;
        }
        let canonical = checkout
            .path
            .canonicalize()
            .unwrap_or_else(|_| checkout.path.clone());
        if canonical == current {
            continue;
        }
        if worktree_is_dirty(&checkout.path)? {
            dirty.push(checkout);
        } else {
            let old_tip = super::rev_parse(workdir, &format!("refs/heads/{}", checkout.branch))?;
            syncs.push(PendingSync {
                branch: checkout.branch,
                path: checkout.path,
                old_tip,
            });
        }
    }

    match dirty.as_slice() {
        [] => Ok(syncs),
        [one] => bail!(
            "Cannot rewrite branch `{}` — it is checked out at `{}` and that worktree has local changes\n\
             Commit or stash the changes there, then retry",
            one.branch,
            one.path.display()
        ),
        many => {
            let list: Vec<String> = many
                .iter()
                .map(|c| format!("  `{}` at `{}`", c.branch, c.path.display()))
                .collect();
            bail!(
                "Cannot rewrite branches that are checked out in worktrees with local changes:\n{}\n\
                 Commit or stash the changes there, then retry",
                list.join("\n")
            )
        }
    }
}

/// Save the sync plan so it survives a paused rebase (conflict or `edit`).
///
/// An empty plan removes any stale file left by an interrupted earlier
/// operation, so the file always reflects the current one.
pub fn save_worktree_syncs(workdir: &Path, syncs: &[PendingSync]) -> Result<()> {
    let path = sync_file_path(workdir)?;
    if syncs.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create loom state directory '{}'",
                parent.display()
            )
        })?;
    }
    let json = serde_json::to_string_pretty(syncs)?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write worktree sync file '{}'", path.display()))?;
    Ok(())
}

/// Apply (and remove) the saved sync plan once no rebase is in progress.
///
/// Safe to call from any completion or abort path: each entry syncs only if
/// its branch actually moved away from the recorded old tip, so after an
/// abort (refs restored) this is a no-op that just removes the file. Never
/// fails — individual problems are reported as warnings.
pub fn finish_worktree_syncs(workdir: &Path) {
    let Ok(git_dir) = absolute_git_dir(workdir) else {
        return;
    };
    let file = sync_file_in(&git_dir);
    if !file.exists() || super::rebase_is_in_progress(&git_dir) {
        return;
    }
    let syncs: Vec<PendingSync> = match std::fs::read_to_string(&file)
        .map_err(anyhow::Error::from)
        .and_then(|json| serde_json::from_str(&json).map_err(anyhow::Error::from))
    {
        Ok(syncs) => syncs,
        Err(e) => {
            msg::warn(&format!(
                "Could not read worktree sync file '{}': {}",
                file.display(),
                e
            ));
            let _ = std::fs::remove_file(&file);
            return;
        }
    };
    let _ = std::fs::remove_file(&file);
    apply_worktree_syncs(workdir, &syncs);
}

/// Fast-forward each worktree to its branch's new tip.
fn apply_worktree_syncs(workdir: &Path, syncs: &[PendingSync]) {
    for sync in syncs {
        let Ok(new_tip) = super::rev_parse(workdir, &format!("refs/heads/{}", sync.branch)) else {
            // Branch deleted by the operation (e.g. drop) — nothing to sync.
            continue;
        };
        if new_tip == sync.old_tip || !sync.path.exists() {
            continue;
        }
        if !worktree_matches_commit(&sync.path, &sync.old_tip) {
            msg::warn(&format!(
                "Worktree `{}` was modified while branch `{}` was being rewritten — not synced\n\
                 Its index and files are still based on the old tip {} plus your changes, \
                 while HEAD now points at the rewritten branch; reconcile manually before committing there",
                sync.path.display(),
                sync.branch,
                super::short_hash(&sync.old_tip),
            ));
            continue;
        }
        // Fast-forward the checkout old→new. Unlike `reset --hard`, this
        // refuses — touching nothing — if an untracked file would be
        // overwritten by a file the rewritten branch newly tracks.
        match super::run_git(
            &sync.path,
            &["read-tree", "-m", "-u", &sync.old_tip, &new_tip],
        ) {
            Ok(()) => msg::success(&format!(
                "Synced worktree `{}` to rewritten branch `{}`",
                sync.path.display(),
                sync.branch
            )),
            Err(_) => msg::warn(&format!(
                "Worktree `{}` was not synced to rewritten branch `{}` — untracked files there would be overwritten\n\
                 Move them away, then run `git -C {} reset --hard`",
                sync.path.display(),
                sync.branch,
                sync.path.display()
            )),
        }
    }
}

/// Whether the worktree's index and files both match the given commit's tree.
/// Untracked files are ignored.
fn worktree_matches_commit(worktree: &Path, commit: &str) -> bool {
    super::run_git(worktree, &["diff", "--quiet", "--cached", commit]).is_ok()
        && super::run_git(worktree, &["diff", "--quiet", commit]).is_ok()
}

fn sync_file_path(workdir: &Path) -> Result<PathBuf> {
    Ok(sync_file_in(&absolute_git_dir(workdir)?))
}

fn sync_file_in(git_dir: &Path) -> PathBuf {
    git_dir.join("loom").join("worktree-sync.json")
}

/// Resolve the git dir, avoiding a process spawn in the common layout.
fn absolute_git_dir(workdir: &Path) -> Result<PathBuf> {
    let dot_git = workdir.join(".git");
    if dot_git.is_dir() {
        return Ok(dot_git);
    }
    let out = super::run_git_stdout(workdir, &["rev-parse", "--absolute-git-dir"])?;
    Ok(PathBuf::from(out.trim()))
}

#[cfg(test)]
#[path = "git_worktree_test.rs"]
mod tests;
