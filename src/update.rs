use std::path::Path;

use anyhow::{Context, Result, bail};
use git2::BranchType;
use serde::{Deserialize, Serialize};

use crate::git;
use crate::git_commands::{self, git_branch, git_rebase};
use crate::msg;
use crate::transaction::{self, LoomState, Rollback};
use crate::weave::{self, IntegrationEntry, RebaseOutcome, Weave};

#[derive(Serialize, Deserialize)]
struct UpdateContext {
    branch_name: String,
    upstream_name: String,
    skip_confirm: bool,
}

/// Update the integration branch by fetching and rebasing from upstream.
pub fn run(skip_confirm: bool) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "update")?.to_path_buf();
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
             Run `git-loom init` to set up an integration branch",
            branch_name
        )
    })?;
    let upstream_name = upstream
        .name()?
        .context("Upstream branch name is not valid UTF-8")?
        .to_string();

    // Build Weave BEFORE fetch to capture the current integration topology.
    // At this point branch commits are relative to the old upstream merge-base,
    // so the graph is accurate even if the upstream later advances past them.
    let mut graph = Weave::from_repo(&repo)?;

    // Fetch with tags, force-update, and prune deleted remote branches
    let spinner = msg::spinner();
    spinner.start("Fetching latest changes...");

    let result = git_commands::run_git(&workdir, &["fetch", "--tags", "--force", "--prune"]);

    match result {
        Ok(()) => {
            spinner.stop("Fetched latest changes");
        }
        Err(e) => {
            spinner.error("Fetch failed");
            return Err(e);
        }
    }

    // Re-open the repository so we see the updated remote-tracking refs.
    // The external `git fetch` process writes refs to disk; the previously
    // opened Repository object may have stale cached data (libgit2 caches
    // packed-refs keyed by mtime, which can be imprecise on Windows NTFS).
    let repo = git2::Repository::discover(&workdir)?;

    // Resolve the new upstream tip now that the fetch is complete.
    let new_upstream_oid = repo
        .revparse_single(&upstream_name)
        .with_context(|| format!("Could not resolve '{}' after fetch", upstream_name))?
        .id();

    // Remove any commits from branch sections that are now reachable from the
    // new upstream — they were integrated upstream and replaying them would
    // produce empty or duplicate commits.
    let mut partially_trimmed: Vec<String> = Vec::new();
    for section in &mut graph.branch_sections {
        let before = section.commits.len();

        // Capture the section tip before any filtering so git cherry can
        // compare the full original range.
        let section_tip = section.commits.last().map(|c| c.oid.to_string());

        // Content-based detection: find commits whose patch is already present
        // in the new upstream via cherry-pick (different OID, same diff).
        // graph_descendant_of only catches exact OID ancestry, not cherry-picks.
        let cherry_applied: Vec<String> = section_tip
            .as_deref()
            .map(|tip| {
                let upstream_str = new_upstream_oid.to_string();
                git_commands::run_git_stdout(&workdir, &["cherry", &upstream_str, tip])
                    .unwrap_or_default()
                    .lines()
                    .filter(|l| l.starts_with("- "))
                    .filter_map(|l| l[2..].trim().split_whitespace().next().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();

        section.commits.retain(|c| {
            // OID-based: commit is a literal ancestor of the new upstream.
            let by_ancestry = repo
                .graph_descendant_of(new_upstream_oid, c.oid)
                .unwrap_or(false);
            // Content-based: commit's patch is already in the upstream (cherry-pick).
            let full_oid = c.oid.to_string();
            let by_content = cherry_applied
                .iter()
                .any(|abbrev| full_oid.starts_with(abbrev.as_str()));
            !by_ancestry && !by_content
        });

        if !section.commits.is_empty() && section.commits.len() < before {
            // Some (not all) commits were removed; the original merge OID no
            // longer describes the trimmed branch, so let git create a fresh
            // merge commit.
            partially_trimmed.push(section.label.clone());
        }
    }
    for label in &partially_trimmed {
        for entry in &mut graph.integration_line {
            if let IntegrationEntry::Merge {
                label: l,
                original_oid,
            } = entry
                && l == label
            {
                *original_oid = None;
            }
        }
    }

    // Drop sections that became fully empty (entire branch merged upstream).
    let empty_labels: Vec<String> = graph
        .branch_sections
        .iter()
        .filter(|s| s.commits.is_empty())
        .map(|s| s.label.clone())
        .collect();
    for label in &empty_labels {
        graph.drop_branch(label);
    }

    // Also drop Pick entries on the integration line that are already
    // reachable from the new upstream. In non-interactive rebase git skips
    // these automatically; in our interactive todo it would cherry-pick them
    // onto a base that already has those changes, causing unnecessary conflicts.
    graph.integration_line.retain(|entry| {
        if let IntegrationEntry::Pick(commit) = entry {
            !repo
                .graph_descendant_of(new_upstream_oid, commit.oid)
                .unwrap_or(false)
        } else {
            true
        }
    });

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

    // Rebase onto upstream using the Weave-generated todo so topology is
    // always correct, even when feature branches were merged into upstream.
    let spinner = msg::spinner();
    spinner.start("Rebasing onto upstream...");

    let todo = graph.to_todo();
    let outcome = weave::run_rebase(&workdir, Some(&upstream_name), &todo);

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
            let _ = git_rebase::abort(&workdir);
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

        let result =
            git_commands::run_git(workdir, &["submodule", "update", "--init", "--recursive"]);

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
            let short_id = &commit.id().to_string()[..7];
            let summary = commit.summary().unwrap_or("");
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
                match git_branch::delete(workdir, name) {
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
