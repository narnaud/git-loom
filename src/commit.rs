use anyhow::{Context, Result, bail};
use git2::{Repository, StatusOptions};

use crate::git::{self, Target};
use crate::git_commands::{self, git_branch, git_commit};
use crate::weave::{self, Weave};

/// Verify that we're on an integration branch (has upstream tracking).
fn verify_on_integration_branch(repo: &Repository) -> Result<()> {
    git::gather_repo_info(repo, false).context(
        "Must be on an integration branch to use commit. \
         Use `git commit` directly on feature branches.",
    )?;
    Ok(())
}

/// Create a commit on a feature branch without leaving the integration branch.
///
/// Stages files, creates the commit at HEAD, then uses Weave to relocate
/// it to the target feature branch (creating merge topology if needed).
pub fn run(branch: Option<String>, message: Option<String>, files: Vec<String>) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "commit")?.to_path_buf();

    // Prerequisite: must be on an integration branch
    verify_on_integration_branch(&repo)?;

    // Step 1: Stage files
    resolve_staging(&repo, &workdir, &files)?;

    // Step 2: Verify index has changes
    verify_has_staged_changes(&repo)?;

    // Step 3: Save HEAD for rollback
    let saved_head = git::head_oid(&repo)?.to_string();

    // Step 4: Resolve branch target (may create a new branch at merge-base)
    let branch_name = resolve_branch_target(&repo, branch.as_deref())?;

    // Check if the branch is at the merge-base (no commits of its own).
    // Empty branches need to have a branch section and merge entry created
    // in the Weave before moving the commit there.
    let branch_is_empty = is_branch_at_merge_base(&repo, &branch_name)?;

    // Step 5: Create commit at HEAD
    if let Some(msg) = &message {
        git_commit::commit(&workdir, msg)?;
    } else {
        git_commit::commit_with_editor(&workdir)?;
    }

    // Step 6: Move commit to branch via Weave
    let head_oid = git::head_oid(&repo)?;

    let mut graph = Weave::from_repo(&repo)?;

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

    graph.move_commit(head_oid, &branch_name);

    let todo = graph.to_todo();
    if let Err(e) = weave::run_rebase(&workdir, Some(&graph.base_oid.to_string()), &todo) {
        let _ = git_commit::reset_hard(&workdir, &saved_head);
        if branch_is_empty {
            let _ = git_branch::delete(&workdir, &branch_name);
        }
        return Err(e);
    }

    println!(
        "Created commit {} on branch '{}'",
        git_commands::short_hash(&head_oid.to_string()),
        branch_name
    );

    Ok(())
}

/// Resolve staging based on file arguments.
///
/// - Empty: use index as-is
/// - Contains "zz": stage all changes
/// - Otherwise: resolve each arg as short ID or file path, then stage
fn resolve_staging(repo: &Repository, workdir: &std::path::Path, files: &[String]) -> Result<()> {
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
fn resolve_file_arg(repo: &Repository, workdir: &std::path::Path, arg: &str) -> Result<String> {
    match git::resolve_target(repo, arg) {
        Ok(Target::File(path)) => Ok(path),
        Ok(_) => bail!("'{}' is not a file", arg),
        Err(_) => {
            let full_path = workdir.join(arg);
            if full_path.exists() {
                Ok(arg.to_string())
            } else {
                bail!("File '{}' not found", arg)
            }
        }
    }
}

/// Verify that the index has staged changes.
fn verify_has_staged_changes(repo: &Repository) -> Result<()> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(false);

    let statuses = repo.statuses(Some(&mut opts))?;

    let has_staged = statuses.iter().any(|entry| {
        let status = entry.status();
        status.is_index_new() || status.is_index_modified() || status.is_index_deleted()
    });

    if !has_staged {
        bail!("Nothing to commit");
    }

    Ok(())
}

/// Resolve the target branch: explicit name/shortID, or interactive picker.
fn resolve_branch_target(repo: &Repository, branch: Option<&str>) -> Result<String> {
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
fn resolve_explicit_branch(repo: &Repository, branch: &str) -> Result<String> {
    let info = git::gather_repo_info(repo, false)?;

    match git::resolve_target(repo, branch) {
        Ok(Target::Branch(name)) => {
            if info.branches.iter().any(|b| b.name == name) {
                Ok(name)
            } else {
                bail!(
                    "Branch '{}' is not woven into the integration branch.",
                    name
                )
            }
        }
        Ok(Target::Commit(_)) => bail!("Commit target must be a branch"),
        Ok(Target::File(_)) => bail!("File target must be a branch"),
        Ok(Target::Unstaged) => bail!("Unstaged target must be a branch"),
        Ok(Target::CommitFile { .. }) => bail!("Commit file target must be a branch"),
        Err(_) => {
            // Treat as new branch name
            let name = branch.trim().to_string();
            if name.is_empty() {
                bail!("Branch name cannot be empty");
            }
            git_branch::validate_name(&name)?;

            if repo.find_branch(&name, git2::BranchType::Local).is_ok() {
                bail!(
                    "Branch '{}' exists but is not woven into the integration branch.",
                    name
                );
            }

            let workdir = git::require_workdir(repo, "create branch")?;
            create_branch_at_merge_base(repo, workdir, &name)?;
            Ok(name)
        }
    }
}

/// Interactive branch picker: list woven branches + option to create new.
fn pick_branch(repo: &Repository) -> Result<String> {
    let info = git::gather_repo_info(repo, false)?;

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
fn prompt_new_branch(repo: &Repository) -> Result<String> {
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

    git::ensure_branch_not_exists(repo, &name)?;

    let workdir = git::require_workdir(repo, "create branch")?;
    create_branch_at_merge_base(repo, workdir, &name)?;
    Ok(name)
}

/// Check if a branch points to the merge-base commit (i.e., has no commits of its own).
fn is_branch_at_merge_base(repo: &Repository, branch_name: &str) -> Result<bool> {
    let info = git::gather_repo_info(repo, false)?;
    let branch = repo.find_branch(branch_name, git2::BranchType::Local)?;
    let branch_oid = branch.get().target().context("Branch has no target")?;
    Ok(branch_oid == info.upstream.merge_base_oid)
}

/// Create a new branch at the merge-base.
///
/// The branch is not yet woven — weaving happens after the commit is created,
/// in the main `run` flow via Weave.
fn create_branch_at_merge_base(
    repo: &Repository,
    workdir: &std::path::Path,
    name: &str,
) -> Result<()> {
    let info = git::gather_repo_info(repo, false)?;
    let merge_base_hash = info.upstream.merge_base_oid.to_string();

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
