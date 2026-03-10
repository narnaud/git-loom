use anyhow::{Context, Result, bail};
use git2::Repository;

use crate::git;
use crate::git_commands::{self, git_branch};
use crate::msg;
use crate::weave::{self, Weave};

use super::{should_weave, warn_if_hidden};

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
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "create branch")?;

    let name = match name {
        Some(n) => n,
        None => msg::input("Branch name", |s| {
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

    git_branch::validate_name(&name)?;

    git::ensure_branch_not_exists(&repo, &name)?;

    // Gather repo info once (needed for merge-base default and weave check).
    // May fail if not on an integration branch — that's OK for plain branch creation.
    let info = git::gather_repo_info(&repo, false, 1).ok();

    let commit_hash = resolve_commit(&repo, &info, target.as_deref())?;

    git_branch::create(workdir, &name, &commit_hash)?;

    warn_if_hidden(&repo, &name);
    msg::success(&format!(
        "Created branch `{}` at `{}`",
        name,
        git_commands::short_hash(&commit_hash)
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
        if let Err(e) = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo) {
            let _ = git_branch::delete(workdir, &name);
            return Err(e);
        }

        msg::success(&format!("Woven `{}` into integration branch", name));
    }

    Ok(())
}

/// Resolve an optional target to a full commit hash.
/// If no target, defaults to the merge-base (upstream base).
fn resolve_commit(
    repo: &Repository,
    info: &Option<git::RepoInfo>,
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
            let resolved = git::resolve_target(repo, t)?;
            match resolved {
                git::Target::Commit(hash) => Ok(hash),
                git::Target::Branch(name) => {
                    // Resolve branch to its tip commit
                    let branch = repo.find_branch(&name, git2::BranchType::Local)?;
                    let oid = branch
                        .get()
                        .target()
                        .context("Branch does not point to a commit")?;
                    Ok(oid.to_string())
                }
                git::Target::File(path) => bail!(
                    "Target resolved to file '{}'\nUse a commit or branch target instead",
                    path
                ),
                git::Target::Unstaged => bail!("Cannot use unstaged as a branch target"),
                git::Target::CommitFile { .. } => {
                    bail!("Cannot use a commit file as a branch target")
                }
            }
        }
    }
}
