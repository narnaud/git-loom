use anyhow::{Result, bail};
use git2::{BranchType, Repository};

use crate::git;
use crate::git_commands::git_branch;
use crate::msg;

/// Initialize a new integration branch tracking a remote upstream.
///
/// Creates a branch (default name: "integration") at the upstream tip and switches to it.
/// The remote is auto-detected from the current branch's upstream tracking ref.
/// If no upstream is found, the user is prompted to choose one.
pub fn run(name: Option<String>) -> Result<()> {
    let repo = git::open_repo()?;

    let name = name.unwrap_or_else(|| "integration".to_string());
    let name = name.trim().to_string();
    if name.is_empty() {
        bail!("Branch name cannot be empty");
    }

    git_branch::validate_name(&name)?;

    git::ensure_branch_not_exists(&repo, &name)?;

    let upstream = detect_upstream(&repo)?;

    let workdir = git::require_workdir(&repo, "create branch")?;

    git_branch::switch_create_tracking(workdir, &name, &upstream)?;

    msg::success(&format!(
        "Initialized integration branch `{}` tracking `{}`",
        name, upstream
    ));

    Ok(())
}

/// Detect the upstream tracking ref to use for the new integration branch.
///
/// Strategy:
/// 1. On GitHub repos with an "upstream" remote (fork workflow), use it.
/// 2. If the current branch has an upstream, use it (e.g., "origin/main").
/// 3. Otherwise, check each remote's HEAD symref (e.g., refs/remotes/origin/HEAD).
/// 4. Fall back to scanning for common branch names (main, master, develop).
/// 5. If exactly one candidate, use it. If multiple, prompt the user.
fn detect_upstream(repo: &Repository) -> Result<String> {
    // On GitHub repos with a fork workflow, prefer the "upstream" remote
    if let Some(upstream) = try_github_upstream(repo) {
        return Ok(upstream);
    }

    // Try the current branch's upstream first
    if let Ok(head) = repo.head()
        && head.is_branch()
        && let Some(branch_name) = head.shorthand()
        && let Ok(local_branch) = repo.find_branch(branch_name, BranchType::Local)
        && let Ok(upstream) = local_branch.upstream()
        && let Ok(Some(upstream_name)) = upstream.name()
    {
        return Ok(upstream_name.to_string());
    }

    // No upstream on current branch — gather remote candidates
    let candidates = gather_remote_candidates(repo)?;

    match candidates.len() {
        0 => bail!(
            "No remote tracking branches found.\n\
             Set up a remote with: `git remote add origin <url>`"
        ),
        1 => Ok(candidates[0].clone()),
        _ => {
            // Prompt the user to pick
            msg::select(
                "Which remote branch should this integration track?",
                candidates,
            )
        }
    }
}

/// On GitHub repositories with an "upstream" remote, find its default branch.
///
/// In the fork workflow, "origin" is the user's fork and "upstream" is the
/// original repository. The integration branch should track the original repo.
fn try_github_upstream(repo: &Repository) -> Option<String> {
    // Check if any remote URL points to GitHub
    let remotes = repo.remotes().ok()?;
    let is_github = remotes.iter().flatten().any(|name| {
        repo.find_remote(name)
            .ok()
            .and_then(|r| r.url().map(|u| u.contains("github.com")))
            .unwrap_or(false)
    });
    if !is_github {
        return None;
    }

    // Check if there's an "upstream" remote
    repo.find_remote("upstream").ok()?;

    // Find the upstream remote's default branch via HEAD symref
    let head_ref = "refs/remotes/upstream/HEAD";
    if let Ok(reference) = repo.find_reference(head_ref)
        && let Ok(resolved) = reference.resolve()
        && let Some(name) = resolved.shorthand()
    {
        return Some(name.to_string());
    }

    // Fall back to common branch names
    for branch_name in &["main", "master", "develop"] {
        let ref_name = format!("upstream/{}", branch_name);
        if repo.find_branch(&ref_name, BranchType::Remote).is_ok() {
            return Some(ref_name);
        }
    }

    None
}

/// Gather candidate remote tracking branches.
///
/// For each remote, first checks the remote's HEAD symref (e.g., refs/remotes/origin/HEAD)
/// which points to the remote's default branch. Falls back to scanning for common
/// branch names (main, master, develop) if the HEAD symref is not available.
fn gather_remote_candidates(repo: &Repository) -> Result<Vec<String>> {
    let mut candidates = Vec::new();

    let remotes = repo.remotes()?;
    for remote_name in remotes.iter() {
        let Some(remote_name) = remote_name else {
            continue;
        };

        // Try the remote's HEAD symref first (e.g., refs/remotes/origin/HEAD → origin/main)
        let head_ref = format!("refs/remotes/{}/HEAD", remote_name);
        if let Ok(reference) = repo.find_reference(&head_ref)
            && let Ok(resolved) = reference.resolve()
            && let Some(name) = resolved.shorthand()
        {
            candidates.push(name.to_string());
            continue;
        }

        // Fall back to common default branch names
        for branch_name in &["main", "master", "develop"] {
            let ref_name = format!("{}/{}", remote_name, branch_name);
            if repo.find_branch(&ref_name, BranchType::Remote).is_ok() {
                candidates.push(ref_name);
                break; // Use the first match per remote
            }
        }
    }

    Ok(candidates)
}

#[cfg(test)]
#[path = "init_test.rs"]
mod tests;
