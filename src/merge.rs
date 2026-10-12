use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use git2::{BranchType, Oid, Repository};
use serde::{Deserialize, Serialize};

use crate::core::msg;
use crate::core::repo;
use crate::core::transaction::{self, LoomState, Rollback};
use crate::git::{self, MergeOutcome};

#[derive(Serialize, Deserialize)]
struct MergeContext {
    branch_name: String,
}

/// Merge an existing branch into the integration branch.
///
/// If no branch is specified, shows an interactive picker with local branches
/// not currently woven. With `--all`, also shows remote branches without a
/// local counterpart.
pub fn run(branch: Option<String>, all: bool) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "merge")?;
    let git_dir = repo.path().to_path_buf();
    let info = repo::gather_repo_info(&repo, false, 1)?;
    let others = other_integrations(&repo, workdir, &info.branch_name)?;

    // `git merge` refuses any staged change, whatever it touches.
    if !repo::get_staged_files(&repo)?.is_empty() {
        bail!("Cannot merge with staged changes\nCommit or unstage them first");
    }

    let merged = merged_into_head(workdir)?;
    let branch_name = match branch {
        Some(name) => resolve_non_woven_branch(&repo, &info, &others, &merged, &name)?,
        None => pick_branch(&repo, &info, &others, &merged, all)?,
    };

    // Remote branches need a local tracking branch before they can be merged.
    let created = repo.find_branch(&branch_name, BranchType::Local).is_err();
    let local_name = if !created {
        branch_name.clone()
    } else {
        let local = repo::upstream_local_branch(&branch_name);
        git::branch_create(workdir, &local, &branch_name)?;
        let tracked = repo
            .find_branch(&local, BranchType::Local)
            .and_then(|mut b| b.set_upstream(Some(&branch_name)));
        if let Err(e) = tracked {
            discard_created(workdir, &local);
            return Err(e.into());
        }
        local
    };

    // Merge the branch into integration (--no-ff) so it appears in the topology
    let outcome = match crate::git::merge_no_ff(workdir, &git_dir, &local_name) {
        Ok(outcome) => outcome,
        Err(e) => {
            if created {
                discard_created(workdir, &local_name);
            }
            return Err(e);
        }
    };
    match outcome {
        MergeOutcome::Completed => {
            msg::success(&format!("Merged `{}` into integration branch", local_name));
        }
        MergeOutcome::Stopped => {
            let delete_branches = if created {
                vec![local_name.clone()]
            } else {
                Vec::new()
            };
            let state = LoomState {
                command: "merge".to_string(),
                rollback: Rollback {
                    delete_branches,
                    ..Rollback::default()
                },
                context: serde_json::to_value(MergeContext {
                    branch_name: local_name,
                })?,
                protect: Vec::new(),
                targets: Vec::new(),
            };
            transaction::save(&git_dir, &state)?;
            transaction::warn_paused(workdir, "merge");
        }
    }

    Ok(())
}

/// Delete the tracking branch a remote pick created, for a merge that did not
/// happen; one that cannot be deleted is named, so it is not left unnoticed.
fn discard_created(workdir: &Path, branch: &str) {
    if git::branch_delete(workdir, branch).is_err() {
        msg::warn(&format!(
            "Could not delete `{}`, created for this merge\n\
             Delete it with `git branch -D {}`",
            branch, branch
        ));
    }
}

/// Resume a `loom merge` after conflicts have been resolved.
pub fn after_continue(context: &serde_json::Value) -> anyhow::Result<()> {
    let ctx: MergeContext =
        serde_json::from_value(context.clone()).context("Failed to parse merge resume context")?;
    msg::success(&format!(
        "Merged `{}` into integration branch",
        ctx.branch_name
    ));
    Ok(())
}

/// The integration branch of another worktree: its checked-out branch, when
/// that branch has an upstream.
struct OtherIntegration {
    branch: String,
    path: PathBuf,
    tip: Oid,
    upstream_tip: Oid,
}

/// Every other worktree's integration branch. `current` is skipped by name, as
/// a branch is checked out in one worktree at most.
fn other_integrations(
    repo: &Repository,
    workdir: &Path,
    current: &str,
) -> Result<Vec<OtherIntegration>> {
    let mut result = Vec::new();
    for checkout in git::git_worktree::worktree_checkouts(workdir)? {
        if checkout.branch == current {
            continue;
        }
        let Ok(local) = repo.find_branch(&checkout.branch, BranchType::Local) else {
            continue;
        };
        let (Some(tip), Some(upstream_tip)) = (
            local.get().target(),
            local.upstream().ok().and_then(|u| u.get().target()),
        ) else {
            continue;
        };
        result.push(OtherIntegration {
            branch: checkout.branch,
            path: checkout.path,
            tip,
            upstream_tip,
        });
    }
    Ok(result)
}

/// The other integration branch `branch` is woven into: its tip is in that
/// branch's history but not yet in its upstream (Spec 005).
fn woven_elsewhere<'a>(
    repo: &Repository,
    others: &'a [OtherIntegration],
    branch: &str,
) -> Result<Option<&'a OtherIntegration>> {
    let Some(tip) = repo
        .find_branch(branch, BranchType::Local)
        .ok()
        .and_then(|local| local.get().target())
    else {
        return Ok(None);
    };
    for other in others {
        if other.branch != branch
            && repo::contains(repo, other.tip, tip)?
            && !repo::contains(repo, other.upstream_tip, tip)?
        {
            return Ok(Some(other));
        }
    }
    Ok(None)
}

/// Full names of the local and remote branches HEAD already holds, which
/// `git merge` would make no commit for: a branch merged upstream that nobody
/// pruned, say. One walk, where a merge-base per branch would go back as far as
/// each old remote branch.
fn merged_into_head(workdir: &Path) -> Result<HashSet<String>> {
    let refs = git::run_git_stdout(
        workdir,
        &[
            "for-each-ref",
            "--merged=HEAD",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    Ok(refs.lines().map(str::to_string).collect())
}

fn in_head(merged: &HashSet<String>, branch: &git2::Branch) -> bool {
    branch.get().name().is_ok_and(|name| merged.contains(name))
}

/// Resolve a branch argument, ensuring it's NOT already woven, here or in
/// another worktree's integration branch, nor in HEAD's history.
fn resolve_non_woven_branch(
    repo: &Repository,
    info: &repo::RepoInfo,
    others: &[OtherIntegration],
    merged: &HashSet<String>,
    branch_arg: &str,
) -> Result<String> {
    // Check if it's already woven
    if info.branches.iter().any(|b| b.name == branch_arg) {
        bail!(
            "Branch '{}' is already woven into the integration branch",
            branch_arg
        );
    }

    // Weaving it would weave everything woven there as well.
    if let Some(other) = others.iter().find(|o| o.branch == branch_arg) {
        bail!(
            "Branch `{}` is checked out in the worktree at `{}`",
            branch_arg,
            other.path.display()
        );
    }

    if let Some(other) = woven_elsewhere(repo, others, branch_arg)? {
        bail!(
            "Branch `{}` is already woven into `{}` at `{}`\n\
             Unmerge it there first: `loom unmerge {}`",
            branch_arg,
            other.branch,
            other.path.display(),
            branch_arg
        );
    }

    let Ok(branch) = repo
        .find_branch(branch_arg, BranchType::Local)
        .or_else(|_| repo.find_branch(branch_arg, BranchType::Remote))
    else {
        bail!("Branch '{}' not found", branch_arg)
    };
    if in_head(merged, &branch) {
        bail!(
            "Branch `{}` is already in the integration branch's history\n\
             There is nothing to merge",
            branch_arg
        );
    }
    Ok(branch_arg.to_string())
}

/// Interactive picker: non-woven local branches, then, with `include_remote`,
/// remote ones under their own heading.
fn pick_branch(
    repo: &Repository,
    info: &repo::RepoInfo,
    others: &[OtherIntegration],
    merged: &HashSet<String>,
    include_remote: bool,
) -> Result<String> {
    let woven_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    let current_branch = &info.branch_name;

    let mut local: Vec<String> = Vec::new();
    let mut remote: Vec<String> = Vec::new();

    // Local branches not woven and not the current branch
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        if let Some(name) = branch.name()?
            && name != current_branch
            && !woven_names.contains(&name)
            && !others.iter().any(|o| o.branch == name)
            && woven_elsewhere(repo, others, name)?.is_none()
            && !in_head(merged, &branch)
        {
            local.push(name.to_string());
        }
    }

    // Remote branches without a local counterpart
    if include_remote {
        let local_names: HashSet<String> = repo
            .branches(Some(BranchType::Local))?
            .filter_map(|b| b.ok())
            .filter_map(|(b, _)| b.name().ok().flatten().map(|n| n.to_string()))
            .collect();

        let upstream_label = &info.upstream.label;

        for branch_result in repo.branches(Some(BranchType::Remote))? {
            let (branch, _) = branch_result?;
            if let Some(name) = branch.name()? {
                // Skip the upstream branch (e.g. origin/main)
                if name == upstream_label {
                    continue;
                }
                // Skip HEAD pointer
                if name.ends_with("/HEAD") {
                    continue;
                }
                // Skip if a local branch with the same short name exists
                if !local_names.contains(&repo::upstream_local_branch(name))
                    && !in_head(merged, &branch)
                {
                    remote.push(name.to_string());
                }
            }
        }
    }

    if local.is_empty() && remote.is_empty() {
        bail!("No branches available to merge");
    }

    msg::select_grouped(
        "Select branch to merge",
        vec![
            ("Local branches".to_string(), local),
            (
                "Remote branches (merged through a new local branch)".to_string(),
                remote,
            ),
        ],
        "re-run with: loom merge <branch>",
    )
}

#[cfg(test)]
#[path = "merge_test.rs"]
mod tests;
