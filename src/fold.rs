use git2::{Repository, StatusOptions};

use crate::git::{self, Target};
use crate::git_commands::git_rebase::{Rebase, RebaseAction, RebaseTarget};
use crate::git_commands::{self, git_commit};

/// Fold source(s) into a target.
///
/// Dispatches to the appropriate operation based on argument types:
/// - File(s) + Commit → amend files into the commit
/// - Commit + Commit  → fixup source into target (source disappears)
/// - Commit + Branch   → move commit to the branch
pub fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 2 {
        return Err("Usage: git-loom fold <source>... <target>\n\
                     At least two arguments required (one source + one target)."
            .into());
    }

    let cwd = std::env::current_dir()?;
    let repo = Repository::discover(cwd)?;

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
    }
}

/// The classified fold operation.
#[derive(Debug)]
enum FoldOp {
    FilesIntoCommit { files: Vec<String>, commit: String },
    CommitIntoCommit { source: String, target: String },
    CommitToBranch { commit: String, branch: String },
}

/// Classify resolved arguments into a specific fold operation.
fn classify(sources: &[Target], target: &Target) -> Result<FoldOp, Box<dyn std::error::Error>> {
    // Check for invalid source types
    for source in sources {
        if let Target::Branch(_) = source {
            return Err(
                "Cannot fold a branch. Use 'git loom branch' for branch operations.".into(),
            );
        }
    }

    // All sources must be the same type (all files or all commits)
    let has_files = sources.iter().any(|s| matches!(s, Target::File(_)));
    let has_commits = sources.iter().any(|s| matches!(s, Target::Commit(_)));

    if has_files && has_commits {
        return Err("Cannot mix file and commit sources.".into());
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
                Err("Cannot fold files into a branch. Target a specific commit.".into())
            }
            Target::File(_) => Err("Target must be a commit or branch, not a file.".into()),
        }
    } else {
        // Commit(s) + target
        if sources.len() > 1 {
            return Err("Only one commit source is allowed.".into());
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
            Target::File(_) => Err("Target must be a commit or branch, not a file.".into()),
        }
    }
}

/// Resolve an argument for the fold command.
///
/// Tries `resolve_target()` first (handles branches, git refs, short IDs).
/// Falls back to checking if the argument is a filesystem path with changes.
fn resolve_fold_arg(repo: &Repository, arg: &str) -> Result<Target, Box<dyn std::error::Error>> {
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
fn file_has_changes(repo: &Repository, path: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let mut opts = StatusOptions::new();
    opts.pathspec(path)
        .include_untracked(true)
        .recurse_untracked_dirs(true);

    let statuses = repo.statuses(Some(&mut opts))?;
    Ok(!statuses.is_empty())
}

/// Fold file changes into a commit (Case 1: File(s) + Commit).
fn fold_files_into_commit(
    repo: &Repository,
    files: &[String],
    commit_hash: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = repo.workdir().ok_or("Cannot fold in bare repository")?;

    // Validate all files have changes
    for file in files {
        if !file_has_changes(repo, file)? {
            return Err(format!("File '{}' has no changes to fold.", file).into());
        }
    }

    let head_oid = repo.head()?.target().ok_or("HEAD has no target")?;
    let target_oid = git2::Oid::from_str(commit_hash)?;
    let is_head = head_oid == target_oid;

    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();

    if is_head {
        // Simple case: stage files and amend HEAD
        git_commit::stage_files(workdir, &file_refs)?;
        git_commit::amend_no_edit(workdir)?;
    } else {
        // Stage files, create a temp commit, then fixup into target
        git_commit::stage_files(workdir, &file_refs)?;
        git_commit::commit(workdir, "fold: temp fixup")?;

        // The temp commit is now at HEAD — fixup it into the target
        let temp_oid = repo.head()?.target().ok_or("HEAD has no target")?;
        let temp_hash = temp_oid.to_string();
        let temp_short = git_commands::short_hash(&temp_hash);
        let target_short = git_commands::short_hash(commit_hash);

        Rebase::new(workdir, RebaseTarget::Commit(commit_hash.to_string()))
            .action(RebaseAction::Fixup {
                source_hash: temp_short.to_string(),
                target_hash: target_short.to_string(),
            })
            .run()?;
    }

    println!(
        "Folded {} file(s) into {}",
        files.len(),
        git_commands::short_hash(commit_hash)
    );

    Ok(())
}

/// Fold a commit into another commit (Case 2: Commit + Commit → Fixup).
fn fold_commit_into_commit(
    repo: &Repository,
    source_hash: &str,
    target_hash: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = repo.workdir().ok_or("Cannot fold in bare repository")?;

    // Validate source is a descendant of target (source is newer)
    let source_oid = git2::Oid::from_str(source_hash)?;
    let target_oid = git2::Oid::from_str(target_hash)?;

    if source_oid == target_oid {
        return Err("Source and target are the same commit.".into());
    }

    if !repo.graph_descendant_of(source_oid, target_oid)? {
        return Err("Source commit must be newer than target commit.".into());
    }

    // Check if the target is a root commit (no parent)
    let target_commit = repo.find_commit(target_oid)?;
    let rebase_target = if target_commit.parent_count() == 0 {
        RebaseTarget::Root
    } else {
        RebaseTarget::Commit(target_hash.to_string())
    };

    let source_short = git_commands::short_hash(source_hash);
    let target_short = git_commands::short_hash(target_hash);

    Rebase::new(workdir, rebase_target)
        .action(RebaseAction::Fixup {
            source_hash: source_short.to_string(),
            target_hash: target_short.to_string(),
        })
        .run()?;

    println!(
        "Folded {} into {}",
        git_commands::short_hash(source_hash),
        git_commands::short_hash(target_hash)
    );

    Ok(())
}

/// Move a commit to a branch (Case 3: Commit + Branch → Move).
fn fold_commit_to_branch(
    repo: &Repository,
    commit_hash: &str,
    branch_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    move_commit_to_branch(repo, commit_hash, branch_name)?;

    println!(
        "Moved {} to branch '{}'",
        git_commands::short_hash(commit_hash),
        branch_name
    );

    Ok(())
}

/// Move a commit to the tip of a branch using interactive rebase.
///
/// The caller is responsible for ensuring the working tree is in an appropriate
/// state. The rebase uses `--autostash` to handle any remaining uncommitted changes.
///
/// Used by both fold (commit+branch) and commit commands.
pub fn move_commit_to_branch(
    repo: &Repository,
    commit_hash: &str,
    branch_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = repo.workdir().ok_or("Cannot fold in bare repository")?;

    let info = git::gather_repo_info(repo)?;
    let merge_base_oid = info.upstream.merge_base_oid;

    let merge_base_commit = repo.find_commit(merge_base_oid)?;
    let target = if merge_base_commit.parent_count() == 0 {
        RebaseTarget::Root
    } else {
        RebaseTarget::Commit(merge_base_oid.to_string())
    };

    let commit_short = git_commands::short_hash(commit_hash);

    Rebase::new(workdir, target)
        .action(RebaseAction::Move {
            commit_hash: commit_short.to_string(),
            before_label: branch_name.to_string(),
        })
        .run()?;

    Ok(())
}

#[cfg(test)]
#[path = "fold_test.rs"]
mod tests;
