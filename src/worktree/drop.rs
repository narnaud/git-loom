use anyhow::{Context, Result, bail};
use git2::Repository;

use crate::core::{msg, repo, transaction};
use crate::git;

/// Remove a linked worktree and, when nothing but weave merges would go with
/// it, the `integration-<name>` branch it holds (Spec 022).
pub fn run(target: String) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "drop a worktree")?;
    let entry = super::resolve(workdir, &target)?;

    if entry.main {
        bail!("`{}` is the main worktree", entry.name);
    }
    if entry.current {
        bail!(
            "Cannot drop the worktree you are in\n\
             Run it from another worktree: `loom worktree drop {}`",
            entry.name
        );
    }
    if super::list::check_dirty(&entry.path)? {
        bail!(
            "Worktree `{}` has uncommitted or untracked changes\n\
             Commit, stash or remove them there, then retry",
            entry.name
        );
    }
    ensure_idle(&entry)?;

    // Counted before the removal, which leaves the branch in place but is the
    // point of no return. Any other checked-out branch is the user's own, or a
    // feature branch woven elsewhere, and always stays.
    let owned = format!("integration-{}", entry.name);
    let unique = match &entry.branch {
        Some(branch) if *branch == owned => Some(unique_commits(workdir, branch)?),
        Some(_) => None,
        None => {
            ensure_detached_head_is_held(&entry)?;
            None
        }
    };

    let path = entry.path.display().to_string();
    git::run_git(workdir, &["worktree", "remove", &path])
        .context("Failed to remove the worktree")?;
    msg::success(&format!("Removed worktree `{path}`"));

    match (&entry.branch, unique) {
        (Some(branch), Some(0)) => {
            git::run_git(workdir, &["branch", "-D", branch])
                .with_context(|| format!("Failed to delete branch `{branch}`"))?;
            msg::success(&format!("Deleted branch `{branch}`"));
        }
        (Some(branch), Some(unique)) => msg::warn(&format!(
            "Kept branch `{branch}`: {unique} of its commits are on no other branch, tag or remote\n\
             Delete it with `git branch -D {branch}` if you do not need them"
        )),
        _ => {}
    }
    Ok(())
}

/// Refuse a detached HEAD holding commits no branch, tag or remote holds: they are
/// reachable only from the worktree's HEAD and reflog, which go with its git
/// dir.
fn ensure_detached_head_is_held(entry: &super::Entry) -> Result<()> {
    let out = git::run_git_stdout(
        &entry.path,
        &[
            "rev-list",
            "--count",
            "HEAD",
            "--not",
            "--branches",
            "--tags",
            "--remotes",
        ],
    )?;
    let loose: usize = out
        .trim()
        .parse()
        .with_context(|| format!("Cannot count the commits only `{}` holds", entry.name))?;
    if loose > 0 {
        bail!(
            "Worktree `{}` has a detached HEAD with {loose} commits on no branch, tag or remote\n\
             Create a branch on them there, then retry",
            entry.name
        );
    }
    Ok(())
}

/// Refuse a worktree with a paused loom operation, rebase or merge: removing
/// it would throw away the operation's state along with its git dir.
fn ensure_idle(entry: &super::Entry) -> Result<()> {
    let wt_repo = Repository::open(&entry.path)?;
    let git_dir = wt_repo.path();
    let paused = transaction::load(git_dir)?.is_some()
        || git::rebase_is_in_progress(git_dir)
        || git::merge_is_in_progress(git_dir);
    if paused {
        bail!(
            "Worktree `{}` has an operation in progress\n\
             Finish it there with `loom continue` or `loom abort`, then retry",
            entry.name
        );
    }
    Ok(())
}

/// Non-merge commits of `branch` that no other local branch, tag or
/// remote-tracking ref holds: what deleting it would lose.
fn unique_commits(workdir: &std::path::Path, branch: &str) -> Result<usize> {
    let exclude = format!("--exclude={branch}");
    let tip = format!("refs/heads/{branch}");
    let out = git::run_git_stdout(
        workdir,
        &[
            "rev-list",
            "--count",
            "--no-merges",
            &tip,
            "--not",
            &exclude,
            "--branches",
            "--tags",
            "--remotes",
        ],
    )?;
    // Never default to 0 on a parse failure: 0 deletes the branch.
    out.trim()
        .parse()
        .with_context(|| format!("Cannot count the commits only `{branch}` holds"))
}

#[cfg(test)]
#[path = "drop_test.rs"]
mod tests;
