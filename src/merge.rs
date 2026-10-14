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
    /// The set-aside work is back over the merge; what is left is the user's.
    #[serde(default)]
    replayed: bool,
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

    // Setting the work aside would reset the conflict state away, or the
    // cherry-pick or revert the user is in (a merge or rebase is refused for
    // every command, Spec 014).
    if git::has_unmerged_paths(workdir) {
        bail!("Cannot merge with unresolved conflicts\nResolve them first");
    }
    if ["CHERRY_PICK_HEAD", "REVERT_HEAD"]
        .iter()
        .any(|head| git_dir.join(head).exists())
    {
        bail!("Cannot merge during a cherry-pick or revert\nFinish or abort it first");
    }

    let merged = merged_into_head(workdir)?;
    let branch_name = match branch {
        Some(name) => resolve_non_woven_branch(&repo, &info, &others, &merged, &name)?,
        None => pick_branch(&repo, &info, &others, &merged, all)?,
    };
    refuse_unreplayable(workdir, &branch_name)?;

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

    // The merge is one transaction (Spec 005): the uncommitted work is saved in
    // the state and set aside, and `loom abort` returns to exactly where it
    // started, whatever happened in between.
    let state = match begin(&repo, workdir, &git_dir, &local_name, created) {
        Ok(state) => state,
        Err(e) => {
            if created {
                discard_created(workdir, &local_name);
            }
            return Err(e);
        }
    };

    // Merge the branch into integration (--no-ff) so it appears in the topology
    let outcome = match crate::git::merge_no_ff(workdir, &git_dir, &local_name) {
        Ok(outcome) => outcome,
        Err(e) => {
            // Refused with HEAD where it was: the rollback puts the work back
            // as it was, staging included, and takes the tracking branch.
            if let Err(rollback) = state.rollback.apply_abort(workdir) {
                msg::warn(&format!(
                    "Could not put your uncommitted changes back ({rollback})\n\
                     Run `loom abort` to finish"
                ));
                return Err(e);
            }
            git::restore_mtimes();
            transaction::delete(&git_dir)?;
            return Err(e);
        }
    };
    match outcome {
        MergeOutcome::Stopped => {
            if !state.rollback.saved_worktree_patch.is_empty()
                || !state.rollback.saved_staged_patch.is_empty()
            {
                // `git merge --abort` would show a clean tree, the work being in
                // the state alone.
                msg::warn(
                    "Your uncommitted changes are set aside until the merge is done\n\
                     `loom abort`, not `git merge --abort`, brings them back",
                );
            }
            transaction::warn_paused(workdir, "merge");
        }
        MergeOutcome::Completed => {
            if land(workdir, &git_dir, &state)? {
                transaction::delete(&git_dir)?;
            }
        }
    }
    Ok(())
}

/// Save the transaction — HEAD, the uncommitted work as patches, what to
/// delete — then set that work aside for the merge. The state is written first:
/// from the reset on, it is the only copy.
fn begin(
    repo: &Repository,
    workdir: &Path,
    git_dir: &Path,
    local_name: &str,
    created: bool,
) -> Result<LoomState> {
    let state = LoomState {
        command: "merge".to_string(),
        rollback: Rollback {
            reset_hard_to: repo::head_oid(repo)?.to_string(),
            delete_branches: if created {
                vec![local_name.to_string()]
            } else {
                Vec::new()
            },
            saved_staged_patch: git::diff_cached_without_submodules(workdir)?,
            saved_worktree_patch: git::diff_head_without_submodules(workdir)?,
            ..Rollback::default()
        },
        context: serde_json::to_value(MergeContext {
            branch_name: local_name.to_string(),
            replayed: false,
        })?,
        protect: Vec::new(),
        targets: Vec::new(),
    };
    transaction::save(git_dir, &state)?;
    git::snapshot_mtimes(workdir, Some("HEAD"));
    let dirty = !state.rollback.saved_worktree_patch.is_empty()
        || !state.rollback.saved_staged_patch.is_empty();
    if dirty && let Err(e) = git::reset_hard(workdir, "HEAD") {
        git::discard_mtimes();
        // A reset can fail part-way (a file locked on Windows): the state goes
        // only when the tree provably kept the work, as it is its only copy.
        let untouched = git::diff_head_without_submodules(workdir).ok().as_ref()
            == Some(&state.rollback.saved_worktree_patch)
            && git::diff_cached_without_submodules(workdir).ok().as_ref()
                == Some(&state.rollback.saved_staged_patch);
        if !untouched {
            return Err(e.context(
                "Could not set your uncommitted changes aside\n\
                 Run `loom abort` to put them back",
            ));
        }
        transaction::delete(git_dir)?;
        return Err(e);
    }
    Ok(state)
}

/// Put the set-aside work back over the landed merge, three-way. `Ok(true)`:
/// the merge is done; `Ok(false)`: paused on conflicts with that work, for the
/// user to resolve, and the state says so.
fn land(workdir: &Path, git_dir: &Path, state: &LoomState) -> Result<bool> {
    let mut ctx: MergeContext = serde_json::from_value(state.context.clone())
        .context("Failed to parse merge resume context")?;
    // Written by a loom that left the work to `git merge` itself: nothing was
    // set aside, so there is nothing to replay.
    if state.rollback.reset_hard_to.is_empty() {
        msg::success(&format!(
            "Merged `{}` into integration branch",
            ctx.branch_name
        ));
        return Ok(true);
    }
    // A merge the user aborted or rewound with git is not one to report done.
    let landed = git::rev_parse(workdir, "HEAD^2").is_ok()
        && git::rev_parse(workdir, "HEAD^1").ok().as_deref()
            == Some(state.rollback.reset_hard_to.as_str());
    if !landed {
        bail!(
            "The merge of `{}` is no longer at HEAD\n\
             Run `loom abort` to return to before it",
            ctx.branch_name
        );
    }
    if !ctx.replayed {
        // Conflicts already in the tree are the replay's, its state write lost:
        // `add -u` would mark them resolved, markers and all. A clean replay so
        // cut short is found by the patch reversing cleanly, and not redone: a
        // staged deletion or new file would not apply twice.
        let clean = if git::has_unmerged_paths(workdir) {
            false
        } else if git::patch_is_applied(workdir, &state.rollback.saved_worktree_patch) {
            true
        } else {
            // `--3way` refuses a file whose tree differs from the index — one
            // edited during a conflict pause — and staging is rebuilt below.
            git::run_git(workdir, &["add", "-u"])?;
            git::replay_patch_three_way(workdir, &state.rollback.saved_worktree_patch).context(
                "Could not put your uncommitted changes back over the merge\n\
                 Run `loom abort` to return to before it",
            )?
        };
        // Recorded only once the work is in the tree: saying so any earlier,
        // a lost replay would let the next continue drop the only copy.
        ctx.replayed = true;
        let mut replayed = state.clone();
        replayed.context = serde_json::to_value(&ctx)?;
        transaction::save(git_dir, &replayed)?;
        if !clean {
            msg::warn("The merge landed, but your uncommitted changes conflict with it");
            keep_own_binaries(workdir);
            transaction::warn_paused(workdir, "merge");
            return Ok(false);
        }
    } else if git::has_unmerged_paths(workdir) {
        transaction::warn_paused(workdir, "merge");
        return Ok(false);
    }
    git::restore_mtimes();
    // `--3way` stages what it applies, and resolving stages the rest: start
    // from HEAD so the saved patch alone says what is staged.
    git::reset_mixed(workdir, "HEAD")?;
    git::restore_staged_after_rebase(workdir, &state.rollback.saved_staged_patch);
    msg::success(&format!(
        "Merged `{}` into integration branch",
        ctx.branch_name
    ));
    Ok(true)
}

/// Put the user's version of each conflicted binary file in the tree. Git
/// leaves the merge's there, markers being impossible, which history already
/// holds: the user's would be one stage-and-continue from gone.
fn keep_own_binaries(workdir: &Path) {
    let binaries: Vec<String> = git::unmerged_paths(workdir)
        .into_iter()
        .filter(|path| {
            // Git's own test: a NUL in the first 8000 bytes.
            git::run_git_stdout(workdir, &["show", &format!(":3:{path}")])
                .is_ok_and(|blob| blob.bytes().take(8000).any(|b| b == 0))
        })
        .collect();
    for path in &binaries {
        let _ = git::run_git(
            workdir,
            &["checkout", "--theirs", "--", &format!(":(literal){path}")],
        );
    }
    if !binaries.is_empty() {
        let list = binaries
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join(", ");
        msg::warn(&format!(
            "Binary, so no conflict markers: {list}\n\
             The tree holds your version; the merge's is in HEAD (`git checkout HEAD -- <file>`)"
        ));
    }
}

/// Refuse up front what the replay over the merge could not take back as a
/// conflict: a change of the user's to a file `branch` deletes, or a deletion
/// of a file it changes (`git apply --3way` refuses either outright).
fn refuse_unreplayable(workdir: &Path, branch: &str) -> Result<()> {
    let names = |args: &[&str]| -> Result<HashSet<String>> {
        Ok(git::run_git_stdout(workdir, args)?
            .lines()
            .map(str::to_string)
            .collect())
    };
    // A staged submodule pointer is left out of the set-aside work, and the
    // reset would unstage it: the one change it could not bring back.
    let staged_pointers: Vec<String> = names(&["diff", "--name-only", "--cached"])?
        .difference(&names(&[
            "diff",
            "--name-only",
            "--cached",
            "--ignore-submodules=all",
        ])?)
        .cloned()
        .collect();
    if !staged_pointers.is_empty() {
        let list = staged_pointers
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join(", ");
        bail!("Cannot merge with a staged submodule change: {list}\nCommit or unstage it first");
    }
    let range = format!("HEAD...{branch}");
    let mut dirty = names(&["diff", "--name-only", "--no-renames", "HEAD"])?;
    dirty.extend(names(&["diff", "--name-only", "--no-renames", "--cached"])?);
    let mut deleted = names(&[
        "diff",
        "--name-only",
        "--no-renames",
        "--diff-filter=D",
        "HEAD",
    ])?;
    deleted.extend(names(&[
        "diff",
        "--name-only",
        "--no-renames",
        "--diff-filter=D",
        "--cached",
    ])?);
    let theirs_deleted = names(&[
        "diff",
        "--name-only",
        "--no-renames",
        "--diff-filter=D",
        &range,
    ])?;
    let theirs_changed = names(&["diff", "--name-only", "--no-renames", &range])?;
    let mut clash: Vec<&String> = dirty
        .iter()
        .filter(|p| {
            theirs_deleted.contains(*p) || (deleted.contains(*p) && theirs_changed.contains(*p))
        })
        .collect();
    if clash.is_empty() {
        return Ok(());
    }
    clash.sort();
    let list = clash
        .iter()
        .map(|p| format!("`{p}`"))
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "`{branch}` deletes a file you changed, or changes one you deleted: {list}\n\
         Commit or discard those changes first"
    )
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

/// Resume a `loom merge`: replay the set-aside work if the merge only just
/// landed — through `loom continue` or the user's own `git merge --continue` —
/// or finish once its conflicts are resolved. `Ok(false)`: still paused.
pub fn after_continue(workdir: &Path, git_dir: &Path, state: &LoomState) -> Result<bool> {
    land(workdir, git_dir, state)
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
