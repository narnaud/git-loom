use git2::Repository;

use crate::git;
use crate::git_commands::{self, git_branch};

/// Create a new branch at a target commit.
///
/// If `name` is `None`, prompts interactively for a branch name.
/// If `target` is `None`, defaults to the merge-base (upstream base) commit.
/// The target can be a commit hash, branch name, or shortID.
pub fn run(
    name: Option<String>,
    target: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let repo = Repository::discover(cwd)?;

    let name = match name {
        Some(n) => n,
        None => cliclack::input("Branch name")
            .validate(|s: &String| {
                if s.trim().is_empty() {
                    Err("Branch name cannot be empty")
                } else {
                    Ok(())
                }
            })
            .interact()?,
    };

    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Branch name cannot be empty".into());
    }

    git_branch::validate_name(&name)?;

    if repo.find_branch(&name, git2::BranchType::Local).is_ok() {
        return Err(format!("Branch '{}' already exists", name).into());
    }

    let commit_hash = resolve_commit(&repo, target.as_deref())?;

    let workdir = repo
        .workdir()
        .ok_or("Cannot create branch in bare repository")?;

    git_branch::create(workdir, &name, &commit_hash)?;

    println!(
        "Created branch '{}' at {}",
        name,
        git_commands::short_hash(&commit_hash)
    );
    Ok(())
}

/// Resolve an optional target to a full commit hash.
/// If no target, defaults to the merge-base (upstream base).
fn resolve_commit(
    repo: &Repository,
    target: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    match target {
        None => {
            // Default: merge-base commit
            let info = git::gather_repo_info(repo)?;
            let base = repo.revparse_single(&info.upstream.base_short_id)?;
            Ok(base.id().to_string())
        }
        Some(t) => {
            let resolved = git::resolve_target(repo, t)?;
            match resolved {
                git::Target::Commit(hash) => Ok(hash),
                git::Target::Branch(name) => {
                    // Resolve branch to its tip commit
                    let branch = repo.find_branch(&name, git2::BranchType::Local)?;
                    let oid = branch
                        .get()
                        .target()
                        .ok_or("Branch does not point to a commit")?;
                    Ok(oid.to_string())
                }
                git::Target::File(path) => Err(format!(
                    "Target resolved to file '{}'. Use a commit or branch target instead.",
                    path
                )
                .into()),
            }
        }
    }
}

#[cfg(test)]
#[path = "branch_test.rs"]
mod tests;
