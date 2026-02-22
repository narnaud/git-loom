use std::collections::{HashMap, HashSet};

use git2::Repository;

use crate::branch::is_on_first_parent_line;
use crate::git::{self, Target};
use crate::git_commands::{self, git_branch};
use crate::weave::{self, Weave};

/// Drop a commit or a branch from history.
///
/// Dispatches based on the resolved target type:
/// - Commit → remove the commit via interactive rebase
/// - Branch → remove all branch commits, unweave merge topology, delete the ref
pub fn run(target: String) -> Result<(), Box<dyn std::error::Error>> {
    let repo = git::open_repo()?;

    let resolved = git::resolve_target(&repo, &target)?;

    match resolved {
        Target::Commit(hash) => drop_commit(&repo, &hash),
        Target::Branch(name) => drop_branch(&repo, &name),
        Target::File(_) => {
            Err("Cannot drop a file. Use 'git restore' to discard file changes.".into())
        }
        Target::Unstaged => {
            Err("Cannot drop unstaged changes. Use 'git restore' to discard changes.".into())
        }
    }
}

/// Drop a single commit from history via interactive rebase.
///
/// If the commit is the only commit on a branch, delegates to `drop_branch`
/// to properly remove the entire branch section and merge topology.
fn drop_commit(repo: &Repository, commit_hash: &str) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = git::require_workdir(repo, "drop")?;

    let commit_oid = git2::Oid::from_str(commit_hash)?;

    // Check if this commit is the only commit on a branch.
    // If so, delegate to drop_branch for clean section removal.
    if let Ok(info) = git::gather_repo_info(repo) {
        let merge_base_oid = info.upstream.merge_base_oid;

        if let Some(branch_name) = find_branch_owning_commit_from_info(&info, commit_oid)
            && let Some(branch_info) = info.branches.iter().find(|b| b.name == branch_name)
        {
            let owned = find_owned_commits(
                repo,
                branch_info.tip_oid,
                merge_base_oid,
                &info.branches,
                &branch_name,
            )?;
            if owned.len() == 1 {
                // This is the only commit on the branch — drop the whole branch
                return drop_branch(repo, &branch_name);
            }
        }
    }

    // Build weave and drop the commit
    let mut graph = Weave::from_repo(repo)?;
    graph.drop_commit(commit_oid);

    let todo = graph.to_todo();
    weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;

    let short_hash = git_commands::short_hash(commit_hash);
    println!("Dropped commit {}", short_hash);
    Ok(())
}

/// Drop a branch: remove all its commits, unweave merge topology, delete the ref.
fn drop_branch(repo: &Repository, branch_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = git::require_workdir(repo, "drop")?;

    let info = git::gather_repo_info(repo)?;

    // Verify the branch is in the integration range
    let branch_info = info
        .branches
        .iter()
        .find(|b| b.name == branch_name)
        .ok_or_else(|| {
            format!(
                "Branch '{}' is not in the integration range. \
                 Use 'git branch -d {}' to delete it directly.",
                branch_name, branch_name
            )
        })?;

    let head_oid = git::head_oid(repo)?;
    let merge_base_oid = info.upstream.merge_base_oid;

    // Check if branch is at the merge-base with no owned commits
    if branch_info.tip_oid == merge_base_oid {
        git_branch::delete(workdir, branch_name)?;
        println!("Dropped branch '{}'", branch_name);
        return Ok(());
    }

    // Check if another branch shares the same tip (co-located branches)
    let colocated_branch = info
        .branches
        .iter()
        .find(|b| b.name != branch_name && b.tip_oid == branch_info.tip_oid);

    // Determine if the branch is woven (tip NOT on first-parent line)
    let is_woven = branch_info.tip_oid != head_oid
        && !is_on_first_parent_line(repo, head_oid, merge_base_oid, branch_info.tip_oid)?;

    // Build weave and apply the appropriate mutation
    let mut graph = Weave::from_repo(repo)?;

    if is_woven {
        if let Some(keep) = colocated_branch {
            graph.reassign_branch(branch_name, &keep.name);
        } else {
            graph.drop_branch(branch_name);
        }
    } else {
        // Non-woven branch: drop each uniquely owned commit individually
        let owned = find_owned_commits(
            repo,
            branch_info.tip_oid,
            merge_base_oid,
            &info.branches,
            branch_name,
        )?;

        for oid in &owned {
            graph.drop_commit(*oid);
        }
    }

    let todo = graph.to_todo();
    weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)?;

    // Delete the branch ref
    git_branch::delete(workdir, branch_name)?;

    println!("Dropped branch '{}'", branch_name);
    Ok(())
}

/// Determine which branch owns the given commit using pre-gathered repo info.
///
/// Walks from each branch tip along parent links, stopping at another branch's
/// tip or the edge of the commit range. Returns the branch name if found.
fn find_branch_owning_commit_from_info(
    info: &git::RepoInfo,
    target_oid: git2::Oid,
) -> Option<String> {
    let parent_map: HashMap<git2::Oid, Option<git2::Oid>> =
        info.commits.iter().map(|c| (c.oid, c.parent_oid)).collect();

    let branch_tip_set: HashSet<git2::Oid> = info.branches.iter().map(|b| b.tip_oid).collect();

    for branch in &info.branches {
        let mut current = Some(branch.tip_oid);
        let mut is_tip = true;
        while let Some(oid) = current {
            if !parent_map.contains_key(&oid) {
                break;
            }
            if !is_tip && branch_tip_set.contains(&oid) {
                break;
            }
            is_tip = false;
            if oid == target_oid {
                return Some(branch.name.clone());
            }
            current = parent_map.get(&oid).and_then(|p| *p);
        }
    }

    None
}

/// Find all commits owned by a branch (from tip to next boundary or merge-base).
///
/// `dropping_branch_name` identifies the branch being dropped so that other
/// branches sharing the same tip are properly excluded. Without this, co-located
/// branches (same tip) would not be hidden, causing their shared commits to be
/// incorrectly reported as owned by the dropping branch.
fn find_owned_commits(
    repo: &Repository,
    branch_tip: git2::Oid,
    merge_base_oid: git2::Oid,
    all_branches: &[git::BranchInfo],
    dropping_branch_name: &str,
) -> Result<Vec<git2::Oid>, Box<dyn std::error::Error>> {
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
        // Skip merge commits (same as gather_repo_info)
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
