use anyhow::{Context, Result, bail};
use git2::{Repository, StatusOptions};

use crate::git::{self, Target};
use crate::git_commands::{self, git_branch, git_commit};
use crate::msg;
use crate::weave::{self, Weave};

/// Create a commit on a feature branch without leaving the integration branch.
///
/// Stages files, creates the commit at HEAD, then uses Weave to relocate
/// it to the target feature branch (creating merge topology if needed).
pub fn run(branch: Option<String>, message: Option<String>, files: Vec<String>) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "commit")?.to_path_buf();

    // Gather repo info once — also serves as verification that we're on an
    // integration branch (gather_repo_info requires an upstream).
    let info = git::gather_repo_info(&repo, false, 1).context(
        "Must be on an integration branch to use commit\n\
         Use `git commit` directly on feature branches",
    )?;

    // Step 1: Stage files (saving aside any pre-existing staged files not in
    // the target list so they don't accidentally end up in this commit).
    let saved_staged = resolve_staging(&repo, &workdir, &files)?;

    // Step 2: Verify index has changes
    verify_has_staged_changes(&repo)?;

    // Loose commit: when no -b flag and local branch name matches the
    // upstream's local counterpart (e.g. "main" tracking "origin/main"),
    // commit directly on the integration branch without targeting a feature
    // branch. This works regardless of whether local commits or woven
    // branches already exist.
    if branch.is_none() && info.branch_name == git::upstream_local_branch(&info.upstream.label) {
        let result = if let Some(msg) = &message {
            git_commit::commit(&workdir, msg)
        } else {
            git_commit::commit_with_editor(&workdir)
        };
        if let Err(e) = result {
            let _ = restore_staged(&workdir, &saved_staged);
            return Err(e);
        }
        restore_staged(&workdir, &saved_staged)?;
        let new_head = git::head_oid(&repo)?;
        msg::success(&format!(
            "Created commit `{}`",
            git_commands::short_hash(&new_head.to_string())
        ));
        return Ok(());
    }

    // Step 3: Save HEAD for rollback
    let saved_head = git::head_oid(&repo)?.to_string();

    // Step 4: Resolve branch target (may create a new branch at merge-base)
    // Snapshot existing branch names so we can detect newly-created branches.
    // Only delete newly-created branches on rollback (not pre-existing empty ones).
    let branches_before: Vec<String> = repo
        .branches(Some(git2::BranchType::Local))?
        .filter_map(|b| b.ok())
        .filter_map(|(b, _)| b.name().ok().flatten().map(String::from))
        .collect();
    let branch_name = resolve_branch_target(&repo, &info, &workdir, branch.as_deref())?;
    let branch_is_new = !branches_before.contains(&branch_name);

    // Check if the branch is at the merge-base (no commits of its own).
    // Empty branches need to have a branch section and merge entry created
    // in the Weave before moving the commit there.
    let branch_is_empty =
        is_branch_at_merge_base(&repo, &branch_name, info.upstream.merge_base_oid)?;

    // Step 5: Create commit at HEAD
    let commit_result = if let Some(msg) = &message {
        git_commit::commit(&workdir, msg)
    } else {
        git_commit::commit_with_editor(&workdir)
    };
    if let Err(e) = commit_result {
        let _ = restore_staged(&workdir, &saved_staged);
        return Err(e);
    }

    // Step 6: Move commit to branch via Weave
    let head_oid = git::head_oid(&repo)?;

    let mut graph = Weave::from_repo_with_info(&repo, &info)?;

    if branch_is_empty {
        // For empty branches, create a new branch section and merge topology
        graph.add_branch_section(
            branch_name.clone(),
            vec![branch_name.clone()],
            vec![],
            "onto".to_string(),
        );
        graph.add_merge(branch_name.clone(), None, None);
    }

    graph.move_commit(head_oid, &branch_name)?;

    let todo = graph.to_todo();
    if let Err(e) = weave::run_rebase(&workdir, Some(&graph.base_oid.to_string()), &todo) {
        // Mixed reset preserves working-tree changes (the committed content
        // stays in the working directory as unstaged modifications).
        let _ = git_commit::reset_mixed(&workdir, &saved_head);
        if branch_is_new {
            let _ = git_branch::delete(&workdir, &branch_name);
        }
        let _ = restore_staged(&workdir, &saved_staged);
        return Err(e);
    }

    restore_staged(&workdir, &saved_staged)?;

    let new_hash = git_commands::rev_parse(&workdir, &branch_name)?;

    msg::success(&format!(
        "Created commit `{}` on branch `{}`",
        git_commands::short_hash(&new_hash),
        branch_name
    ));

    Ok(())
}

/// Resolve staging based on file arguments.
///
/// - Empty: use index as-is; returns an empty saved patch.
/// - Contains "zz": stage all changes; returns an empty saved patch.
/// - Otherwise: save and unstage any pre-existing staged files NOT in the
///   target list (so they don't leak into this commit), stage the target
///   files, and return the saved patch for later restoration.
fn resolve_staging(
    repo: &Repository,
    workdir: &std::path::Path,
    files: &[String],
) -> Result<String> {
    if files.is_empty() {
        return Ok(String::new());
    }

    if files.iter().any(|f| f == "zz") {
        git_commit::stage_all(workdir)?;
        return Ok(String::new());
    }

    let mut resolved_paths = Vec::new();
    for arg in files {
        let path = resolve_file_arg(repo, arg)?;
        resolved_paths.push(path);
    }

    let path_refs: Vec<&str> = resolved_paths.iter().map(|s| s.as_str()).collect();

    // Save and unstage any pre-existing staged files not in our target list.
    let saved_staged = save_and_unstage_other_staged(repo, workdir, &path_refs)?;

    git_commit::stage_files(workdir, &path_refs)?;

    Ok(saved_staged)
}

/// Save the staged diff for files that are staged but NOT in `target_files`,
/// then unstage them so they don't leak into the upcoming commit.
///
/// Returns the patch as a string (may be empty if nothing to save).
fn save_and_unstage_other_staged(
    repo: &Repository,
    workdir: &std::path::Path,
    target_files: &[&str],
) -> Result<String> {
    let staged = git::get_staged_files(repo)?;
    let other: Vec<&str> = staged
        .iter()
        .filter(|f| !target_files.contains(&f.as_str()))
        .map(|s| s.as_str())
        .collect();

    if other.is_empty() {
        return Ok(String::new());
    }

    let patch = git_commands::diff_cached_files(workdir, &other)?;
    git_commands::unstage_files(workdir, &other)?;
    Ok(patch)
}

/// Re-apply a previously saved staged patch.
///
/// No-ops if the patch is empty. On failure, emits a warning rather than
/// returning an error — the primary operation already succeeded.
fn restore_staged(workdir: &std::path::Path, patch: &str) -> Result<()> {
    if patch.is_empty() {
        return Ok(());
    }
    if let Err(e) = git_commands::apply_cached_patch(workdir, patch) {
        eprintln!(
            "Warning: could not restore pre-existing staged changes: {}",
            e
        );
    }
    Ok(())
}

/// Resolve a file argument using the centralized resolver.
fn resolve_file_arg(repo: &Repository, arg: &str) -> Result<String> {
    match git::resolve_arg(repo, arg, &[git::TargetKind::File])? {
        Target::File(path) => Ok(path),
        _ => unreachable!(),
    }
}

/// Verify that the index has staged changes.
pub fn verify_has_staged_changes(repo: &Repository) -> Result<()> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(false);

    let statuses = repo.statuses(Some(&mut opts))?;

    let has_staged = statuses.iter().any(|entry| {
        let status = entry.status();
        status.is_index_new()
            || status.is_index_modified()
            || status.is_index_deleted()
            || status.is_index_renamed()
            || status.is_index_typechange()
    });

    if !has_staged {
        bail!("Nothing to commit");
    }

    Ok(())
}

/// Resolve the target branch: explicit name/shortID, or interactive picker.
fn resolve_branch_target(
    repo: &Repository,
    info: &git::RepoInfo,
    workdir: &std::path::Path,
    branch: Option<&str>,
) -> Result<String> {
    match branch {
        Some(b) => resolve_explicit_branch(repo, info, workdir, b),
        None => pick_branch(repo, info, workdir),
    }
}

/// Resolve an explicit branch argument.
///
/// - Known woven branch (by name or short ID): use it
/// - Known branch but not woven: error
/// - Unknown: treat as new branch name, validate, create at merge-base, weave
fn resolve_explicit_branch(
    repo: &Repository,
    info: &git::RepoInfo,
    workdir: &std::path::Path,
    branch: &str,
) -> Result<String> {
    match git::resolve_arg(repo, branch, &[git::TargetKind::Branch]) {
        Ok(target) => {
            let name = target.expect_branch()?;
            if info.branches.iter().any(|b| b.name == name) {
                Ok(name)
            } else {
                bail!("Branch '{}' is not woven into the integration branch", name)
            }
        }
        Err(_) => {
            // Treat as new branch name
            let name = branch.trim().to_string();
            if name.is_empty() {
                bail!("Branch name cannot be empty");
            }
            git_branch::validate_name(&name)?;

            if repo.find_branch(&name, git2::BranchType::Local).is_ok() {
                bail!(
                    "Branch '{}' exists but is not woven into the integration branch",
                    name
                );
            }

            create_branch_at_merge_base(workdir, &name, info.upstream.merge_base_oid)?;
            Ok(name)
        }
    }
}

/// Interactive branch picker: select an existing woven branch or type a new name.
fn pick_branch(
    repo: &Repository,
    info: &git::RepoInfo,
    workdir: &std::path::Path,
) -> Result<String> {
    let branch_names: Vec<String> = info.branches.iter().map(|b| b.name.clone()).collect();

    let not_empty = |s: &str| {
        if s.trim().is_empty() {
            Err("Branch name cannot be empty")
        } else {
            Ok(())
        }
    };

    let name = if branch_names.is_empty() {
        msg::input("Branch name", not_empty)?
    } else {
        msg::select_or_input("Select target branch", branch_names.clone(), not_empty)?
    };

    let name = name.trim().to_string();

    // If user typed a name that isn't an existing woven branch, create it
    if !branch_names.contains(&name) {
        git_branch::validate_name(&name)?;
        git::ensure_branch_not_exists(repo, &name)?;
        create_branch_at_merge_base(workdir, &name, info.upstream.merge_base_oid)?;
    }

    Ok(name)
}

/// Check if a branch points to the merge-base commit (i.e., has no commits of its own).
fn is_branch_at_merge_base(
    repo: &Repository,
    branch_name: &str,
    merge_base_oid: git2::Oid,
) -> Result<bool> {
    let branch = repo.find_branch(branch_name, git2::BranchType::Local)?;
    let branch_oid = branch.get().target().context("Branch has no target")?;
    Ok(branch_oid == merge_base_oid)
}

/// Create a new branch at the merge-base.
///
/// The branch is not yet woven — weaving happens after the commit is created,
/// in the main `run` flow via Weave.
fn create_branch_at_merge_base(
    workdir: &std::path::Path,
    name: &str,
    merge_base_oid: git2::Oid,
) -> Result<()> {
    let merge_base_hash = merge_base_oid.to_string();

    git_branch::create(workdir, name, &merge_base_hash)?;

    msg::success(&format!(
        "Created branch `{}` at `{}`",
        name,
        git_commands::short_hash(&merge_base_hash)
    ));

    Ok(())
}

#[cfg(test)]
#[path = "commit_test.rs"]
mod tests;
