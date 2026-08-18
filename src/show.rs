use anyhow::{Result, bail};

use crate::core::graph;
use crate::core::repo::{self, Target};
use crate::git;
use crate::status;

/// Show the diff and metadata for a commit (like `git show`), using short IDs.
///
/// With no target, shows the commit at the top of `loom status` — the tip of
/// the integration line, skipping merge commits and hidden branches.
pub fn run(target: Option<String>) -> Result<()> {
    let repo = repo::open_repo()?;
    let revs = show_revs(&repo, target)?;

    let workdir = repo::require_workdir(&repo, "show")?;
    let mut args = vec!["show"];
    args.extend(revs.iter().map(String::as_str));
    git::run_git_interactive(workdir, &args)
}

/// Resolve a target into the revisions to pass to `git show`, newest first.
fn show_revs(repo: &git2::Repository, target: Option<String>) -> Result<Vec<String>> {
    let Some(target) = target else {
        // Fall back to HEAD outside an integration branch (e.g. plain repo)
        // or when the integration line has no commits of its own.
        if !repo::has_integration_context(repo) {
            return Ok(vec!["HEAD".to_string()]);
        }
        return Ok(match status::top_commit(repo)? {
            Some(oid) => vec![oid.to_string()],
            None => vec!["HEAD".to_string()],
        });
    };

    let resolved = repo::resolve_arg(
        repo,
        &target,
        &[repo::TargetKind::Commit, repo::TargetKind::Branch],
    )?;

    match resolved {
        Target::Commit(hash) => Ok(vec![hash]),
        Target::Branch(name) => branch_revs(repo, &name),
        _ => unreachable!(),
    }
}

/// Every commit `branch` owns, newest first — not just its tip.
///
/// Ownership is the one `loom status` renders: commits reachable from the
/// branch tip down to the next branch tip below it in the stack. Naming the
/// integration branch shows its loose commits instead.
///
/// Falls back to the branch tip alone, like plain `git show`, when there is no
/// integration context (no upstream, detached HEAD) or when `branch` is not
/// part of the stack at all.
fn branch_revs(repo: &git2::Repository, branch: &str) -> Result<Vec<String>> {
    if !repo::has_integration_context(repo) {
        return Ok(vec![branch.to_string()]);
    }

    let mut info = repo::gather_commit_graph(repo)?;

    let commits = if branch == info.branch_name {
        status::apply_hidden_branches(repo, &mut info);
        graph::loose_commits(&info)
    } else if info.branches.iter().any(|b| b.name == branch) {
        graph::commits_in_branch(&info, branch)
    } else {
        return Ok(vec![branch.to_string()]);
    };

    if commits.is_empty() {
        bail!("Branch '{branch}' has no commits of its own");
    }
    Ok(commits.iter().map(|oid| oid.to_string()).collect())
}

#[cfg(test)]
#[path = "show_test.rs"]
mod tests;
