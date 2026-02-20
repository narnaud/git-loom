use git2::Repository;

use crate::git;
use crate::git_commands::{self, git_branch};
use crate::weave::{self, Weave};

/// Create a new branch at a target commit, weaving it into the integration branch
/// if the target is between the merge-base and HEAD.
///
/// If `name` is `None`, prompts interactively for a branch name.
/// If `target` is `None`, defaults to the merge-base (upstream base) commit.
/// The target can be a commit hash, branch name, or shortID.
///
/// When the branch is created at a commit that is neither HEAD nor the merge-base,
/// the topology is restructured: commits after the branch point are rebased onto
/// the merge-base, and a merge commit joins them with the branch.
pub fn run(name: Option<String>, target: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let repo = git::open_repo()?;

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

    git::ensure_branch_not_exists(&repo, &name)?;

    let commit_hash = resolve_commit(&repo, target.as_deref())?;

    let workdir = git::require_workdir(&repo, "create branch")?;

    git_branch::create(workdir, &name, &commit_hash)?;

    println!(
        "Created branch '{}' at {}",
        name,
        git_commands::short_hash(&commit_hash)
    );

    // Check if weaving is needed
    if should_weave(&repo, &commit_hash)? {
        let mut graph = Weave::from_repo(&repo)?;
        graph.weave_branch(&name);

        let todo = graph.to_todo();
        weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;

        println!("Woven '{}' into integration branch", name);
    }

    Ok(())
}

/// Determine if weaving is needed after branch creation.
///
/// Weaving is needed when the branch target is on the first-parent line
/// from HEAD to the merge-base (i.e., it's a loose commit on the integration
/// line, not already on a side branch). Commits at HEAD or the merge-base
/// are excluded since no topology change is needed for those.
fn should_weave(repo: &Repository, commit_hash: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let info = match git::gather_repo_info(repo) {
        Ok(info) => info,
        Err(_) => return Ok(false), // No upstream info available, skip weave
    };

    let head_oid = git::head_oid(repo)?;
    let branch_oid = git2::Oid::from_str(commit_hash)?;

    let merge_base_oid = info.upstream.merge_base_oid;

    if branch_oid == head_oid || branch_oid == merge_base_oid {
        return Ok(false);
    }

    // Only weave if the target commit is on the first-parent line.
    // Commits on side branches (reachable only through merge second-parents)
    // already have the merge topology in place.
    if !is_on_first_parent_line(repo, head_oid, merge_base_oid, branch_oid)? {
        return Ok(false);
    }

    Ok(true)
}

/// Check if `target` is on the first-parent path from `from` down to `stop`.
///
/// Walks the first-parent chain (skipping merge second-parents) and returns
/// true if `target` is found before reaching `stop`.
pub fn is_on_first_parent_line(
    repo: &Repository,
    from: git2::Oid,
    stop: git2::Oid,
    target: git2::Oid,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut current = from;
    loop {
        if current == stop {
            return Ok(false);
        }
        let commit = repo.find_commit(current)?;
        // Follow only the first parent
        let first_parent = match commit.parent_id(0) {
            Ok(oid) => oid,
            Err(_) => return Ok(false), // reached root
        };
        if first_parent == target {
            return Ok(true);
        }
        current = first_parent;
    }
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
            Ok(info.upstream.merge_base_oid.to_string())
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
