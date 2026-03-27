pub mod merge;
pub mod new;
pub mod unmerge;

use std::collections::HashSet;

use git2::Repository;

use anyhow::{Result, bail};

use crate::core::msg;
use crate::core::repo;

/// Emit a warning if `name` starts with the configured hidden branch prefix.
/// Despite the config key name `loom.hideBranchPattern`, this performs
/// prefix matching, not glob matching.
pub(crate) fn warn_if_hidden(repo: &Repository, name: &str) {
    let pattern =
        repo::hide_branch_pattern(repo).unwrap_or_else(|| repo::DEFAULT_HIDE_PATTERN.to_string());
    if !pattern.is_empty() && name.starts_with(&pattern) {
        msg::warn(&format!(
            "Branch `{}` is hidden from status by default. Use `--all` to show it.",
            name
        ));
    }
}

/// Determine if weaving is needed after branch creation.
///
/// Weaving is needed when the branch target is on the first-parent line
/// from HEAD to the merge-base (i.e., it's a loose commit on the integration
/// line, not already on a side branch). Commits at the merge-base are excluded
/// since no topology change is needed. Branching at HEAD weaves all first-parent
/// commits into the new branch section with a merge commit.
pub(crate) fn should_weave(
    info: &repo::RepoInfo,
    repo: &Repository,
    commit_hash: &str,
) -> Result<bool> {
    let head_oid = repo::head_oid(repo)?;
    let branch_oid = git2::Oid::from_str(commit_hash)?;

    let merge_base_oid = info.upstream.merge_base_oid;

    if branch_oid == merge_base_oid {
        return Ok(false);
    }

    // HEAD is on the first-parent line by definition
    if branch_oid == head_oid {
        return Ok(true);
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
) -> Result<bool> {
    let mut current = from;
    let mut visited: HashSet<git2::Oid> = HashSet::new();
    loop {
        if current == stop {
            return Ok(false);
        }
        if !visited.insert(current) {
            bail!("cycle detected in commit graph at {}", current);
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

#[cfg(test)]
#[path = "new_test.rs"]
mod new_tests;

#[cfg(test)]
#[path = "unmerge_test.rs"]
mod unmerge_tests;

#[cfg(test)]
#[path = "merge_test.rs"]
mod merge_tests;
