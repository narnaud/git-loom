use anyhow::{Context, Result, bail};
use git2::{Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::commit;
use crate::git::{self, Target, TargetKind};
use crate::git_commands::{self, git_branch, git_commit, git_rebase};
use crate::msg;
use crate::transaction::{self, LoomState, Rollback};
use crate::weave::{self, RebaseOutcome, Weave};

#[derive(Serialize, Deserialize)]
#[serde(tag = "op")]
enum FoldVariant {
    FilesIntoCommit {
        original_commit_hash: String,
        files_count: usize,
        saved_staged: String,
    },
    CommitIntoCommit {
        source_hash: String,
        target_hash: String,
    },
    CommitToBranch {
        commit_hash: String,
        branch_name: String,
    },
    CommitToUnstaged {
        commit_hash: String,
        diff: String,
    },
}

/// Temporary branch used to track a commit's new OID through a rebase.
const TRACK_BRANCH: &str = "_loom-track";

/// Fold source(s) into a target.
///
/// Dispatches to the appropriate operation based on argument types:
/// - File(s) + Commit → amend files into the commit
/// - Commit + Commit  → fixup source into target (source disappears)
/// - Commit + Branch   → move commit to the branch
///
/// With `--create` (`-c`): create a new branch and move the source commit into it.
pub fn run(create: bool, args: Vec<String>) -> Result<()> {
    if args.is_empty() {
        bail!(
            "At least one argument required\n\
             Usage: git-loom fold [<source>...] <target>"
        );
    }

    let repo = git::open_repo()?;

    if create {
        return run_create(&repo, &args);
    }

    // Single argument: fold staged files into the target commit
    if args.len() == 1 {
        return run_staged(&repo, &args[0]);
    }

    // Last argument is the target, everything else is a source
    let (source_args, target_arg) = args.split_at(args.len() - 1);
    let target_arg = &target_arg[0];

    // If any source is "zz", expand to all changed files (zz takes precedence)
    let source_args = if source_args.iter().any(|s| s == "zz") {
        let files = collect_changed_files(&repo)?;
        if files.is_empty() {
            bail!("No changes to fold — working tree is clean");
        }
        files
    } else {
        source_args.to_vec()
    };

    // Resolve all arguments
    let resolved_sources: Vec<Target> = source_args
        .iter()
        .map(|s| {
            git::resolve_arg(
                &repo,
                s,
                &[
                    TargetKind::Commit,
                    TargetKind::CommitFile,
                    TargetKind::File,
                    TargetKind::Unstaged,
                ],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let resolved_target = git::resolve_arg(
        &repo,
        target_arg,
        &[
            TargetKind::Branch,
            TargetKind::Commit,
            TargetKind::CommitFile,
            TargetKind::File,
            TargetKind::Unstaged,
        ],
    )?;

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

/// Create a new branch and move the source commit into it.
///
/// `args` must be exactly `[<commit>, <new-branch-name>]`.
/// The branch is created at the merge-base, then the commit is moved to it
/// using the same Weave machinery as the normal commit-to-branch fold.
fn run_create(repo: &Repository, args: &[String]) -> Result<()> {
    if args.len() != 2 {
        bail!(
            "fold --create requires exactly one commit and one new branch name\n\
             Usage: loom fold -c <commit> <new-branch>"
        );
    }

    let (source_arg, branch_name) = (&args[0], &args[1]);
    let workdir = git::require_workdir(repo, "fold")?;

    // Resolve source — must be a commit
    let source = git::resolve_arg(repo, source_arg, &[TargetKind::Commit])?;
    let commit_hash = match source {
        Target::Commit(hash) => hash,
        _ => unreachable!(),
    };

    git_branch::validate_name(branch_name)?;

    // If the branch already exists, warn and fall through to a normal move.
    let branch_exists = repo
        .find_branch(branch_name, git2::BranchType::Local)
        .is_ok();
    if branch_exists {
        msg::warn(&format!(
            "Branch `{}` already exists — moving commit to it",
            branch_name
        ));
        return fold_commit_to_branch(repo, &commit_hash, branch_name);
    }

    // Create the branch at the merge-base so it has no commits of its own yet;
    // move_commit_to_branch will add a section for it in the Weave graph.
    let info = git::gather_repo_info(repo, false, 1).ok();
    let base_hash = match &info {
        Some(info) => info.upstream.merge_base_oid.to_string(),
        None => bail!(
            "Cannot create branch: no upstream tracking branch configured\n\
             Use 'loom branch <name> -t <commit>' instead"
        ),
    };

    create_branch_and_move(workdir, repo, &commit_hash, branch_name, &base_hash)
}

/// Create a branch at `base_hash`, then move `commit_hash` to it.
fn create_branch_and_move(
    workdir: &Path,
    repo: &Repository,
    commit_hash: &str,
    branch_name: &str,
    base_hash: &str,
) -> Result<()> {
    git_branch::create(workdir, branch_name, base_hash)?;

    match move_commit_to_branch(repo, commit_hash, branch_name) {
        Ok(RebaseOutcome::Completed) => {}
        Ok(RebaseOutcome::Conflicted) => {
            let _ = git_rebase::abort(workdir);
            let _ = git_branch::delete(workdir, branch_name);
            bail!("Rebase failed with conflicts — aborted");
        }
        Err(e) => {
            let _ = git_branch::delete(workdir, branch_name);
            return Err(e);
        }
    }

    let new_hash = git_commands::rev_parse(workdir, branch_name)?;
    msg::success(&format!(
        "Created branch `{}` and moved `{}` to it (now `{}`)",
        branch_name,
        git_commands::short_hash(commit_hash),
        git_commands::short_hash(&new_hash)
    ));

    Ok(())
}

/// Fold currently staged files into a target commit.
///
/// Single-argument form: `loom fold <target>`. The target must resolve to a
/// commit. If nothing is staged, bails with the same message as `loom commit`.
fn run_staged(repo: &Repository, target_arg: &str) -> Result<()> {
    let resolved = git::resolve_arg(repo, target_arg, &[TargetKind::Commit])?;
    let commit_hash = match resolved {
        Target::Commit(hash) => hash,
        _ => unreachable!(),
    };

    commit::verify_has_staged_changes(repo)?;

    let staged = git::get_staged_files(repo)?;
    fold_files_into_commit(repo, &staged, &commit_hash)
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
        if matches!(source, Target::Branch(_)) {
            bail!("Cannot fold a branch\nUse `git loom branch` for branch operations");
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

/// Collect all file paths with staged or unstaged changes.
fn collect_changed_files(repo: &Repository) -> Result<Vec<String>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    let mut paths = Vec::new();
    for entry in statuses.iter() {
        if let Some(path) = entry.path() {
            paths.push(path.to_string());
        }
    }
    Ok(paths)
}

/// Fold file changes into a commit (Case 1: File(s) + Commit).
fn fold_files_into_commit(repo: &Repository, files: &[String], commit_hash: &str) -> Result<()> {
    let workdir = git::require_workdir(repo, "fold")?;

    // Validate all files have changes
    for file in files {
        if !git::path_has_changes(repo, file)? {
            bail!("File '{}' has no changes to fold", file);
        }
    }

    let head_oid = git::head_oid(repo)?;
    let target_oid = git2::Oid::from_str(commit_hash)?;
    let is_head = head_oid == target_oid;

    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();

    // Save and unstage any pre-existing staged files not in our target list,
    // so they don't accidentally end up in this commit/amend.
    let staged = git::get_staged_files(repo)?;
    let other_staged: Vec<&str> = staged
        .iter()
        .filter(|f| !file_refs.contains(&f.as_str()))
        .map(|s| s.as_str())
        .collect();
    let saved_staged = if other_staged.is_empty() {
        String::new()
    } else {
        let patch = git_commands::diff_cached_files(workdir, &other_staged)?;
        git_commands::unstage_files(workdir, &other_staged)?;
        patch
    };

    let new_hash;

    if is_head {
        // Simple case: stage files and amend HEAD
        git_commit::stage_files(workdir, &file_refs)?;
        if let Err(e) = git_commit::amend_no_edit(workdir) {
            let _ = git_commands::unstage_files(workdir, &file_refs);
            let _ = git_commands::restore_staged_patch(workdir, &saved_staged);
            return Err(e);
        }
        git_commands::restore_staged_patch(workdir, &saved_staged)?;
        new_hash = git_commands::rev_parse(workdir, "HEAD")?;
    } else {
        // Create a fixup commit on HEAD with only the changed files, then
        // use the weave machinery to squash it into the target commit.
        // This avoids fragile file-restoration that can fail on Windows
        // (os error 5) when files are locked by editors or indexers.
        let target_commit = repo.find_commit(target_oid)?;
        let subject = target_commit.summary().unwrap_or("fixup");
        let message = format!("fixup! {}", subject);

        git_commit::stage_files(workdir, &file_refs)?;
        if let Err(e) = git_commit::commit(workdir, &message) {
            let _ = git_commands::unstage_files(workdir, &file_refs);
            let _ = git_commands::restore_staged_patch(workdir, &saved_staged);
            return Err(e);
        }

        // Re-read state after creating the fixup commit
        let fixup_hash = git_commands::rev_parse(workdir, "HEAD")?;
        let fixup_oid = git2::Oid::from_str(&fixup_hash)?;

        let repo = git2::Repository::discover(workdir)?;
        let mut graph = Weave::from_repo(&repo)?;
        graph.fixup_commit(fixup_oid, target_oid)?;

        // Track target commit through the rebase via a temp branch.
        // The branch must exist before the rebase AND have an update-ref
        // line in the todo so git keeps it in sync.
        git_branch::force_create(workdir, TRACK_BRANCH, commit_hash)?;
        graph.track_commit(target_oid, TRACK_BRANCH);

        // Save LoomState before the rebase.
        let git_dir = repo.path().to_path_buf();
        let fold_ctx = serde_json::to_value(FoldVariant::FilesIntoCommit {
            original_commit_hash: commit_hash.to_string(),
            files_count: files.len(),
            saved_staged: saved_staged.clone(),
        })?;
        let loom_state = LoomState {
            command: "fold".to_string(),
            rollback: Rollback {
                saved_staged_patch: saved_staged.clone(),
                delete_branches: vec![TRACK_BRANCH.to_string()],
                ..Default::default()
            },
            context: fold_ctx,
        };
        transaction::save(&git_dir, &loom_state)?;

        let todo = graph.to_todo();
        match weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)? {
            RebaseOutcome::Completed => {
                transaction::delete(&git_dir)?;
                git_commands::restore_staged_patch(workdir, &saved_staged)?;
                new_hash = git_commands::rev_parse(workdir, TRACK_BRANCH)?;
                let _ = git_branch::delete(workdir, TRACK_BRANCH);
            }
            RebaseOutcome::Conflicted => {
                transaction::warn_conflict_paused("fold");
                return Ok(());
            }
        }
    }

    msg::success(&format!(
        "Folded {} file(s) into `{}` (now `{}`)",
        files.len(),
        git_commands::short_hash(commit_hash),
        git_commands::short_hash(&new_hash)
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
    graph.fixup_commit(source_oid, target_oid)?;

    // Track target commit through the rebase via a temp branch.
    git_branch::force_create(workdir, TRACK_BRANCH, target_hash)?;
    graph.track_commit(target_oid, TRACK_BRANCH);

    // Save LoomState before the rebase.
    let git_dir = repo.path().to_path_buf();
    let fold_ctx = serde_json::to_value(FoldVariant::CommitIntoCommit {
        source_hash: source_hash.to_string(),
        target_hash: target_hash.to_string(),
    })?;
    let loom_state = LoomState {
        command: "fold".to_string(),
        rollback: Rollback {
            delete_branches: vec![TRACK_BRANCH.to_string()],
            ..Default::default()
        },
        context: fold_ctx,
    };
    transaction::save(&git_dir, &loom_state)?;

    let todo = graph.to_todo();
    match weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)? {
        RebaseOutcome::Completed => {
            transaction::delete(&git_dir)?;
            let new_hash = git_commands::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git_branch::delete(workdir, TRACK_BRANCH);
            msg::success(&format!(
                "Folded `{}` into `{}` (now `{}`)",
                git_commands::short_hash(source_hash),
                git_commands::short_hash(target_hash),
                git_commands::short_hash(&new_hash)
            ));
        }
        RebaseOutcome::Conflicted => {
            transaction::warn_conflict_paused("fold");
        }
    }

    Ok(())
}

/// Move a commit to a branch (Case 3: Commit + Branch → Move).
fn fold_commit_to_branch(repo: &Repository, commit_hash: &str, branch_name: &str) -> Result<()> {
    let workdir = git::require_workdir(repo, "fold")?;
    let git_dir = repo.path().to_path_buf();

    let ctx = serde_json::to_value(FoldVariant::CommitToBranch {
        commit_hash: commit_hash.to_string(),
        branch_name: branch_name.to_string(),
    })?;
    let state = LoomState {
        command: "fold".to_string(),
        rollback: Rollback::default(),
        context: ctx,
    };
    transaction::save(&git_dir, &state)?;

    match move_commit_to_branch(repo, commit_hash, branch_name)? {
        RebaseOutcome::Completed => {
            transaction::delete(&git_dir)?;
            let new_hash = git_commands::rev_parse(workdir, branch_name)?;
            msg::success(&format!(
                "Moved `{}` to branch `{}` (now `{}`)",
                git_commands::short_hash(commit_hash),
                branch_name,
                git_commands::short_hash(&new_hash)
            ));
        }
        RebaseOutcome::Conflicted => {
            transaction::warn_conflict_paused("fold");
        }
    }

    Ok(())
}

/// Move a commit to the tip of a branch using Weave.
///
/// Returns `RebaseOutcome` — callers are responsible for building and saving
/// their own `LoomState` before calling this function.
pub fn move_commit_to_branch(
    repo: &Repository,
    commit_hash: &str,
    branch_name: &str,
) -> Result<RebaseOutcome> {
    let workdir = git::require_workdir(repo, "fold")?;

    let commit_oid = git2::Oid::from_str(commit_hash)?;

    let mut graph = Weave::from_repo(repo)?;

    // If the target branch has no section in the Weave graph, create one.
    // This happens when the branch is at the merge-base (no commits of its
    // own) — either it was never woven, or a previous rebase dropped the
    // degenerate merge (merging two identical commits is a no-op for git).
    // Same pattern as commit.rs for empty branches.
    let has_section = graph
        .branch_sections
        .iter()
        .any(|s| s.label == branch_name || s.branch_names.contains(&branch_name.to_string()));
    if !has_section {
        // Only allow creating a synthetic section for branches that are
        // at the merge-base (empty) or don't exist yet. Reject branches
        // that have diverged — they are out of scope.
        if let Ok(branch) = repo.find_branch(branch_name, git2::BranchType::Local) {
            let branch_oid = branch
                .get()
                .peel_to_commit()
                .map(|c| c.id())
                .unwrap_or(git2::Oid::zero());
            if branch_oid != graph.base_oid {
                bail!(
                    "Branch '{}' exists but is not part of the current integration scope.\n\
                     Use `loom branch merge {}` to weave it first.",
                    branch_name,
                    branch_name
                );
            }
        }
        graph.add_branch_section(
            branch_name.to_string(),
            vec![branch_name.to_string()],
            vec![],
            "onto".to_string(),
        );
        graph.add_merge(branch_name.to_string(), None, None);
    }

    graph.move_commit(commit_oid, branch_name)?;

    let todo = graph.to_todo();
    weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)
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

    let new_hash;

    if is_head {
        // Simple case: reverse the file's changes, amend HEAD, then re-apply
        let saved_head = head_oid.to_string();
        git_commands::apply_patch_reverse(workdir, &file_diff)?;
        git_commit::stage_path(workdir, path)?;
        git_commit::amend_no_edit(workdir)?;
        new_hash = git_commands::rev_parse(workdir, "HEAD")?;
        if let Err(e) = git_commands::apply_patch(workdir, &file_diff) {
            let _ = git_commit::reset_hard(workdir, &saved_head);
            return Err(e).context("Failed to uncommit file, operation rolled back");
        }
    } else {
        // Non-HEAD: edit+continue pattern with save-head rollback
        let saved_head = head_oid.to_string();
        let saved_refs = git::snapshot_branch_refs(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        graph.edit_commit(target_oid);

        let todo = graph.to_todo();
        weave::run_rebase_or_abort(workdir, Some(&graph.base_oid.to_string()), &todo)?;

        // Paused at target — reverse the file, stage, amend
        if let Err(e) = git_commands::apply_patch_reverse(workdir, &file_diff)
            .and_then(|()| git_commit::stage_path(workdir, path))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            return Err(e);
        }

        // Capture new hash after amend, before continue moves HEAD
        new_hash = git_commands::rev_parse(workdir, "HEAD")?;

        // Continue the rebase
        git_rebase::continue_rebase_or_abort(workdir)?;

        // Re-apply changes to working tree
        if let Err(e) = git_commands::apply_patch(workdir, &file_diff) {
            let _ = git_commit::reset_hard(workdir, &saved_head);
            let _ = git::restore_branch_refs(workdir, &saved_refs);
            return Err(e).context("Failed to uncommit file, operation rolled back");
        }
    }

    msg::success(&format!(
        "Uncommitted `{}` from `{}` (now `{}`) to working directory",
        path,
        git_commands::short_hash(commit_hash),
        git_commands::short_hash(&new_hash)
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

    let new_source_hash;
    let new_target_hash;

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

        // Phase 1: edit at source, remove file, continue
        let mut graph = Weave::from_repo(repo)?;
        graph.edit_commit(source_oid);
        let todo = graph.to_todo();

        // Create temp branch AFTER from_repo to avoid polluting the Weave graph
        git_branch::force_create(workdir, TRACK_BRANCH, target_hash)?;

        let phase1_rebase =
            weave::run_rebase_or_abort(workdir, Some(&graph.base_oid.to_string()), &todo);
        if let Err(e) = phase1_rebase {
            let _ = git_branch::delete(workdir, TRACK_BRANCH);
            return Err(e);
        }

        if let Err(e) = git_commands::apply_patch_reverse(workdir, &file_diff)
            .and_then(|()| git_commit::stage_path(workdir, path))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            let _ = git_branch::delete(workdir, TRACK_BRANCH);
            return Err(e);
        }

        // Capture source new hash after amend, before continue moves HEAD.
        // This hash will be tracked through phase 2 via a temp branch.
        let phase1_source_hash = git_commands::rev_parse(workdir, "HEAD")?;

        if let Err(e) = git_rebase::continue_rebase_or_abort(workdir) {
            let _ = git_branch::delete(workdir, TRACK_BRANCH);
            return Err(e);
        }

        // Phase 2: Resolve the target's new OID via the temp branch.
        let phase2_target_hash = git_commands::rev_parse(workdir, TRACK_BRANCH)?;
        let _ = git_branch::delete(workdir, TRACK_BRANCH);
        let phase2_target_oid = git2::Oid::from_str(&phase2_target_hash)?;

        let repo2 = git2::Repository::discover(workdir)?;
        let mut graph = Weave::from_repo(&repo2)?;
        graph.edit_commit(phase2_target_oid);

        // Track source through phase 2 — it will be rewritten when the
        // graph is replayed from base_oid.
        let phase1_source_oid = git2::Oid::from_str(&phase1_source_hash)?;
        git_branch::force_create(workdir, TRACK_BRANCH, &phase1_source_hash)?;
        graph.track_commit(phase1_source_oid, TRACK_BRANCH);

        let todo = graph.to_todo();

        if let Err(e) =
            weave::run_rebase_or_abort(workdir, Some(&graph.base_oid.to_string()), &todo)
        {
            let _ = git_branch::delete(workdir, TRACK_BRANCH);
            let _ = git_commit::reset_hard(workdir, &saved_head);
            let _ = git::restore_branch_refs(workdir, &saved_refs);
            return Err(e);
        }

        if let Err(e) = git_commands::apply_patch(workdir, &file_diff)
            .and_then(|()| git_commit::stage_path(workdir, path))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            let _ = git_branch::delete(workdir, TRACK_BRANCH);
            let _ = git_commit::reset_hard(workdir, &saved_head);
            let _ = git::restore_branch_refs(workdir, &saved_refs);
            return Err(e);
        }

        // Capture target new hash after amend, before continue moves HEAD
        new_target_hash = git_commands::rev_parse(workdir, "HEAD")?;

        if let Err(e) = git_rebase::continue_rebase_or_abort(workdir) {
            let _ = git_branch::delete(workdir, TRACK_BRANCH);
            let _ = git_commit::reset_hard(workdir, &saved_head);
            let _ = git::restore_branch_refs(workdir, &saved_refs);
            return Err(e);
        }

        // Resolve source's final hash after phase 2
        new_source_hash = git_commands::rev_parse(workdir, TRACK_BRANCH)?;
        let _ = git_branch::delete(workdir, TRACK_BRANCH);
    } else {
        // Source is older than target: single rebase with two edit pauses.
        // Source is picked first (older), target second (newer). Removing
        // the file from source before target is replayed avoids conflicts.
        let mut graph = Weave::from_repo(repo)?;
        graph.edit_commit(source_oid);
        graph.edit_commit(target_oid);

        let todo = graph.to_todo();
        weave::run_rebase_or_abort(workdir, Some(&graph.base_oid.to_string()), &todo)?;

        // First pause: at source — remove file
        if let Err(e) = git_commands::apply_patch_reverse(workdir, &file_diff)
            .and_then(|()| git_commit::stage_path(workdir, path))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            return Err(e);
        }

        // Capture source new hash after amend, before continue moves HEAD
        new_source_hash = git_commands::rev_parse(workdir, "HEAD")?;

        // Continue to second pause: at target
        git_rebase::continue_rebase_or_abort(workdir)?;

        // Second pause: at target — add file
        if let Err(e) = git_commands::apply_patch(workdir, &file_diff)
            .and_then(|()| git_commit::stage_path(workdir, path))
            .and_then(|()| git_commit::amend_no_edit(workdir))
        {
            let _ = git_rebase::abort(workdir);
            return Err(e);
        }

        // Capture target new hash after amend, before continue moves HEAD
        new_target_hash = git_commands::rev_parse(workdir, "HEAD")?;

        git_rebase::continue_rebase_or_abort(workdir)?;
    }

    msg::success(&format!(
        "Moved `{}` from `{}` (now `{}`) to `{}` (now `{}`)",
        path,
        git_commands::short_hash(source_hash),
        git_commands::short_hash(&new_source_hash),
        git_commands::short_hash(target_hash),
        git_commands::short_hash(&new_target_hash)
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
        let saved_refs = git::snapshot_branch_refs(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        graph.drop_commit(target_oid);

        // Save LoomState before the rebase.
        let git_dir = repo.path().to_path_buf();
        let fold_ctx = serde_json::to_value(FoldVariant::CommitToUnstaged {
            commit_hash: commit_hash.to_string(),
            diff: diff.clone(),
        })?;
        let loom_state = LoomState {
            command: "fold".to_string(),
            rollback: Rollback::default(),
            context: fold_ctx,
        };
        transaction::save(&git_dir, &loom_state)?;

        let todo = graph.to_todo();
        match weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)? {
            RebaseOutcome::Completed => {
                transaction::delete(&git_dir)?;
                if !diff.is_empty()
                    && let Err(e) = git_commands::apply_patch(workdir, &diff)
                {
                    let _ = git_commit::reset_hard(workdir, &saved_head);
                    let _ = git::restore_branch_refs(workdir, &saved_refs);
                    return Err(e).context(
                        "Failed to apply changes to working directory, operation rolled back",
                    );
                }
            }
            RebaseOutcome::Conflicted => {
                transaction::warn_conflict_paused("fold");
                return Ok(());
            }
        }
    }

    msg::success(&format!(
        "Uncommitted `{}` to working directory",
        git_commands::short_hash(commit_hash)
    ));

    Ok(())
}

/// Resume a `fold` operation after a conflict has been resolved.
pub fn after_continue(workdir: &Path, context: &serde_json::Value) -> Result<()> {
    let variant: FoldVariant =
        serde_json::from_value(context.clone()).context("Failed to parse fold resume context")?;

    match variant {
        FoldVariant::FilesIntoCommit {
            original_commit_hash,
            files_count,
            saved_staged,
        } => {
            let new_hash = git_commands::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git_branch::delete(workdir, TRACK_BRANCH);
            git_commands::restore_staged_patch(workdir, &saved_staged)?;
            msg::success(&format!(
                "Folded {} file(s) into `{}` (now `{}`)",
                files_count,
                git_commands::short_hash(&original_commit_hash),
                git_commands::short_hash(&new_hash)
            ));
        }
        FoldVariant::CommitIntoCommit {
            source_hash,
            target_hash,
        } => {
            let new_hash = git_commands::rev_parse(workdir, TRACK_BRANCH)?;
            let _ = git_branch::delete(workdir, TRACK_BRANCH);
            msg::success(&format!(
                "Folded `{}` into `{}` (now `{}`)",
                git_commands::short_hash(&source_hash),
                git_commands::short_hash(&target_hash),
                git_commands::short_hash(&new_hash)
            ));
        }
        FoldVariant::CommitToBranch {
            commit_hash,
            branch_name,
        } => {
            let new_hash = git_commands::rev_parse(workdir, &branch_name)?;
            msg::success(&format!(
                "Moved `{}` to branch `{}` (now `{}`)",
                git_commands::short_hash(&commit_hash),
                branch_name,
                git_commands::short_hash(&new_hash)
            ));
        }
        FoldVariant::CommitToUnstaged { commit_hash, diff } => {
            if !diff.is_empty()
                && let Err(e) = git_commands::apply_patch(workdir, &diff)
            {
                bail!(
                    "Commit `{}` was dropped but the changes could not be re-applied \
                     to the working directory: {}\n\
                     Run `loom continue` to retry, or `loom abort` to discard.\n\
                     The diff is preserved in .git/loom/state.json.",
                    git_commands::short_hash(&commit_hash),
                    e
                );
            }
            msg::success(&format!(
                "Uncommitted `{}` to working directory",
                git_commands::short_hash(&commit_hash)
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "fold_test.rs"]
mod tests;
