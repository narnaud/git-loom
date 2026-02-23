use anyhow::{Context, Result, bail};
use git2::{Repository, StatusOptions};

use crate::git::{self, Target};
use crate::git_commands::{self, git_commit};
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
            "Usage: git-loom fold <source>... <target>\n\
             At least two arguments required (one source + one target)."
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
                bail!("Cannot fold a branch. Use 'git loom branch' for branch operations.");
            }
            Target::Unstaged => {
                bail!(
                    "Cannot fold unstaged changes. Stage files first, or use \
                     'git loom fold <file> <commit>' to amend specific files."
                );
            }
            _ => {}
        }
    }

    // Reject CommitFile as target
    if matches!(target, Target::CommitFile { .. }) {
        bail!("Target must be a commit, branch, or unstaged (zz), not a commit file.");
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
        bail!("Cannot mix different source types (files, commits, commit files).");
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
                    "Cannot fold a commit file into a branch. Target a specific commit or use 'zz' to uncommit."
                )
            }
            Target::File(_) => bail!("Target must be a commit or unstaged (zz), not a file."),
            Target::CommitFile { .. } => unreachable!(),
        };
    }

    // Handle Unstaged target
    if matches!(target, Target::Unstaged) {
        if has_files {
            bail!("Cannot fold files into unstaged — files are already in the working directory.");
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
                bail!("Cannot fold files into a branch. Target a specific commit.")
            }
            Target::File(_) => bail!("Target must be a commit or branch, not a file."),
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
            Target::File(_) => bail!("Target must be a commit or branch, not a file."),
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
        git_commit::amend_no_edit(workdir)?;
    } else {
        // Stage files, create a temp commit, then fixup into target via Weave
        git_commit::stage_files(workdir, &file_refs)?;
        git_commit::commit(workdir, "fold: temp fixup")?;

        // The temp commit is now at HEAD — fixup it into the target
        let temp_oid = git::head_oid(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        graph.fixup_commit(temp_oid, target_oid);

        let todo = graph.to_todo();
        weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;
    }

    println!(
        "Folded {} file(s) into {}",
        files.len(),
        git_commands::short_hash(commit_hash)
    );

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

    println!(
        "Folded {} into {}",
        git_commands::short_hash(source_hash),
        git_commands::short_hash(target_hash)
    );

    Ok(())
}

/// Move a commit to a branch (Case 3: Commit + Branch → Move).
fn fold_commit_to_branch(repo: &Repository, commit_hash: &str, branch_name: &str) -> Result<()> {
    move_commit_to_branch(repo, commit_hash, branch_name)?;

    println!(
        "Moved {} to branch '{}'",
        git_commands::short_hash(commit_hash),
        branch_name
    );

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
        git_commands::apply_patch_reverse(workdir, &file_diff)?;
        git_commit::stage_path(workdir, path)?;
        git_commit::amend_no_edit(workdir)?;
        git_commands::apply_patch(workdir, &file_diff).context(
            "The commit was already modified but the diff could not be re-applied to the working directory. \
             Use 'git diff HEAD' or 'git reflog' to recover the changes.",
        )?;
    } else {
        // Non-HEAD: reverse + temp commit + fixup, then re-apply
        git_commands::apply_patch_reverse(workdir, &file_diff)?;
        git_commit::stage_path(workdir, path)?;
        git_commit::commit(workdir, "fold: remove file from commit")?;

        let temp_oid = git::head_oid(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        graph.fixup_commit(temp_oid, target_oid);

        let todo = graph.to_todo();
        weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;

        git_commands::apply_patch(workdir, &file_diff).context(
            "The commit was already modified but the diff could not be re-applied to the working directory. \
             Use 'git reflog' to find the original commit and recover the changes.",
        )?;
    }

    println!(
        "Uncommitted '{}' from {} to working directory",
        path,
        git_commands::short_hash(commit_hash)
    );

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
        // Two-phase approach: when source is newer than target, a single rebase
        // that fixups both temp commits causes an add/add conflict during
        // cherry-pick of the source commit (because the target already has the
        // file from its fixup). Instead, we do two separate rebases:
        //   Phase 1: Remove the file from source.
        //   Phase 2: Add the file to target.

        // Phase 1: Remove file from source via fixup rebase.
        git_commands::apply_patch_reverse(workdir, &file_diff)?;
        git_commit::stage_path(workdir, path)?;
        git_commit::commit(workdir, "fold: remove file from source")?;
        let temp_oid = git::head_oid(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        graph.fixup_commit(temp_oid, source_oid);
        let todo = graph.to_todo();

        // Create a temp branch on target so --update-refs tracks its new OID
        // across the rebase. Created AFTER from_repo() to avoid polluting the
        // Weave graph.
        let tmp_branch = "_loom-fold-target";
        git_commands::run_git(workdir, &["branch", "-f", tmp_branch, target_hash])?;

        let phase1 = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo);
        if phase1.is_err() {
            let _ = git_commands::run_git(workdir, &["branch", "-D", tmp_branch]);
            return phase1;
        }

        // Phase 2: Add file to target via fixup rebase.
        let new_target_hash = git_commands::run_git_stdout(workdir, &["rev-parse", tmp_branch])?;
        let new_target_hash = new_target_hash.trim().to_string();
        let _ = git_commands::run_git(workdir, &["branch", "-D", tmp_branch]);
        let new_target_oid = git2::Oid::from_str(&new_target_hash)?;

        git_commands::apply_patch(workdir, &file_diff)?;
        git_commit::stage_path(workdir, path)?;
        git_commit::commit(workdir, "fold: add file to target")?;
        let temp_oid = git::head_oid(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        graph.fixup_commit(temp_oid, new_target_oid);
        let todo = graph.to_todo();
        weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;
    } else {
        // Source is older than target: both fixups in one rebase (no conflict
        // because the removal is picked before the addition in the todo).
        git_commands::apply_patch_reverse(workdir, &file_diff)?;
        git_commit::stage_path(workdir, path)?;
        git_commit::commit(workdir, "fold: remove file from source")?;
        let temp_a_oid = git::head_oid(repo)?;

        git_commands::apply_patch(workdir, &file_diff)?;
        git_commit::stage_path(workdir, path)?;
        git_commit::commit(workdir, "fold: add file to target")?;
        let temp_b_oid = git::head_oid(repo)?;

        let mut graph = Weave::from_repo(repo)?;
        graph.fixup_commit(temp_a_oid, source_oid);
        graph.fixup_commit(temp_b_oid, target_oid);
        let todo = graph.to_todo();
        weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;
    }

    println!(
        "Moved '{}' from {} to {}",
        path,
        git_commands::short_hash(source_hash),
        git_commands::short_hash(target_hash)
    );

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

        let mut graph = Weave::from_repo(repo)?;
        graph.drop_commit(target_oid);

        let todo = graph.to_todo();
        weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;

        if !diff.is_empty() {
            git_commands::apply_patch(workdir, &diff)?;
        }
    }

    println!(
        "Uncommitted {} to working directory",
        git_commands::short_hash(commit_hash)
    );

    Ok(())
}

#[cfg(test)]
#[path = "fold_test.rs"]
mod tests;
