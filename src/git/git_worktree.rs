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

/// One entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Branch name without the `refs/heads/` prefix; `None` when detached or bare.
    pub branch: Option<String>,
    pub bare: bool,
    /// Registered, but its directory is gone.
    pub prunable: bool,
}

/// A local branch checked out in a worktree.
///
/// Detached, bare, and prunable (directory gone) worktrees are excluded.
#[derive(Debug, PartialEq, Eq)]
pub struct WorktreeCheckout {
    pub path: PathBuf,
    /// Branch name without the `refs/heads/` prefix.
    pub branch: String,
}

/// Every registered worktree, the main one first, as git lists them.
pub fn list_worktrees(workdir: &Path) -> Result<Vec<Worktree>> {
    let stdout = super::run_git_stdout(workdir, &["worktree", "list", "--porcelain"])?;
    Ok(parse_worktrees(&stdout))
}

/// List all worktrees that have a local branch checked out.
pub fn worktree_checkouts(workdir: &Path) -> Result<Vec<WorktreeCheckout>> {
    Ok(checkouts(list_worktrees(workdir)?))
}

fn checkouts(worktrees: Vec<Worktree>) -> Vec<WorktreeCheckout> {
    worktrees
        .into_iter()
        .filter(|w| !w.bare && !w.prunable)
        .filter_map(|w| {
            Some(WorktreeCheckout {
                path: w.path,
                branch: w.branch?,
            })
        })
        .collect()
}

/// Parse `git worktree list --porcelain` output.
///
/// Each worktree is a block of `attribute [value]` lines separated by a blank
/// line: `worktree <path>`, `HEAD <sha>`, then `branch refs/heads/<name>` or
/// `detached`, optionally `bare`, `locked [reason]`, `prunable [reason]`.
fn parse_worktrees(porcelain: &str) -> Vec<Worktree> {
    let mut result = Vec::new();
    let mut current: Option<Worktree> = None;

    // Trailing sentinel so the last block is flushed even without a blank line.
    for line in porcelain.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            result.extend(current.take());
        } else if let Some(p) = line.strip_prefix("worktree ") {
            current = Some(Worktree {
                path: PathBuf::from(p),
                branch: None,
                bare: false,
                prunable: false,
            });
        } else if let Some(w) = current.as_mut() {
            if let Some(b) = line.strip_prefix("branch refs/heads/") {
                w.branch = Some(b.to_string());
            } else if line == "bare" {
                w.bare = true;
            } else if line.starts_with("prunable") {
                w.prunable = true;
            }
        }
    }
    result
}

#[cfg(test)]
fn parse_worktree_list(porcelain: &str) -> Vec<WorktreeCheckout> {
    checkouts(parse_worktrees(porcelain))
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
    // git prints the main worktree at its git dir minus a trailing `/.git` —
    // the checkout itself only when the git dir sits inside it (Spec 004).
    // Never the common dir: from a linked worktree that would exempt the main
    // worktree loom is not standing in.
    let mut listed_as = super::absolute_git_dir(workdir)?;
    if listed_as.file_name().is_some_and(|name| name == ".git") {
        listed_as.pop();
    }
    let listed_as = listed_as.canonicalize().unwrap_or(listed_as);

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
        if canonical == current || canonical == listed_as {
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
