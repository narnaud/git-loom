use anyhow::{Context, Result, bail};
use git2::Repository;

use crate::core::msg;
use crate::core::repo;
use crate::core::weave::{self, Weave};
use crate::git;

/// Create a new branch at a target commit, weaving it into the integration branch
/// if the target is between the merge-base and HEAD.
///
/// If `name` is `None`, prompts interactively for a branch name.
/// If `target` is `None`, defaults to the merge-base (upstream base) commit.
/// The target can be a commit hash, branch name, or shortID.
///
/// When the branch is created at a commit that is neither HEAD nor the merge-base,
/// the topology is restructured: commits after the branch point are rebased onto
/// the merge-base, and a merge commit joins them with the branch.
pub fn run(name: Option<String>, target: Option<String>) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "create branch")?;

    let name = match name {
        Some(n) => n,
        None => msg::input("Branch name", "re-run with: loom branch <name>", |s| {
            if s.trim().is_empty() {
                Err("Branch name cannot be empty")
            } else {
                Ok(())
            }
        })?,
    };

    let name = name.trim().to_string();
    if name.is_empty() {
        bail!("Branch name cannot be empty");
    }

    git::branch_validate_name(workdir, &name)?;

    repo::ensure_branch_not_exists(&repo, &name)?;

    // Gather repo info once (needed for merge-base default and weave check).
    // May fail if not on an integration branch — that's OK for plain branch creation.
    let info = repo::gather_repo_info(&repo, false, 1).ok();

    let commit_hash = resolve_commit(&repo, &info, target.as_deref())?;

    git::branch_create(workdir, &name, &commit_hash)?;

    repo::warn_if_hidden(&repo, &name);
    msg::success(&format!(
        "Created branch `{}` at `{}`",
        name,
        git::short_hash(&commit_hash)
    ));

    // Check if weaving is needed (only possible when repo info is available)
    if let Some(ref info) = info
        && should_weave(info, &repo, &commit_hash)?
    {
        // Use from_repo (not from_repo_with_info) because the branch list
        // is stale — the new branch was just created after info was gathered.
        let mut graph = Weave::from_repo(&repo)?;
        graph.weave_branch(&name);

        let todo = graph.to_todo();
        if let Err(e) =
            weave::run_rebase_or_abort(workdir, Some(&graph.base_oid.to_string()), &todo)
        {
            let _ = git::branch_delete(workdir, &name);
            return Err(e);
        }

        msg::success(&format!("Woven `{}` into integration branch", name));
    }

    Ok(())
}

/// Determine if weaving is needed after branch creation.
///
/// Weaving is needed when the branch target is on the first-parent line
/// from HEAD to the merge-base (i.e., it's a loose commit on the integration
/// line, not already on a side branch). Commits at the merge-base are excluded
/// since no topology change is needed. Branching at HEAD weaves all first-parent
/// commits into the new branch section with a merge commit.
fn should_weave(info: &repo::RepoInfo, repo: &Repository, commit_hash: &str) -> Result<bool> {
    let head_oid = repo::head_oid(repo)?;
    let branch_oid = git2::Oid::from_str(commit_hash)?;

    let merge_base_oid = info.upstream.merge_base_oid;

    if branch_oid == merge_base_oid {
        return Ok(false);
    }

    // HEAD is on the first-parent line by definition
    if branch_oid == head_oid {
        return Ok(true);
    }

    // Only weave if the target commit is on the first-parent line.
    // Commits on side branches (reachable only through merge second-parents)
    // already have the merge topology in place.
    if !repo::is_on_first_parent_line(repo, head_oid, merge_base_oid, branch_oid)? {
        return Ok(false);
    }

    Ok(true)
}

/// Resolve an optional target to a full commit hash.
/// If no target, defaults to the merge-base (upstream base).
fn resolve_commit(
    repo: &Repository,
    info: &Option<repo::RepoInfo>,
    target: Option<&str>,
) -> Result<String> {
    match target {
        None => {
            // Default: merge-base commit
            let info = info
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No upstream tracking branch — cannot determine merge-base\nSpecify an explicit target commit"))?;
            Ok(info.upstream.merge_base_oid.to_string())
        }
        Some(t) => {
            let resolved = repo::resolve_arg(
                repo,
                t,
                &[repo::TargetKind::Commit, repo::TargetKind::Branch],
            )?;
            match resolved {
                repo::Target::Commit(hash) => Ok(hash),
                repo::Target::Branch(name) => {
                    // Resolve branch to its tip commit
                    let branch = repo.find_branch(&name, git2::BranchType::Local)?;
                    let oid = branch
                        .get()
                        .target()
                        .context("Branch does not point to a commit")?;
                    Ok(oid.to_string())
                }
                _ => unreachable!(),
            }
        }
    }
}

#[cfg(test)]
#[path = "branch_test.rs"]
mod tests;
