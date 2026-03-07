use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::{git, graph};

/// Branch name prefix used to identify local branches that should be hidden by default in status display.
const LOCAL_BRANCH: &str = "local-";

pub fn run(show_files: bool, context: usize, show_all: bool, theme: graph::Theme) -> Result<()> {
    let repo = git::open_repo()?;
    let _ = git::require_workdir(&repo, "display status")?;

    let opts = graph::default_render_opts(theme);
    let mut info = git::gather_repo_info(&repo, show_files, context)?;
    if !show_all {
        let pattern = git::hide_branch_pattern(&repo).unwrap_or_else(|| LOCAL_BRANCH.to_string());
        if !pattern.is_empty() {
            hide_branches(&mut info, &pattern);
        }
    }
    let output = graph::render(info, &opts);
    print!("{}", output);
    Ok(())
}

/// Remove branches matching `pattern` (prefix match) and their owned commits
/// from `info` so they are fully invisible in the status display.
fn hide_branches(info: &mut git::RepoInfo, pattern: &str) {
    let hidden_tips: HashSet<git2::Oid> = info
        .branches
        .iter()
        .filter(|b| b.name.starts_with(pattern))
        .map(|b| b.tip_oid)
        .collect();

    if hidden_tips.is_empty() {
        return;
    }

    // All branch tip OIDs (including hidden), used as stop points when walking.
    let all_tips: HashSet<git2::Oid> = info.branches.iter().map(|b| b.tip_oid).collect();

    // Visible branch tip OIDs: when a hidden branch is co-located with a visible
    // branch (same tip OID), we must not steal the shared commit.
    let visible_tips: HashSet<git2::Oid> = info
        .branches
        .iter()
        .filter(|b| !b.name.starts_with(pattern))
        .map(|b| b.tip_oid)
        .collect();

    // Build a parent-chain lookup so we can walk ancestry without touching git2.
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

    info.branches.retain(|b| !b.name.starts_with(pattern));
    info.commits.retain(|c| !hidden_commits.contains(&c.oid));
}

#[cfg(test)]
#[path = "status_test.rs"]
mod tests;
