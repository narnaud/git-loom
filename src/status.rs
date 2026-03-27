use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::core::{graph, repo, shortid};

pub fn run(
    file_filter: Option<Vec<String>>,
    context: usize,
    show_all: bool,
    theme: graph::Theme,
) -> Result<()> {
    let repo = repo::open_repo()?;
    let _ = repo::require_workdir(&repo, "display status")?;

    let cwd_prefix = repo::cwd_relative_to_repo(&repo).unwrap_or_default();
    let opts = graph::default_render_opts(theme, cwd_prefix);
    let show_files = file_filter.is_some();
    let mut info = repo::gather_repo_info(&repo, show_files, context)?;

    // Collect entities from the full info BEFORE filtering so that short IDs
    // are stable regardless of which branches are hidden.
    let ids = shortid::IdAllocator::new(info.collect_entities());

    if !show_all {
        let pattern = repo::hide_branch_pattern(&repo)
            .unwrap_or_else(|| repo::DEFAULT_HIDE_PATTERN.to_string());
        if !pattern.is_empty() {
            hide_branches(&mut info, &pattern);
        }
    }

    // When specific commits are requested, clear files from non-matching commits.
    if let Some(filter_ids) = &file_filter
        && !filter_ids.is_empty()
    {
        let filter_oids = resolve_commit_filter(&repo, filter_ids, &info, &ids);
        for commit in &mut info.commits {
            if !filter_oids.contains(&commit.oid) {
                commit.files.clear();
            }
        }
    }

    let output = graph::render(info, &ids, &opts);
    print!("{}", output);
    Ok(())
}

/// Resolve a list of user-supplied IDs to a set of commit OIDs whose files
/// should be shown. Supports git hashes and loom commit short IDs.
/// Unknown IDs are silently skipped.
fn resolve_commit_filter(
    repo: &git2::Repository,
    ids: &[String],
    info: &repo::RepoInfo,
    allocator: &shortid::IdAllocator,
) -> HashSet<git2::Oid> {
    let mut filter_oids = HashSet::new();

    for id in ids {
        // 1. Try git reference (full/short hash, HEAD, etc.)
        if let Ok(obj) = repo.revparse_single(id)
            && let Ok(commit) = obj.peel_to_commit()
        {
            filter_oids.insert(commit.id());
            continue;
        }

        // 2. Try loom short ID for a commit
        if let Some(commit) = info
            .commits
            .iter()
            .find(|c| allocator.get_commit(c.oid) == id.as_str())
        {
            filter_oids.insert(commit.oid);
            continue;
        }

        // Not found → skip silently
    }

    filter_oids
}

/// Remove branches matching `pattern` (prefix match) and their owned commits
/// from `info` so they are fully invisible in the status display.
fn hide_branches(info: &mut repo::RepoInfo, pattern: &str) {
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
