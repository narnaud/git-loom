use anyhow::{Context, Result, bail};
use git2::Repository;
use serde::{Deserialize, Serialize};

use crate::branch::is_on_first_parent_line;
use crate::core::msg;
use crate::core::repo::{self, Target, TargetKind};
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave::{self, EmptiedRefs, RebaseOutcome, Weave};
use crate::git;

fn confirm_or_bail(skip: bool, prompt: &str) -> Result<()> {
    if !skip && !msg::confirm(prompt, "re-run with: loom drop <target> -y")? {
        return Err(msg::cancelled());
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct DropContext {
    commit_hash: String,
    /// Branches the commit was the only commit of, now parked at their base.
    #[serde(default)]
    emptied_branches: Vec<String>,
}

/// Drop commits, branches, files, or all local changes.
///
/// Dispatches based on the resolved target type:
/// - Commit → remove the commit via interactive rebase
/// - Branch → remove all branch commits, unweave merge topology, delete the ref
/// - File → restore/delete the file (tracked: restore, new: delete)
/// - Unstaged (`zz`) → discard all local changes (restore + clean)
///
/// Several targets at once must all be files; they share one confirmation.
pub fn run(targets: Vec<String>, skip_confirm: bool) -> Result<()> {
    let repo = repo::open_repo()?;

    let resolved = targets
        .iter()
        .map(|target| {
            repo::resolve_arg(
                &repo,
                target,
                &[
                    TargetKind::File,
                    TargetKind::Branch,
                    TargetKind::Commit,
                    TargetKind::Unstaged,
                ],
            )
        })
        .collect::<Result<Vec<_>>>()?;

    match resolved.as_slice() {
        [] => bail!("No target to drop"),
        [Target::Commit(hash)] => drop_commit(&repo, hash, skip_confirm),
        [Target::Branch(name)] => drop_branch(&repo, name, skip_confirm),
        [Target::File(path)] => drop_files(&repo, std::slice::from_ref(path), skip_confirm),
        [Target::Unstaged] => drop_all(&repo, skip_confirm),
        [_] => unreachable!(),
        many => {
            // A path and its short ID may both be given: drop each file once,
            // or the second pass fails on what the first already removed.
            let mut paths: Vec<String> = Vec::new();
            for target in many {
                match target {
                    Target::File(path) if paths.contains(path) => {}
                    Target::File(path) => paths.push(path.clone()),
                    _ => bail!("Only files can be dropped together"),
                }
            }
            drop_files(&repo, &paths, skip_confirm)
        }
    }
}

/// How one working-tree path is dropped, per its status.
#[derive(Debug, Clone, Copy)]
enum FileOp {
    /// Directory with tracked changes: restore them, then clean untracked entries.
    RestoreDir,
    /// Directory of untracked entries only.
    CleanDir,
    /// Modified tracked file.
    Restore,
    /// Untracked file.
    Remove,
    /// Staged new file: index and disk.
    RmStaged,
}

impl FileOp {
    fn restores(self) -> bool {
        matches!(self, FileOp::RestoreDir | FileOp::Restore)
    }

    fn is_dir(self) -> bool {
        matches!(self, FileOp::RestoreDir | FileOp::CleanDir)
    }
}

fn plan_file(repo: &Repository, path: &str) -> Result<FileOp> {
    let workdir = repo::require_workdir(repo, "drop")?;
    if workdir.join(path).is_dir() {
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(false).recurse_untracked_dirs(false);
        // libgit2 pathspecs are patterns: `.` matches nothing, so the root
        // takes the whole status.
        if path != "." {
            opts.pathspec(path);
        }
        let has_tracked = !repo.statuses(Some(&mut opts))?.is_empty();
        return Ok(if has_tracked {
            FileOp::RestoreDir
        } else {
            FileOp::CleanDir
        });
    }
    let status = repo
        .status_file(std::path::Path::new(path))
        .with_context(|| format!("'{}' is not tracked by git", path))?;
    Ok(if status.is_wt_new() {
        FileOp::Remove
    } else if status.is_index_new() {
        FileOp::RmStaged
    } else {
        FileOp::Restore
    })
}

/// Confirmation for dropping `plans` (Spec 008): the single-path wording, or
/// a short question with one detail line per path.
fn files_prompt(plans: &[(String, FileOp)]) -> String {
    if let [(path, op)] = plans {
        return match op {
            FileOp::RestoreDir => format!("Discard all changes in `{}`?", path),
            FileOp::Restore => format!("Discard changes to `{}`?", path),
            _ => format!("Delete `{}`?", path),
        };
    }
    let restores = plans.iter().any(|(_, op)| op.restores());
    let deletes = plans.iter().any(|(_, op)| !op.restores());
    let question = match (restores, deletes) {
        (true, false) => "Discard all selected changes?",
        (false, true) => "Delete all selected files?",
        _ => "Discard all selected changes and delete all selected files?",
    };
    let details = plans
        .iter()
        .map(|(path, op)| detail_line(op.restores(), path));
    std::iter::once(question.to_string())
        .chain(details)
        .collect::<Vec<_>>()
        .join("\n")
}

/// One `restore <path>` / `delete <path>` line of a confirmation.
fn detail_line(restores: bool, path: &str) -> String {
    let verb = if restores { "restore" } else { "delete" };
    format!("{} `{}`", verb, path)
}

/// Drop files or directories: restore tracked changes, delete new/untracked
/// entries. One confirmation covers them all.
fn drop_files(repo: &Repository, paths: &[String], skip_confirm: bool) -> Result<()> {
    let workdir = repo::require_workdir(repo, "drop")?;
    let mut plans = paths
        .iter()
        .map(|path| Ok((path.clone(), plan_file(repo, path)?)))
        .collect::<Result<Vec<_>>>()?;
    // A path inside a directory target goes with the directory; its own op
    // would run on what the directory already removed. `.` is the repo root
    // and holds every other path.
    let root = plans.iter().any(|(dir, op)| op.is_dir() && dir == ".");
    let dirs: Vec<String> = plans
        .iter()
        .filter(|(dir, op)| op.is_dir() && dir != ".")
        .map(|(dir, _)| format!("{}/", dir))
        .collect();
    plans.retain(|(path, _)| {
        (!root || path == ".") && !dirs.iter().any(|dir| path.starts_with(dir.as_str()))
    });
    if plans.is_empty() {
        bail!("No file to drop");
    }

    confirm_or_bail(skip_confirm, &files_prompt(&plans))?;

    for (path, op) in &plans {
        match op {
            FileOp::RestoreDir => {
                git::run_git(workdir, &["restore", "--staged", "--worktree", "--", path])?;
                git::run_git(workdir, &["clean", "-fd", "--", path])?;
            }
            FileOp::CleanDir => git::run_git(workdir, &["clean", "-fd", "--", path])?,
            FileOp::Restore => {
                git::run_git(workdir, &["restore", "--staged", "--worktree", "--", path])?
            }
            FileOp::Remove => std::fs::remove_file(workdir.join(path))
                .with_context(|| format!("Failed to delete '{}'", path))?,
            FileOp::RmStaged => git::run_git(workdir, &["rm", "--force", "--", path])?,
        }
        // A directory with tracked changes also loses its untracked entries.
        msg::success(&match op {
            FileOp::RestoreDir => format!("Discarded all changes in `{}`", path),
            FileOp::Restore => format!("Restored `{}`", path),
            _ => format!("Deleted `{}`", path),
        });
    }
    Ok(())
}

/// Drop all local changes: restore tracked files and delete untracked files.
fn drop_all(repo: &Repository, skip_confirm: bool) -> Result<()> {
    let workdir = repo::require_workdir(repo, "drop")?;

    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(false);
    let statuses = repo.statuses(Some(&mut opts))?;

    if statuses.is_empty() {
        bail!("No local changes to discard");
    }

    let details = statuses.iter().map(|entry| {
        let status = entry.status();
        let restores = !status.is_wt_new() && !status.is_index_new();
        detail_line(restores, &String::from_utf8_lossy(entry.path_bytes()))
    });
    let prompt = std::iter::once("Discard all local changes?".to_string())
        .chain(details)
        .collect::<Vec<_>>()
        .join("\n");
    confirm_or_bail(skip_confirm, &prompt)?;

    git::run_git(workdir, &["restore", "--staged", "--worktree", "."])?;
    git::run_git(workdir, &["clean", "-fd"])?;

    msg::success("Discarded all local changes");
    Ok(())
}

/// Drop a single commit from history via interactive rebase.
///
/// A branch the commit was the only commit of survives, empty, parked at the
/// base it built on. Deleting it is `loom drop <branch>`.
fn drop_commit(repo: &Repository, commit_hash: &str, skip_confirm: bool) -> Result<()> {
    let workdir = repo::require_workdir(repo, "drop")?;
    let git_dir = repo.path().to_path_buf();

    let commit_oid = git2::Oid::from_str(commit_hash)?;
    let info = repo::gather_repo_info(repo, false, 1)?;

    // Refuse commits outside the local range (upstream..HEAD). Stale SHAs from
    // before a rewrite still resolve via the reflog, and upstream commits
    // resolve too — dropping those must fail, not pretend to succeed.
    if !info.commits.iter().any(|c| c.oid == commit_oid) {
        let short_hash = git::short_hash(commit_hash);
        let upstream = &info.upstream.label;
        let in_upstream = repo
            .merge_base(info.upstream.merge_base_oid, commit_oid)
            .is_ok_and(|base| base == commit_oid);
        if in_upstream {
            bail!(
                "Commit `{}` is already in the upstream ({})\n\
                 loom only manages the local commits ({}..HEAD)",
                short_hash,
                upstream,
                upstream
            );
        }
        bail!(
            "Commit `{}` is not in the local commits ({}..HEAD)\n\
             If history was rewritten, the SHA may be stale — run `loom` to see the current commits",
            short_hash,
            upstream
        );
    }

    let short_hash = git::short_hash(commit_hash);
    let mut graph = Weave::from_repo_with_info(repo, &info)?;
    let Some(emptied) = graph.drop_commit(commit_oid, EmptiedRefs::Park) else {
        bail!(
            "Cannot drop commit: {} not found in weave graph",
            short_hash
        );
    };

    let summary = repo::commit_subject(&repo.find_commit(commit_oid)?);
    let mut prompt = format!("Drop commit `{}` {}", short_hash, summary);
    if !emptied.is_empty() {
        prompt.push_str(&format!(
            ", leaving {} empty",
            weave::describe_branches(&emptied)
        ));
    }
    prompt.push('?');
    confirm_or_bail(skip_confirm, &prompt)?;

    let ctx = DropContext {
        commit_hash: commit_hash.to_string(),
        emptied_branches: emptied,
    };
    let state = LoomState {
        command: "drop".to_string(),
        rollback: Rollback::default(),
        context: serde_json::to_value(&ctx)?,
        protect: Vec::new(),
    };
    transaction::save(&git_dir, &state)?;

    let todo = graph.to_todo();
    let outcome = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)
        .map_err(|e| transaction::discard_state_after(workdir, &git_dir, e))?;
    match outcome {
        RebaseOutcome::Completed => {
            transaction::delete(&git_dir)?;
            report_dropped(&ctx);
        }
        RebaseOutcome::Stopped => {
            transaction::warn_paused(workdir, "drop");
        }
        RebaseOutcome::Paused => {
            transaction::warn_paused_at_edit(Some("drop"));
        }
    }

    Ok(())
}

/// Resume a `drop commit` operation after a conflict has been resolved.
pub fn after_continue(context: &serde_json::Value) -> Result<()> {
    let ctx: DropContext =
        serde_json::from_value(context.clone()).context("Failed to parse drop resume context")?;
    report_dropped(&ctx);
    Ok(())
}

/// Report the drop, naming the branches it left empty.
fn report_dropped(ctx: &DropContext) {
    let mut message = format!("Dropped commit `{}`", git::short_hash(&ctx.commit_hash));
    if !ctx.emptied_branches.is_empty() {
        message.push_str(&format!(
            "\n{} now empty, at the base",
            weave::describe_branches(&ctx.emptied_branches)
        ));
    }
    msg::success(&message);
}

/// Drop a branch: remove all its commits, unweave merge topology, delete the ref.
fn drop_branch(repo: &Repository, branch_name: &str, skip_confirm: bool) -> Result<()> {
    let workdir = repo::require_workdir(repo, "drop")?;
    let info = &repo::gather_repo_info(repo, false, 1)?;

    let branch_info = info
        .branches
        .iter()
        .find(|b| b.name == branch_name)
        .with_context(|| {
            format!(
                "Branch '{}' is not woven into the integration branch\n\
                 Use `git branch -d {}` to delete it directly",
                branch_name, branch_name
            )
        })?;

    let head_oid = repo::head_oid(repo)?;
    let merge_base_oid = info.upstream.merge_base_oid;

    // An empty branch owns nothing, so there is nothing to confirm losing.
    if branch_info.tip_oid == merge_base_oid {
        git::branch_delete(workdir, branch_name)?;
        msg::success(&dropped_message(branch_name, &DropScope::Empty));
        return Ok(());
    }

    let colocated_branch = info
        .branches
        .iter()
        .find(|b| b.name != branch_name && b.tip_oid == branch_info.tip_oid);

    // Determine if the branch is woven (tip NOT on first-parent line)
    let is_woven = branch_info.tip_oid != head_oid
        && !is_on_first_parent_line(repo, head_oid, merge_base_oid, branch_info.tip_oid)?;

    let mut graph = Weave::from_repo_with_info(repo, info)?;

    // An inner (stacked) branch has no section of its own: its commits belong
    // to the outer branch's section, so only the ref goes and nothing rewrites.
    // A non-woven branch never matches, even though a later section's commits
    // do cover the integration line: its Pick is built first and claims the ref
    // (`assigned_branches`), so no section lists it. It takes the path below.
    if !graph.has_branch_section(branch_name) && graph.is_inner_branch(branch_name) {
        let scope = match graph.inner_branch_keeper(branch_name) {
            Some(outer) => DropScope::KeptBy(outer),
            None => DropScope::KeptInHistory,
        };
        confirm_or_bail(skip_confirm, &drop_prompt(branch_name, &scope))?;
        git::branch_delete(workdir, branch_name)?;
        msg::success(&dropped_message(branch_name, &scope));
        return Ok(());
    }

    let owned = if is_woven {
        Vec::new()
    } else {
        find_owned_commits(
            repo,
            branch_info.tip_oid,
            merge_base_oid,
            &info.branches,
            branch_name,
        )?
    };

    let scope = match colocated_branch {
        Some(keep) => {
            // find_owned_commits hides a sibling at the same tip, so a keeper
            // always means this drop removes nothing.
            debug_assert!(is_woven || owned.is_empty());
            DropScope::KeptBy(&keep.name)
        }
        // A woven drop removes the weave section, not everything down to the
        // merge-base: a branch based on an integration commit shares that
        // commit with the integration line, which survives. No section means
        // the weave does not know this branch — refuse before prompting,
        // since the drop cannot go through either.
        None if is_woven => match graph.branch_drop_size(branch_name) {
            Some(n) => DropScope::Commits(n),
            None => bail!(
                "Cannot drop branch: '{}' not found in weave graph",
                branch_name
            ),
        },
        None => DropScope::Commits(owned.len()),
    };
    confirm_or_bail(skip_confirm, &drop_prompt(branch_name, &scope))?;

    if is_woven {
        let removed = if let Some(keep) = colocated_branch {
            graph.reassign_branch(branch_name, &keep.name)
        } else {
            graph.drop_branch(branch_name)
        };
        if !removed {
            bail!(
                "Cannot drop branch: '{}' not found in weave graph",
                branch_name
            );
        }
    } else if owned.is_empty() {
        // Co-located non-woven: no commits to drop, just delete the ref
        git::branch_delete(workdir, branch_name)?;
        msg::success(&dropped_message(branch_name, &scope));
        return Ok(());
    } else {
        // Non-woven branch: drop each uniquely owned commit individually
        for oid in &owned {
            if graph.drop_commit(*oid, EmptiedRefs::Detach).is_none() {
                bail!(
                    "Cannot drop branch: commit {} not found in weave graph",
                    oid
                );
            }
        }
    }

    let todo = graph.to_todo();
    weave::run_rebase_or_abort(workdir, Some(&graph.base_oid.to_string()), &todo)?;

    // Delete the branch ref (warn on failure — extremely unlikely)
    if let Err(e) = git::branch_delete(workdir, branch_name) {
        msg::warn(&format!(
            "Could not delete branch ref '{}': {} (may have been cleaned up automatically)",
            branch_name, e
        ));
    }

    msg::success(&dropped_message(branch_name, &scope));
    Ok(())
}

/// What a branch drop takes with it.
enum DropScope<'a> {
    /// Commits removed from history.
    Commits(usize),
    /// Nothing removed: a co-located sibling at the same tip, or the outer
    /// branch of a stacked one, keeps them.
    KeptBy(&'a str),
    /// Nothing removed, and no branch to name: the commits stay in the
    /// integration history, inside a section no ref sits at.
    KeptInHistory,
    /// Nothing to remove: the branch sits at the merge-base.
    Empty,
}

/// `1 commit` or `<n> commits`.
fn commits_phrase(count: usize) -> String {
    format!("{} commit{}", count, if count == 1 { "" } else { "s" })
}

/// Confirmation prompt saying how much the drop removes.
fn drop_prompt(branch_name: &str, scope: &DropScope) -> String {
    match scope {
        DropScope::KeptBy(keep) => format!(
            "Drop branch `{}`, keeping its commits on `{}`?",
            branch_name, keep
        ),
        DropScope::KeptInHistory => {
            format!(
                "Drop branch `{}`, keeping its commits in history?",
                branch_name
            )
        }
        // An empty branch is dropped without confirmation, so `Empty` only
        // reaches here if that ever changes.
        DropScope::Empty | DropScope::Commits(0) => format!("Drop branch `{}`?", branch_name),
        DropScope::Commits(n) => format!(
            "Drop branch `{}` and its {}?",
            branch_name,
            commits_phrase(*n)
        ),
    }
}

/// Success message saying how much the drop removed.
fn dropped_message(branch_name: &str, scope: &DropScope) -> String {
    match scope {
        DropScope::Empty => format!("Dropped empty branch `{}`", branch_name),
        DropScope::KeptBy(keep) => format!(
            "Dropped branch `{}`, its commits stay on `{}`",
            branch_name, keep
        ),
        DropScope::KeptInHistory => {
            format!(
                "Dropped branch `{}`, its commits stay in history",
                branch_name
            )
        }
        DropScope::Commits(0) => format!("Dropped branch `{}`", branch_name),
        DropScope::Commits(n) => format!(
            "Dropped branch `{}` and its {}",
            branch_name,
            commits_phrase(*n)
        ),
    }
}

/// Find all commits owned by a branch (from tip to next boundary or merge-base).
///
/// `dropping_branch_name` names the branch being dropped, so branches
/// co-located at the same tip stay hidden and their shared commits are not
/// reported as owned by it.
fn find_owned_commits(
    repo: &Repository,
    branch_tip: git2::Oid,
    merge_base_oid: git2::Oid,
    all_branches: &[repo::BranchInfo],
    dropping_branch_name: &str,
) -> Result<Vec<git2::Oid>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push(branch_tip)?;
    revwalk.hide(merge_base_oid)?;

    // Hide other branch tips that are ancestors of (or co-located with) our tip.
    // Skip the branch being dropped (by name), so co-located branches with the
    // same tip_oid are still hidden — their shared commits are not "owned" by us.
    for other_branch in all_branches {
        if other_branch.name == dropping_branch_name {
            continue;
        }
        if other_branch.tip_oid == branch_tip
            || repo.graph_descendant_of(branch_tip, other_branch.tip_oid)?
        {
            revwalk.hide(other_branch.tip_oid)?;
        }
    }

    revwalk.set_sorting(git2::Sort::TOPOLOGICAL)?;

    let mut oids = Vec::new();
    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        if commit.parent_count() > 1 {
            continue;
        }
        oids.push(oid);
    }

    Ok(oids)
}

#[cfg(test)]
#[path = "drop_test.rs"]
mod tests;
