//! Worktree ref safety.
//!
//! A weave rebase moves branch refs two ways: the explicit `update-ref` todo
//! lines move the feature branches, and completing the rebase moves HEAD's own
//! branch. Neither goes through git's porcelain check against moving a branch
//! that is checked out in another worktree, and moving such a ref desyncs that
//! worktree: its index and files stay at the old tip, so `git status` there
//! reports the old→new delta as phantom staged changes.
//!
//! Loom therefore applies git's own rule itself, before the rebase starts.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

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

/// Refuse the operation if any of `branches` is checked out in another
/// worktree, before anything has been rewritten.
///
/// The worktree loom runs in is exempt: the rebase moves its ref, index and
/// files together.
pub fn ensure_not_checked_out_elsewhere(workdir: &Path, branches: &[String]) -> Result<()> {
    if branches.is_empty() {
        return Ok(());
    }
    let current = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf());

    let mut blocked = Vec::new();
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
        blocked.push(checkout);
    }

    match blocked.as_slice() {
        [] => Ok(()),
        [one] => bail!(
            "Cannot rewrite branch `{}` — it is checked out at `{}`\n\
             That worktree's index and files would stay at the old tip. Check out \
             another branch there (or remove the worktree), then retry",
            one.branch,
            one.path.display()
        ),
        many => {
            let list: Vec<String> = many
                .iter()
                .map(|c| format!("  `{}` at `{}`", c.branch, c.path.display()))
                .collect();
            bail!(
                "Cannot rewrite branches that are checked out in other worktrees:\n{}\n\
                 Their indexes and files would stay at the old tips. Check out other \
                 branches there (or remove the worktrees), then retry",
                list.join("\n")
            )
        }
    }
}

#[cfg(test)]
#[path = "git_worktree_test.rs"]
mod tests;
