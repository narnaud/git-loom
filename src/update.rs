use std::path::Path;

use anyhow::{Context, Result, bail};
use git2::BranchType;
use serde::{Deserialize, Serialize};

use crate::core::repo;

use crate::core::agent_mode;
use crate::core::msg;
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave::Weave;
use crate::git::{self, RebaseOutcome};

#[derive(Serialize, Deserialize)]
struct UpdateContext {
    branch_name: String,
    upstream_name: String,
    skip_confirm: bool,
    /// Branches whose every commit was already upstream before the rebase.
    #[serde(default)]
    merged_branches: Vec<String>,
}

/// Update the integration branch by fetching and rebasing from upstream.
pub fn run(skip_confirm: bool) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "update")?.to_path_buf();
    let git_dir = repo.path().to_path_buf();

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

    let head_refname = head.name().context("HEAD ref name is not valid UTF-8")?;
    let upstream_remote = repo
        .branch_upstream_remote(head_refname)
        .context("Failed to resolve the upstream remote")?;
    let upstream_remote = upstream_remote
        .as_str()
        .context("Upstream remote name is not valid UTF-8")?
        .to_string();

    // Fetch with tags, force-update, and prune deleted remote branches. The
    // remote is named explicitly so `--tags` makes the tag refspec explicit and
    // `--prune` only drops tags missing from *this* remote: under
    // `fetch.all = true` a plain `git fetch` would also hit other (typically
    // tagless) remotes and wipe every tag on each run. The spinner covers a slow
    // fetch; git's own summary is printed afterwards so the user can see whether
    // anything was pulled, with `--no-progress` keeping out the transfer noise.
    let spinner = msg::spinner();
    spinner.start("Fetching latest changes...");

    let result = git::run_git_combined(
        &workdir,
        &[
            "fetch",
            "--no-progress",
            "--tags",
            "--force",
            "--prune",
            &upstream_remote,
        ],
    );

    match result {
        Ok(summary) => {
            spinner.stop("Fetched latest changes");
            if !summary.is_empty() {
                println!("{}", summary);
            }
        }
        Err(e) => {
            spinner.error("Fetch failed");
            return Err(e);
        }
    }

    fetch_push_remote(&repo, &workdir, &upstream_name);

    let repo = git2::Repository::discover(&workdir)?;

    // Rebase onto upstream using the weave model. Plain
    // `git rebase --rebase-merges` preserves merge topology literally, which can
    // place new upstream commits inside a feature branch instead of on the base
    // line; the weave's todo `reset onto` for every branch section, so branches
    // land on the new upstream tip. No topology means a plain rebase.
    let (todo, merged_branches) = match Weave::from_repo(&repo) {
        Ok(mut graph) => {
            // Drop branch-section commits already in the new upstream, so their
            // content is not replayed onto a base that already has it.
            let new_upstream_oid = repo
                .revparse_single(&upstream_name)
                .context("Failed to resolve upstream ref")?
                .id();
            let filtered_out = graph.filter_upstream_commits(&repo, &workdir, new_upstream_oid)?;

            // Fully merged branches: every commit was filtered out (merged
            // or cherry-picked), or the tip sits below the merge base with
            // the upstream. The weave only knows tips above the merge base,
            // so ancestry has to find the latter.
            let mut merged = find_branches_merged_upstream(
                &repo,
                &branch_name,
                repo::head_oid(&repo)?,
                graph.base_oid,
                new_upstream_oid,
            )?;
            merged.extend(filtered_out);
            merged.sort();
            merged.dedup();
            (Some(graph.to_todo()), merged)
        }
        Err(_) => (None, Vec::new()),
    };

    // Save rollback state before the rebase
    let ctx = UpdateContext {
        branch_name: branch_name.clone(),
        upstream_name: upstream_name.clone(),
        skip_confirm,
        merged_branches,
    };
    let state = LoomState {
        command: "update".to_string(),
        rollback: Rollback {
            ..Default::default()
        },
        context: serde_json::to_value(&ctx)?,
    };
    transaction::save(&git_dir, &state)?;

    let spinner = msg::spinner();
    spinner.start("Rebasing onto upstream...");

    let outcome = match &todo {
        Some(todo) => crate::core::weave::run_rebase(&workdir, Some(&upstream_name), todo),
        None => git::rebase(&git_dir, &workdir, &upstream_name),
    };

    match outcome {
        Ok(RebaseOutcome::Completed) => {
            spinner.stop("Rebased onto upstream");
            transaction::delete(&git_dir)?;
            let repo2 = git2::Repository::discover(&workdir)?;
            post_update(&workdir, &repo2, &ctx)?;
        }
        Ok(RebaseOutcome::Stopped) => {
            spinner.error("Rebase paused");
            transaction::warn_paused(&workdir, "update");
        }
        Ok(RebaseOutcome::Paused) => {
            spinner.error("Rebase paused");
            transaction::warn_paused_at_edit(Some("update"));
        }
        Err(e) => {
            spinner.error("Rebase failed");
            return Err(git::rebase_abort_then_cleanup(&workdir, e, || {
                if let Err(e) = transaction::delete(&git_dir) {
                    // Left behind, it blocks every later loom command with a
                    // "paused" message for an operation that is over.
                    msg::warn(&format!("could not remove the loom state file: {e}"));
                }
            }));
        }
    }

    Ok(())
}

/// Fetch and prune the push remote when it differs from the integration
/// branch's remote.
///
/// In a fork workflow feature branches live on the push remote, which the
/// fetch above never touches: its remote-tracking refs would stay stale and
/// branches deleted on the fork would never show up as gone.
///
/// Only branches are pruned: tags come from the integration remote, which is
/// the authoritative one. `--no-prune-tags` says so explicitly — a user-level
/// `fetch.pruneTags = true` would delete every tag missing from the fork, and
/// the next update's `--tags` fetch would re-add them all.
fn fetch_push_remote(repo: &git2::Repository, workdir: &Path, upstream_name: &str) {
    let Some(remote) = crate::push::fork_push_remote(repo, workdir, upstream_name) else {
        return;
    };

    let spinner = msg::spinner();
    spinner.start(&format!("Fetching `{}`...", remote));

    match git::run_git_combined(
        workdir,
        &[
            "fetch",
            "--no-progress",
            "--prune",
            "--no-prune-tags",
            &remote,
        ],
    ) {
        Ok(summary) => {
            spinner.stop(&format!("Fetched `{}`", remote));
            if !summary.is_empty() {
                println!("{}", summary);
            }
        }
        // An unreachable fork must not stop the update: the integration branch
        // can still be rebased onto its own upstream.
        Err(_) => {
            spinner.error(&format!(
                "Could not fetch `{}` — branches deleted there are not detected",
                remote
            ));
        }
    }
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

    // Propose removing local branches that are fully merged upstream or
    // whose remote tracking branch was pruned
    let gone = find_branches_with_gone_upstream(repo, &ctx.branch_name)?;
    // The merged list predates the rebase; a branch may be gone by now
    // (deleted by hand during a conflict pause)
    let merged: Vec<String> = ctx
        .merged_branches
        .iter()
        .filter(|name| !gone.contains(name) && repo.find_branch(name, BranchType::Local).is_ok())
        .cloned()
        .collect();
    let to_remove: Vec<&String> = merged.iter().chain(gone.iter()).collect();
    if !to_remove.is_empty() {
        warn_branch_list(&merged, "fully merged upstream");
        warn_branch_list(&gone, "with a gone upstream");
        // Post-mutation prompt: the pull-rebase already succeeded, so agent
        // mode must not answer `needs_input` (that would imply nothing
        // happened) — skip the optional pruning instead and say how to redo it.
        let confirmed = if agent_mode::enabled() && !ctx.skip_confirm {
            let confirmed = repo::prune_gone_branches(repo);
            if !confirmed {
                msg::warn(
                    "Skipped removing branches (agent mode)\n\
                     Re-run with `loom update -y` to remove them",
                );
            }
            confirmed
        } else {
            ctx.skip_confirm
                || repo::prune_gone_branches(repo)
                || msg::confirm(
                    if to_remove.len() == 1 {
                        "Remove it?"
                    } else {
                        "Remove them?"
                    },
                    "re-run with: loom update -y",
                )?
        };
        if confirmed {
            for name in to_remove {
                // Capture the tip before deletion so users can revive the branch.
                let short_id = repo
                    .revparse_single(name)
                    .ok()
                    .map(|obj| git::short_hash(&obj.id().to_string()).to_string());
                match git::branch_delete(workdir, name) {
                    Ok(()) => match &short_id {
                        Some(id) => {
                            msg::success(&format!("Removed branch `{}` (was {})", name, id))
                        }
                        None => msg::success(&format!("Removed branch `{}`", name)),
                    },
                    Err(_) => {
                        msg::warn(&format!(
                            "Skipped branch `{}` — could not delete it (run `loom trace` for the git error)",
                            name
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Warn with the branch names listed one per line. Silent for an empty list.
fn warn_branch_list(names: &[String], what: &str) {
    if names.is_empty() {
        return;
    }
    let mut warn_msg = format!(
        "{} local {} {}:",
        names.len(),
        if names.len() == 1 {
            "branch"
        } else {
            "branches"
        },
        what
    );
    for name in names {
        warn_msg.push('\n');
        warn_msg.push_str(name);
    }
    msg::warn(&warn_msg);
}

/// Find local branches woven into the integration branch whose tip the new
/// upstream already contains.
///
/// Only branches reachable from `head_oid` and above `base_oid` (the
/// integration base) count. Below that point lies upstream history, where a
/// branch like a local `main` is not loom's to remove.
fn find_branches_merged_upstream(
    repo: &git2::Repository,
    current_branch: &str,
    head_oid: git2::Oid,
    base_oid: git2::Oid,
    upstream_oid: git2::Oid,
) -> Result<Vec<String>> {
    let contains = |tip, oid| repo::contains(repo, tip, oid);

    let mut merged = Vec::new();
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        let Some(name) = branch.name()? else {
            continue;
        };
        if name == current_branch {
            continue;
        }
        let Some(tip) = branch.get().target() else {
            continue;
        };
        if !contains(base_oid, tip)? && contains(upstream_oid, tip)? && contains(head_oid, tip)? {
            merged.push(name.to_string());
        }
    }
    Ok(merged)
}

/// Find local branches whose configured upstream tracking ref no longer
/// exists — `git fetch --prune` removed it, so the branch counts as "gone".
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
