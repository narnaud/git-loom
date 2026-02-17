use git2::{Repository, StatusOptions};

use crate::fold;
use crate::git::{self, Target};
use crate::git_commands::{self, git_branch, git_commit, git_merge};

/// Create a commit on a feature branch without leaving the integration branch.
///
/// Stages files, creates the commit at HEAD, then uses fold's Move rebase action
/// to relocate it to the target feature branch.
pub fn run(
    branch: Option<String>,
    message: Option<String>,
    files: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let repo = Repository::discover(cwd)?;
    let workdir = repo
        .workdir()
        .ok_or("Cannot commit in bare repository")?
        .to_path_buf();

    // Prerequisite: must be on an integration branch
    verify_on_integration_branch(&repo)?;

    // Step 1: Stage files
    resolve_staging(&repo, &workdir, &files)?;

    // Step 2: Verify index has changes
    verify_has_staged_changes(&repo)?;

    // Step 3: Resolve branch target (may create a new branch at merge-base)
    let branch_name = resolve_branch_target(&repo, branch.as_deref())?;

    // Check if the branch is at the merge-base (no commits of its own).
    // Empty branches need a different strategy: reset + merge instead of rebase,
    // because rebase can't create the merge topology needed for weaving.
    let branch_is_empty = is_branch_at_merge_base(&repo, &branch_name)?;

    // Step 4: Create commit at HEAD
    if let Some(msg) = &message {
        git_commit::commit(&workdir, msg)?;
    } else {
        git_commit::commit_with_editor(&workdir)?;
    }

    // Step 5: Move commit to branch
    let head_oid = repo.head()?.target().ok_or("HEAD has no target")?;
    let head_hash = head_oid.to_string();

    if branch_is_empty {
        weave_head_commit_to_branch(&workdir, &branch_name)?;
    } else {
        fold::move_commit_to_branch(&repo, &head_hash, &branch_name)?;
    }

    println!(
        "Created commit {} on branch '{}'",
        git_commands::short_hash(&head_hash),
        branch_name
    );

    Ok(())
}

/// Verify that we're on an integration branch (has upstream tracking).
fn verify_on_integration_branch(repo: &Repository) -> Result<(), Box<dyn std::error::Error>> {
    git::gather_repo_info(repo).map_err(|_| {
        "Must be on an integration branch to use commit. \
         Use `git commit` directly on feature branches."
    })?;
    Ok(())
}

/// Resolve staging based on file arguments.
///
/// - Empty: use index as-is
/// - Contains "zz": stage all changes
/// - Otherwise: resolve each arg as short ID or file path, then stage
fn resolve_staging(
    repo: &Repository,
    workdir: &std::path::Path,
    files: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    if files.is_empty() {
        return Ok(());
    }

    if files.iter().any(|f| f == "zz") {
        git_commit::stage_all(workdir)?;
        return Ok(());
    }

    let mut resolved_paths = Vec::new();
    for arg in files {
        let path = resolve_file_arg(repo, workdir, arg)?;
        resolved_paths.push(path);
    }

    let path_refs: Vec<&str> = resolved_paths.iter().map(|s| s.as_str()).collect();
    git_commit::stage_files(workdir, &path_refs)?;

    Ok(())
}

/// Resolve a file argument: try as short ID first, fall back to filesystem path.
fn resolve_file_arg(
    repo: &Repository,
    workdir: &std::path::Path,
    arg: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    match git::resolve_target(repo, arg) {
        Ok(Target::File(path)) => Ok(path),
        Ok(_) => Err(format!("'{}' is not a file.", arg).into()),
        Err(_) => {
            let full_path = workdir.join(arg);
            if full_path.exists() {
                Ok(arg.to_string())
            } else {
                Err(format!("File '{}' not found.", arg).into())
            }
        }
    }
}

/// Verify that the index has staged changes.
fn verify_has_staged_changes(repo: &Repository) -> Result<(), Box<dyn std::error::Error>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(false);

    let statuses = repo.statuses(Some(&mut opts))?;

    let has_staged = statuses.iter().any(|entry| {
        let status = entry.status();
        status.is_index_new() || status.is_index_modified() || status.is_index_deleted()
    });

    if !has_staged {
        return Err("Nothing to commit.".into());
    }

    Ok(())
}

/// Resolve the target branch: explicit name/shortID, or interactive picker.
fn resolve_branch_target(
    repo: &Repository,
    branch: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    match branch {
        Some(b) => resolve_explicit_branch(repo, b),
        None => pick_branch(repo),
    }
}

/// Resolve an explicit branch argument.
///
/// - Known woven branch (by name or short ID): use it
/// - Known branch but not woven: error
/// - Unknown: treat as new branch name, validate, create at merge-base, weave
fn resolve_explicit_branch(
    repo: &Repository,
    branch: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let info = git::gather_repo_info(repo)?;

    match git::resolve_target(repo, branch) {
        Ok(Target::Branch(name)) => {
            if info.branches.iter().any(|b| b.name == name) {
                Ok(name)
            } else {
                Err(format!(
                    "Branch '{}' is not woven into the integration branch.",
                    name
                )
                .into())
            }
        }
        Ok(Target::Commit(_)) => Err("Commit target must be a branch.".into()),
        Ok(Target::File(_)) => Err("File target must be a branch.".into()),
        Err(_) => {
            // Treat as new branch name
            let name = branch.trim().to_string();
            if name.is_empty() {
                return Err("Branch name cannot be empty.".into());
            }
            git_branch::validate_name(&name)?;

            if repo.find_branch(&name, git2::BranchType::Local).is_ok() {
                return Err(format!(
                    "Branch '{}' exists but is not woven into the integration branch.",
                    name
                )
                .into());
            }

            let workdir = repo
                .workdir()
                .ok_or("Cannot create branch in bare repository")?;
            create_branch_at_merge_base(repo, workdir, &name)?;
            Ok(name)
        }
    }
}

/// Interactive branch picker: list woven branches + option to create new.
fn pick_branch(repo: &Repository) -> Result<String, Box<dyn std::error::Error>> {
    let info = git::gather_repo_info(repo)?;

    if info.branches.is_empty() {
        // No woven branches — prompt to create one
        return prompt_new_branch(repo);
    }

    let mut select = cliclack::select("Select target branch");
    for branch in &info.branches {
        select = select.item(branch.name.clone(), &branch.name, "");
    }
    select = select.item("__create_new__".to_string(), "Create new branch", "");

    let selection: String = select.interact()?;

    if selection == "__create_new__" {
        prompt_new_branch(repo)
    } else {
        Ok(selection)
    }
}

/// Prompt for a new branch name and create it at merge-base.
fn prompt_new_branch(repo: &Repository) -> Result<String, Box<dyn std::error::Error>> {
    let name: String = cliclack::input("Branch name")
        .validate(|s: &String| {
            if s.trim().is_empty() {
                Err("Branch name cannot be empty")
            } else {
                Ok(())
            }
        })
        .interact()?;
    let name = name.trim().to_string();
    git_branch::validate_name(&name)?;

    if repo.find_branch(&name, git2::BranchType::Local).is_ok() {
        return Err(format!("Branch '{}' already exists.", name).into());
    }

    let workdir = repo
        .workdir()
        .ok_or("Cannot create branch in bare repository")?;
    create_branch_at_merge_base(repo, workdir, &name)?;
    Ok(name)
}

/// Check if a branch points to the merge-base commit (i.e., has no commits of its own).
fn is_branch_at_merge_base(
    repo: &Repository,
    branch_name: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let info = git::gather_repo_info(repo)?;
    let merge_base = repo.revparse_single(&info.upstream.base_short_id)?;
    let branch = repo.find_branch(branch_name, git2::BranchType::Local)?;
    let branch_oid = branch.get().target().ok_or("Branch has no target")?;
    Ok(branch_oid == merge_base.id())
}

/// Weave a newly-committed HEAD into a branch that previously had no commits.
///
/// For branches at the merge-base, the rebase-based move can't create merge topology.
/// Instead, we point the branch at the new commit, reset integration back, and merge
/// with `--no-ff` to create a proper merge commit.
fn weave_head_commit_to_branch(
    workdir: &std::path::Path,
    branch_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Stash any remaining working tree changes (mimics rebase --autostash)
    let stashed = git_commands::run_git(workdir, &["stash"]).is_ok();

    // Point the branch to HEAD (the new commit)
    git_commands::run_git(workdir, &["branch", "-f", branch_name, "HEAD"])?;
    // Move integration back to before the new commit
    git_commands::run_git(workdir, &["reset", "--hard", "HEAD~1"])?;
    // Merge the branch to create proper merge topology
    git_merge::merge_no_ff(workdir, branch_name)?;

    // Restore stashed changes
    if stashed {
        let _ = git_commands::run_git(workdir, &["stash", "pop"]);
    }

    Ok(())
}

/// Create a new branch at the merge-base.
///
/// The branch is not yet woven — weaving happens after the commit is created,
/// in the main `run` flow via `weave_head_commit_to_branch`.
fn create_branch_at_merge_base(
    repo: &Repository,
    workdir: &std::path::Path,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let info = git::gather_repo_info(repo)?;
    let merge_base = repo.revparse_single(&info.upstream.base_short_id)?;
    let merge_base_hash = merge_base.id().to_string();

    git_branch::create(workdir, name, &merge_base_hash)?;

    println!(
        "Created branch '{}' at {}",
        name,
        git_commands::short_hash(&merge_base_hash)
    );

    Ok(())
}

#[cfg(test)]
#[path = "commit_test.rs"]
mod tests;
