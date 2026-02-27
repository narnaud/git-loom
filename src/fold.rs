use anyhow::{Context, Result, bail};
use git2::{Repository, StatusOptions};

use crate::git::{self, Target};
use crate::git_commands::{self, git_commit, git_rebase};
use crate::msg;
use crate::weave::{self, Weave};

/// Fold source(s) into a target.
///
/// Dispatches to the appropriate operation based on argument types:
/// - File(s) + Commit → amend files into the commit
/// - Commit + Commit  → fixup source into target (source disappears)
/// - Commit + Branch   → move commit to the branch
pub fn run(args: Vec<String>) -> Result<()> {
    if args.len() < 2 {
        bail!(
            "At least two arguments required (one source + one target)\n\
             Usage: git-loom fold <source>... <target>"
        );
    }

    let repo = git::open_repo()?;

    // Last argument is the target, everything else is a source
    let (source_args, target_arg) = args.split_at(args.len() - 1);
    let target_arg = &target_arg[0];

    // Resolve all arguments
    let resolved_sources: Vec<Target> = source_args
        .iter()
        .map(|s| resolve_fold_arg(&repo, s))
        .collect::<Result<Vec<_>, _>>()?;
    let resolved_target = resolve_fold_arg(&repo, target_arg)?;

    // Classify and dispatch
    match classify(&resolved_sources, &resolved_target)? {
        FoldOp::FilesIntoCommit { files, commit } => fold_files_into_commit(&repo, &files, &commit),
        FoldOp::CommitIntoCommit { source, target } => {
            fold_commit_into_commit(&repo, &source, &target)
        }
        FoldOp::CommitToBranch { commit, branch } => fold_commit_to_branch(&repo, &commit, &branch),
        FoldOp::CommitToUnstaged { commit } => fold_commit_to_unstaged(&repo, &commit),
        FoldOp::CommitFileToUnstaged { commit, path } => {
            fold_commit_file_to_unstaged(&repo, &commit, &path)
        }
        FoldOp::CommitFileToCommit {
            source_commit,
            path,
            target_commit,
        } => fold_commit_file_to_commit(&repo, &source_commit, &path, &target_commit),
    }
}

/// The classified fold operation.
#[derive(Debug)]
enum FoldOp {
    FilesIntoCommit {
        files: Vec<String>,
        commit: String,
    },
    CommitIntoCommit {
        source: String,
        target: String,
    },
    CommitToBranch {
        commit: String,
        branch: String,
    },
    CommitToUnstaged {
        commit: String,
    },
    /// Uncommit a single file from a commit to the working directory.
    CommitFileToUnstaged {
        commit: String,
        path: String,
    },
    /// Move a file's changes from one commit to another.
    CommitFileToCommit {
        source_commit: String,
        path: String,
        target_commit: String,
    },
}

/// Classify resolved arguments into a specific fold operation.
fn classify(sources: &[Target], target: &Target) -> Result<FoldOp> {
    // Check for invalid source types
    for source in sources {
        match source {
            Target::Branch(_) => {
                bail!("Cannot fold a branch\nUse `git loom branch` for branch operations");
            }
            Target::Unstaged => {
                bail!(
                    "Cannot fold unstaged changes\n\
                     Stage files first, or use `git loom fold <file> <commit>` to amend specific files"
                );
            }
            _ => {}
        }
    }

    // Reject CommitFile as target
    if matches!(target, Target::CommitFile { .. }) {
        bail!("Target must be a commit, branch, or unstaged (zz), not a commit file");
    }

    // Classify source types
    let has_files = sources.iter().any(|s| matches!(s, Target::File(_)));
    let has_commits = sources.iter().any(|s| matches!(s, Target::Commit(_)));
    let has_commit_files = sources
        .iter()
        .any(|s| matches!(s, Target::CommitFile { .. }));

    // Reject mixed source types
    if [has_files, has_commits, has_commit_files]
        .iter()
        .filter(|&&x| x)
        .count()
        > 1
    {
        bail!("Cannot mix different source types (files, commits, commit files)");
    }

    // Handle CommitFile sources (file within a commit)
    if has_commit_files {
        if sources.len() > 1 {
            bail!("Only one commit file source is allowed");
        }
        let (commit, path) = match &sources[0] {
            Target::CommitFile { commit, path } => (commit.clone(), path.clone()),
            _ => unreachable!(),
        };

        return match target {
            Target::Unstaged => Ok(FoldOp::CommitFileToUnstaged { commit, path }),
            Target::Commit(hash) => Ok(FoldOp::CommitFileToCommit {
                source_commit: commit,
                path,
                target_commit: hash.clone(),
            }),
            Target::Branch(_) => {
                bail!(
                    "Cannot fold a commit file into a branch\n\
                     Target a specific commit or use `zz` to uncommit"
                )
            }
            Target::File(_) => bail!("Target must be a commit or unstaged (zz), not a file"),
            Target::CommitFile { .. } => unreachable!(),
        };
    }

    // Handle Unstaged target
    if matches!(target, Target::Unstaged) {
        if has_files {
            bail!("Cannot fold files into unstaged — files are already in the working directory");
        }

        if sources.len() > 1 {
            bail!("Only one commit source is allowed");
        }

        let source_hash = match &sources[0] {
            Target::Commit(hash) => hash.clone(),
            _ => unreachable!(),
        };

        return Ok(FoldOp::CommitToUnstaged {
            commit: source_hash,
        });
    }

    if has_files {
        // File(s) + target
        let files: Vec<String> = sources
            .iter()
            .map(|s| match s {
                Target::File(path) => path.clone(),
                _ => unreachable!(),
            })
            .collect();

        match target {
            Target::Commit(hash) => Ok(FoldOp::FilesIntoCommit {
                files,
                commit: hash.clone(),
            }),
            Target::Branch(_) => {
                bail!("Cannot fold files into a branch\nTarget a specific commit")
            }
            Target::File(_) => bail!("Target must be a commit or branch, not a file"),
            _ => unreachable!(),
        }
    } else {
        // Commit(s) + target
        if sources.len() > 1 {
            bail!("Only one commit source is allowed");
        }

        let source_hash = match &sources[0] {
            Target::Commit(hash) => hash.clone(),
            _ => unreachable!(),
        };

        match target {
            Target::Commit(hash) => Ok(FoldOp::CommitIntoCommit {
                source: source_hash,
                target: hash.clone(),
            }),
            Target::Branch(name) => Ok(FoldOp::CommitToBranch {
                commit: source_hash,
                branch: name.clone(),
            }),
            Target::File(_) => bail!("Target must be a commit or branch, not a file"),
            _ => unreachable!(),
        }
    }
}

/// Resolve an argument for the fold command.
///
/// Tries `resolve_target()` first (handles branches, git refs, short IDs).
/// Falls back to checking if the argument is a filesystem path with changes.
fn resolve_fold_arg(repo: &Repository, arg: &str) -> Result<Target> {
    match git::resolve_target(repo, arg) {
        Ok(target) => Ok(target),
        Err(resolve_err) => {
            // Try as a filesystem path with changes
            if let Some(workdir) = repo.workdir() {
                let full_path = workdir.join(arg);
                if full_path.exists() && file_has_changes(repo, arg)? {
                    return Ok(Target::File(arg.to_string()));
                }
            }
            Err(resolve_err)
        }
    }
}

/// Check if a file has staged or unstaged changes.
fn file_has_changes(repo: &Repository, path: &str) -> Result<bool> {
    let mut opts = StatusOptions::new();
    opts.pathspec(path)
        .include_untracked(true)
        .recurse_untracked_dirs(true);

    let statuses = repo.statuses(Some(&mut opts))?;
    Ok(!statuses.is_empty())
}

/// Fold file changes into a commit (Case 1: File(s) + Commit).
fn fold_files_into_commit(repo: &Repository, files: &[String], commit_hash: &str) -> Result<()> {
    let workdir = git::require_workdir(repo, "fold")?;

    // Validate all files have changes
    for file in files {
        if !file_has_changes(repo, file)? {
            bail!("File '{}' has no changes to fold", file);
        }
    }

    let head_oid = git::head_oid(repo)?;
    let target_oid = git2::Oid::from_str(commit_hash)?;
    let is_head = head_oid == target_oid;

    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();

    if is_head {
        // Simple case: stage files and amend HEAD
        git_commit::stage_files(workdir, &file_refs)?;
        if let Err(e) = git_commit::amend_no_edit(workdir) {
            let _ = git_commands::unstage_files(workdir, &file_refs);
            return Err(e);
        }
    } else {
        // Edit+continue: stage files, save patch, unstage, clean working tree,
        // then rebase with edit at target.
        git_commit::stage_files(workdir, &file_refs)?;
        let patch = git_commands::diff_cached(workdir)?;
        git_commands::unstage_files(workdir, &file_refs)?;

        // Discard working-tree changes for the folded files. Their diff is
        // captured in `patch` and will be amended into the target commit.
        // Without this, --autostash stashes (then pops) these changes after
        // the rebase rewrites history, causing spurious merge conflicts.
        git_commands::restore_files_to_head(workdir, &file_refs)?;

        let mut graph = Weave::from_repo(repo)?;
        graph.edit_commit(target_oid);

        let todo = graph.to_todo();
        if let Err(e) = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo) {
            // Rebase failed before we could amend — restore working-tree changes.
            let _ = git_commands::apply_patch(workdir, &patch);
            return Err(e);
        }

        // Now paused at the target commit — apply patch, stage, amend
        if let Err(e) = git_commands::apply_patch(workdir, &patch)
            .and_then(|()| git_commit::stage_files(workdir, &file_refs))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            let _ = git_commands::apply_patch(workdir, &patch);
            return Err(e);
        }

        if let Err(e) = git_rebase::continue_rebase(workdir) {
            // continue_rebase already aborts on failure — restore changes.
            let _ = git_commands::apply_patch(workdir, &patch);
            return Err(e);
        }
    }

    msg::success(&format!(
        "Folded {} file(s) into `{}`",
        files.len(),
        git_commands::short_hash(commit_hash)
    ));

    Ok(())
}

/// Fold a commit into another commit (Case 2: Commit + Commit → Fixup).
fn fold_commit_into_commit(repo: &Repository, source_hash: &str, target_hash: &str) -> Result<()> {
    let workdir = git::require_workdir(repo, "fold")?;

    // Validate source is a descendant of target (source is newer)
    let source_oid = git2::Oid::from_str(source_hash)?;
    let target_oid = git2::Oid::from_str(target_hash)?;

    if source_oid == target_oid {
        bail!("Source and target are the same commit");
    }

    if !repo.graph_descendant_of(source_oid, target_oid)? {
        bail!("Source commit must be newer than target commit");
    }

    let mut graph = Weave::from_repo(repo)?;
    graph.fixup_commit(source_oid, target_oid);

    let todo = graph.to_todo();
    weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;

    msg::success(&format!(
        "Folded `{}` into `{}`",
        git_commands::short_hash(source_hash),
        git_commands::short_hash(target_hash)
    ));

    Ok(())
}

/// Move a commit to a branch (Case 3: Commit + Branch → Move).
fn fold_commit_to_branch(repo: &Repository, commit_hash: &str, branch_name: &str) -> Result<()> {
    move_commit_to_branch(repo, commit_hash, branch_name)?;

    msg::success(&format!(
        "Moved `{}` to branch `{}`",
        git_commands::short_hash(commit_hash),
        branch_name
    ));

    Ok(())
}

/// Move a commit to the tip of a branch using Weave.
///
/// The caller is responsible for ensuring the working tree is in an appropriate
/// state. The rebase uses `--autostash` to handle any remaining uncommitted changes.
///
/// Used by both fold (commit+branch) and commit commands.
pub fn move_commit_to_branch(
    repo: &Repository,
    commit_hash: &str,
    branch_name: &str,
) -> Result<()> {
    let workdir = git::require_workdir(repo, "fold")?;

    let commit_oid = git2::Oid::from_str(commit_hash)?;

    let mut graph = Weave::from_repo(repo)?;
    graph.move_commit(commit_oid, branch_name);

    let todo = graph.to_todo();
    weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;

    Ok(())
}

/// Uncommit a single file from a commit to the working directory.
///
/// Removes the file's changes from the commit and places them in the working
/// directory as unstaged modifications. The commit itself is preserved (minus
/// the file's changes).
fn fold_commit_file_to_unstaged(repo: &Repository, commit_hash: &str, path: &str) -> Result<()> {
    let workdir = git::require_workdir(repo, "fold")?;

    let head_oid = git::head_oid(repo)?;
    let target_oid = git2::Oid::from_str(commit_hash)?;
    let is_head = head_oid == target_oid;

    // Get the file's diff from the commit
    let file_diff = git_commands::diff_commit_file(workdir, commit_hash, path)?;
    if file_diff.is_empty() {
        bail!(
            "File '{}' has no changes in commit {}",
            path,
            git_commands::short_hash(commit_hash)
        );
    }

    if is_head {
        // Simple case: reverse the file's changes, amend HEAD, then re-apply
        let saved_head = head_oid.to_string();
        git_commands::apply_patch_reverse(workdir, &file_diff)?;
        git_commit::stage_path(workdir, path)?;
        git_commit::amend_no_edit(workdir)?;
        if let Err(e) = git_commands::apply_patch(workdir, &file_diff) {
            let _ = git_commit::reset_hard(workdir, &saved_head);
            return Err(e).context("Failed to uncommit file, operation rolled back");
        }
    } else {
        // Non-HEAD: edit+continue pattern with save-head rollback
        let saved_head = head_oid.to_string();

        let mut graph = Weave::from_repo(repo)?;
        graph.edit_commit(target_oid);

        let todo = graph.to_todo();
        weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;

        // Paused at target — reverse the file, stage, amend
        if let Err(e) = git_commands::apply_patch_reverse(workdir, &file_diff)
            .and_then(|()| git_commit::stage_path(workdir, path))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            return Err(e);
        }

        // Continue the rebase
        git_rebase::continue_rebase(workdir)?;

        // Re-apply changes to working tree
        if let Err(e) = git_commands::apply_patch(workdir, &file_diff) {
            let _ = git_commit::reset_hard(workdir, &saved_head);
            return Err(e).context("Failed to uncommit file, operation rolled back");
        }
    }

    msg::success(&format!(
        "Uncommitted `{}` from `{}` to working directory",
        path,
        git_commands::short_hash(commit_hash)
    ));

    Ok(())
}

/// Move a file's changes from one commit to another.
///
/// Removes the file's changes from the source commit and adds them to the
/// target commit. Both commits are rewritten.
fn fold_commit_file_to_commit(
    repo: &Repository,
    source_hash: &str,
    path: &str,
    target_hash: &str,
) -> Result<()> {
    let workdir = git::require_workdir(repo, "fold")?;

    let source_oid = git2::Oid::from_str(source_hash)?;
    let target_oid = git2::Oid::from_str(target_hash)?;

    if source_oid == target_oid {
        bail!("Source and target are the same commit");
    }

    // Get the file's diff from the source commit
    let file_diff = git_commands::diff_commit_file(workdir, source_hash, path)?;
    if file_diff.is_empty() {
        bail!(
            "File '{}' has no changes in commit {}",
            path,
            git_commands::short_hash(source_hash)
        );
    }

    // Check direction: is source a descendant of target (source is newer)?
    let source_is_newer = repo.graph_descendant_of(source_oid, target_oid)?;

    if source_is_newer {
        // Two-phase approach with edit+continue and rollback.
        // When source is newer than target, a single rebase can't do both
        // edits: adding the file to target (older, picked first) would
        // conflict when source (newer) is replayed — source still has the file.
        //   Phase 1: Remove the file from source via edit+continue.
        //   Phase 2: Add the file to target via edit+continue.
        // On phase 2 failure, roll back to pre-phase-1 state.
        let saved_head = git::head_oid(repo)?.to_string();
        let saved_refs = git::snapshot_branch_refs(repo)?;

        // Create a temp branch on target so --update-refs tracks its new OID
        // through the phase 1 rebase. Created BEFORE from_repo() would include
        // it in the graph, but after the graph snapshot — we create it right
        // before the rebase so git's --update-refs picks it up.
        let tmp_branch = "_loom-fold-target";

        // Phase 1: edit at source, remove file, continue
        let mut graph = Weave::from_repo(repo)?;
        graph.edit_commit(source_oid);
        let todo = graph.to_todo();

        // Create temp branch AFTER from_repo to avoid polluting the Weave graph
        git_commands::run_git(workdir, &["branch", "-f", tmp_branch, target_hash])?;

        let phase1_rebase = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo);
        if let Err(e) = phase1_rebase {
            let _ = git_commands::run_git(workdir, &["branch", "-D", tmp_branch]);
            return Err(e);
        }

        if let Err(e) = git_commands::apply_patch_reverse(workdir, &file_diff)
            .and_then(|()| git_commit::stage_path(workdir, path))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            let _ = git_commands::run_git(workdir, &["branch", "-D", tmp_branch]);
            return Err(e);
        }
        if let Err(e) = git_rebase::continue_rebase(workdir) {
            let _ = git_commands::run_git(workdir, &["branch", "-D", tmp_branch]);
            return Err(e);
        }

        // Phase 2: Resolve the target's new OID via the temp branch
        let new_target_hash = git_commands::run_git_stdout(workdir, &["rev-parse", tmp_branch])?;
        let new_target_hash = new_target_hash.trim().to_string();
        let _ = git_commands::run_git(workdir, &["branch", "-D", tmp_branch]);
        let new_target_oid = git2::Oid::from_str(&new_target_hash)?;

        let repo2 = git2::Repository::discover(workdir)?;
        let mut graph = Weave::from_repo(&repo2)?;
        graph.edit_commit(new_target_oid);
        let todo = graph.to_todo();

        if let Err(e) = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo) {
            let _ = git_commit::reset_hard(workdir, &saved_head);
            let _ = git::restore_branch_refs(workdir, &saved_refs);
            return Err(e);
        }

        if let Err(e) = git_commands::apply_patch(workdir, &file_diff)
            .and_then(|()| git_commit::stage_path(workdir, path))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            let _ = git_commit::reset_hard(workdir, &saved_head);
            let _ = git::restore_branch_refs(workdir, &saved_refs);
            return Err(e);
        }

        if let Err(e) = git_rebase::continue_rebase(workdir) {
            let _ = git_commit::reset_hard(workdir, &saved_head);
            let _ = git::restore_branch_refs(workdir, &saved_refs);
            return Err(e);
        }
    } else {
        // Source is older than target: single rebase with two edit pauses.
        // Source is picked first (older), target second (newer). Removing
        // the file from source before target is replayed avoids conflicts.
        let mut graph = Weave::from_repo(repo)?;
        graph.edit_commit(source_oid);
        graph.edit_commit(target_oid);

        let todo = graph.to_todo();
        weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;

        // First pause: at source — remove file
        if let Err(e) = git_commands::apply_patch_reverse(workdir, &file_diff)
            .and_then(|()| git_commit::stage_path(workdir, path))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            return Err(e);
        }

        // Continue to second pause: at target
        git_rebase::continue_rebase(workdir)?;

        // Second pause: at target — add file
        if let Err(e) = git_commands::apply_patch(workdir, &file_diff)
            .and_then(|()| git_commit::stage_path(workdir, path))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            return Err(e);
        }

        git_rebase::continue_rebase(workdir)?;
    }

    msg::success(&format!(
        "Moved `{}` from `{}` to `{}`",
        path,
        git_commands::short_hash(source_hash),
        git_commands::short_hash(target_hash)
    ));

    Ok(())
}

/// Uncommit a commit to the working directory (Case 4: Commit + Unstaged).
///
/// Removes the commit from history and places its changes in the working
/// directory as unstaged modifications.
fn fold_commit_to_unstaged(repo: &Repository, commit_hash: &str) -> Result<()> {
    let workdir = git::require_workdir(repo, "fold")?;

    let head_oid = git::head_oid(repo)?;
    let target_oid = git2::Oid::from_str(commit_hash)?;
    let is_head = head_oid == target_oid;

    if is_head {
        // Simple case: mixed reset to HEAD~1
        git_commit::reset_mixed(workdir, "HEAD~1")?;
    } else {
        // Non-HEAD: capture the diff, drop the commit, then apply the diff
        let diff = git_commands::diff_commit(workdir, commit_hash)?;
        let saved_head = head_oid.to_string();

        let mut graph = Weave::from_repo(repo)?;
        graph.drop_commit(target_oid);

        let todo = graph.to_todo();
        weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;

        if !diff.is_empty()
            && let Err(e) = git_commands::apply_patch(workdir, &diff)
        {
            let _ = git_commit::reset_hard(workdir, &saved_head);
            return Err(e)
                .context("Failed to apply changes to working directory, operation rolled back");
        }
    }

    msg::success(&format!(
        "Uncommitted `{}` to working directory",
        git_commands::short_hash(commit_hash)
    ));

    Ok(())
}

#[cfg(test)]
#[path = "fold_test.rs"]
mod tests;
