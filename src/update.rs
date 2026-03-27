use std::path::Path;

use anyhow::{Context, Result, bail};
use git2::BranchType;
use serde::{Deserialize, Serialize};

use crate::core::repo;

use crate::core::msg;
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave::Weave;
use crate::git::{self, RebaseOutcome};

#[derive(Serialize, Deserialize)]
struct UpdateContext {
    branch_name: String,
    upstream_name: String,
    skip_confirm: bool,
}

/// Update the integration branch by fetching and rebasing from upstream.
pub fn run(skip_confirm: bool) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "update")?.to_path_buf();
    let git_dir = repo.path().to_path_buf();

    // Validate that we're on a branch with an upstream tracking ref
    let head = repo.head().context("Failed to get HEAD reference")?;
    if !head.is_branch() {
        bail!("HEAD is detached\nSwitch to an integration branch");
    }

    let branch_name = head
        .shorthand()
        .context("Could not determine current branch name")?
        .to_string();

    let local_branch = repo.find_branch(&branch_name, BranchType::Local)?;
    let upstream = local_branch.upstream().with_context(|| {
        format!(
            "Branch `{}` has no upstream tracking branch\n\
             Run `loom init` to set up an integration branch",
            branch_name
        )
    })?;
    let upstream_name = upstream
        .name()?
        .context("Upstream branch name is not valid UTF-8")?
        .to_string();

    // Fetch with tags, force-update, and prune deleted remote branches
    let spinner = msg::spinner();
    spinner.start("Fetching latest changes...");

    let result = git::run_git(&workdir, &["fetch", "--tags", "--force", "--prune"]);

    match result {
        Ok(()) => {
            spinner.stop("Fetched latest changes");
        }
        Err(e) => {
            spinner.error("Fetch failed");
            return Err(e);
        }
    }

    // Save rollback state before the rebase
    let ctx = UpdateContext {
        branch_name: branch_name.clone(),
        upstream_name: upstream_name.clone(),
        skip_confirm,
    };
    let state = LoomState {
        command: "update".to_string(),
        rollback: Rollback {
            ..Default::default()
        },
        context: serde_json::to_value(&ctx)?,
    };
    transaction::save(&git_dir, &state)?;

    // Rebase onto upstream using the weave model.
    //
    // Plain `git rebase --rebase-merges` preserves merge topology literally,
    // which can place new upstream commits on the wrong side of merge commits
    // (inside a feature branch instead of on the base line). The weave model
    // generates a clean todo where every branch section `reset onto`, ensuring
    // branches are correctly rebased onto the new upstream tip.
    let spinner = msg::spinner();
    spinner.start("Rebasing onto upstream...");

    // Re-open repo after fetch (remote refs changed)
    let repo = git2::Repository::discover(&workdir)?;

    let outcome = match Weave::from_repo(&repo) {
        Ok(mut graph) => {
            // Drop branch-section commits already in the new upstream
            // (merged or cherry-picked). This prevents conflicts from
            // replaying commits whose content is already in the base.
            let new_upstream_oid = repo
                .revparse_single(&upstream_name)
                .context("Failed to resolve upstream ref")?
                .id();
            graph.filter_upstream_commits(&repo, &workdir, new_upstream_oid)?;
            let todo = graph.to_todo();
            crate::core::weave::run_rebase(&workdir, Some(&upstream_name), &todo)
        }
        Err(_) => {
            // Fallback: no integration topology (e.g., plain branch with no weave).
            // Use plain rebase.
            git::rebase(&git_dir, &workdir, &upstream_name)
        }
    };

    match outcome {
        Ok(RebaseOutcome::Completed) => {
            spinner.stop("Rebased onto upstream");
            transaction::delete(&git_dir)?;
            // Re-open repo after rebase (OIDs changed)
            let repo2 = git2::Repository::discover(&workdir)?;
            post_update(&workdir, &repo2, &ctx)?;
        }
        Ok(RebaseOutcome::Conflicted) => {
            spinner.error("Rebase paused due to conflicts");
            transaction::warn_conflict_paused("update");
        }
        Err(e) => {
            let _ = git::rebase_abort(&workdir);
            transaction::delete(&git_dir)?;
            spinner.error("Rebase failed");
            return Err(e);
        }
    }

    Ok(())
}

/// Resume an `update` operation after a conflict has been resolved.
pub fn after_continue(workdir: &Path, context: &serde_json::Value) -> Result<()> {
    let ctx: UpdateContext =
        serde_json::from_value(context.clone()).context("Failed to parse update resume context")?;
    let repo = git2::Repository::discover(workdir)?;
    post_update(workdir, &repo, &ctx)
}

/// Post-rebase work: submodule update, upstream reporting, gone-branch cleanup.
fn post_update(workdir: &Path, repo: &git2::Repository, ctx: &UpdateContext) -> Result<()> {
    // Update submodules if .gitmodules exists
    if workdir.join(".gitmodules").exists() {
        let spinner = msg::spinner();
        spinner.start("Updating submodules...");

        let result = git::run_git(workdir, &["submodule", "update", "--init", "--recursive"]);

        match result {
            Ok(()) => {
                spinner.stop("Updated submodules");
            }
            Err(e) => {
                spinner.error("Submodule update failed");
                return Err(e);
            }
        }
    }

    // Show the latest upstream commit
    let upstream_info = repo
        .revparse_single(&ctx.upstream_name)
        .ok()
        .and_then(|obj| obj.peel_to_commit().ok())
        .map(|commit| {
            let short_id = git::short_hash(&commit.id().to_string()).to_string();
            let summary = repo::commit_subject(&commit);
            format!(" ({} {})", short_id, summary)
        })
        .unwrap_or_default();

    msg::success(&format!(
        "Updated branch `{}` with `{}`{}",
        ctx.branch_name, ctx.upstream_name, upstream_info
    ));

    // Propose removing local branches whose remote tracking branch was pruned
    let gone = find_branches_with_gone_upstream(repo, &ctx.branch_name)?;
    if !gone.is_empty() {
        let mut warn_msg = format!(
            "{} local {} with a gone upstream:",
            gone.len(),
            if gone.len() == 1 {
                "branch"
            } else {
                "branches"
            }
        );
        for name in &gone {
            warn_msg.push('\n');
            warn_msg.push_str(name);
        }
        msg::warn(&warn_msg);
        let confirmed = ctx.skip_confirm
            || msg::confirm(if gone.len() == 1 {
                "Remove it?"
            } else {
                "Remove them?"
            })?;
        if confirmed {
            for name in &gone {
                match git::branch_delete(workdir, name) {
                    Ok(()) => msg::success(&format!("Removed branch `{}`", name)),
                    Err(_) => {
                        msg::warn(&format!(
                            "Skipped branch `{}` — it has unmerged local commits.\n\
                             Use `git branch -D {}` to force-delete.",
                            name, name
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Find local branches whose configured upstream tracking ref no longer exists.
///
/// After `git fetch --prune`, remote-tracking refs for deleted remote branches
/// are removed. Any local branch that had an upstream configured pointing to
/// one of those refs is considered "gone".
fn find_branches_with_gone_upstream(
    repo: &git2::Repository,
    current_branch: &str,
) -> Result<Vec<String>> {
    let config = repo.config()?;
    let mut gone = Vec::new();

    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        let Some(name) = branch.name()? else {
            continue;
        };
        let name = name.to_string();
        if name == current_branch {
            continue;
        }

        // Check if an upstream remote is configured for this branch
        let remote_key = format!("branch.{}.remote", name);
        let Ok(remote) = config.get_string(&remote_key) else {
            continue;
        };

        // Check if the merge ref (upstream branch name) is configured
        let merge_key = format!("branch.{}.merge", name);
        let Ok(merge) = config.get_string(&merge_key) else {
            continue;
        };

        // Construct the remote-tracking ref and check if it still exists
        let branch_part = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
        let tracking_ref = format!("refs/remotes/{}/{}", remote, branch_part);
        if repo.find_reference(&tracking_ref).is_err() {
            gone.push(name);
        }
    }

    Ok(gone)
}

#[cfg(test)]
#[path = "update_test.rs"]
mod tests;
