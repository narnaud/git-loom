use std::collections::{HashMap, HashSet};

use anyhow::Result;
use git2::Repository;

use crate::core::{graph, msg, repo};

/// Default prefix for branches hidden from status display.
const DEFAULT_HIDE_PATTERN: &str = "local-";

/// The branch-name prefix `loom.hideBranchPattern` hides, defaulting to
/// `local-`; empty when hiding is disabled. A prefix, not a glob, despite the
/// key's name (Spec 001).
pub fn hide_pattern(repo: &Repository) -> String {
    repo.config()
        .ok()
        .and_then(|config| config.get_string("loom.hideBranchPattern").ok())
        .unwrap_or_else(|| DEFAULT_HIDE_PATTERN.to_string())
}

/// True when `name` matches the hidden-branch prefix `pattern`.
pub fn is_hidden(name: &str, pattern: &str) -> bool {
    !pattern.is_empty() && name.starts_with(pattern)
}

/// Warn that `name` is hidden from `loom status`.
pub fn warn_if_hidden(repo: &Repository, name: &str) {
    if is_hidden(name, &hide_pattern(repo)) {
        msg::warn(&format!(
            "Branch `{}` is hidden from status by default. Use `--all` to show it.",
            name
        ));
    }
}

/// Drop the branches hidden by the `loom.hideBranchPattern` prefix from `info`,
/// as `loom status` does: both the branches and the commits they own are
/// removed, so neither shows up anywhere in the graph.
pub fn apply_hidden_branches(repo: &Repository, info: &mut repo::RepoInfo) {
    hide_branches(info, &hide_pattern(repo));
}

/// OID of the commit shown at the top of `loom status`: the tip of the
/// integration line, skipping merge commits and hidden branches. None when the
/// integration branch has no commits of its own above the merge-base.
pub fn top_commit(repo: &Repository) -> Result<Option<git2::Oid>> {
    let mut info = repo::gather_commit_graph(repo)?;
    apply_hidden_branches(repo, &mut info);
    Ok(graph::loose_commits(&info).first().copied())
}

/// Remove branches matching `pattern` (prefix match) and their owned commits
/// from `info` so they are fully invisible in the status display.
fn hide_branches(info: &mut repo::RepoInfo, pattern: &str) {
    let hidden_tips: HashSet<git2::Oid> = info
        .branches
        .iter()
        .filter(|b| is_hidden(&b.name, pattern))
        .map(|b| b.tip_oid)
        .collect();

    if hidden_tips.is_empty() {
        return;
    }

    let all_tips: HashSet<git2::Oid> = info.branches.iter().map(|b| b.tip_oid).collect();

    // Visible branch tip OIDs: when a hidden branch is co-located with a visible
    // branch (same tip OID), we must not steal the shared commit.
    let visible_tips: HashSet<git2::Oid> = info
        .branches
        .iter()
        .filter(|b| !is_hidden(&b.name, pattern))
        .map(|b| b.tip_oid)
        .collect();

    let commit_map: HashMap<git2::Oid, Option<git2::Oid>> =
        info.commits.iter().map(|c| (c.oid, c.parent_oid)).collect();

    // Walk from each hidden branch tip, collecting commits it owns.
    // Stop at another branch's tip (stacked-branch boundary), a visible branch
    // tip (co-located case), or out-of-range.
    let mut hidden_commits: HashSet<git2::Oid> = HashSet::new();
    for &tip in &hidden_tips {
        let mut current = Some(tip);
        let mut is_tip = true;
        while let Some(oid) = current {
            if !commit_map.contains_key(&oid) {
                break; // outside our commit range
            }
            // A visible branch owns this commit (co-located or stacked below).
            if visible_tips.contains(&oid) {
                break;
            }
            if !is_tip && all_tips.contains(&oid) {
                break; // reached another branch's tip
            }
            is_tip = false;
            hidden_commits.insert(oid);
            current = commit_map.get(&oid).and_then(|p| *p);
        }
    }

    info.branches.retain(|b| !is_hidden(&b.name, pattern));
    info.commits.retain(|c| !hidden_commits.contains(&c.oid));
}

#[cfg(test)]
#[path = "hidden_test.rs"]
mod tests;
