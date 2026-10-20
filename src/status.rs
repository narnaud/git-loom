use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::core::{agent_mode, graph, hidden, msg, repo, shortid, status_json};

pub fn run(
    file_filter: Option<Vec<String>>,
    context: Option<usize>,
    show_all: bool,
    theme: graph::Theme,
) -> Result<()> {
    let repo = repo::open_repo()?;
    let _ = repo::require_workdir(&repo, "display status")?;

    let cwd_prefix = repo::cwd_relative_to_repo(&repo).unwrap_or_default();
    let opts = graph::default_render_opts(theme, cwd_prefix);
    let show_files = file_filter.is_some();
    let (mut info, ids) =
        repo::gather_status(&repo, show_files, repo::resolve_context(&repo, context))?;

    // Before hiding: it cuts the edge to a hidden branch, which `push` still
    // walks and refuses.
    let stacks = agent_mode::enabled().then(|| stack_edges(&repo, &info, show_all));

    if !show_all {
        hidden::apply_hidden_branches(&repo, &mut info);
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

    // One section list feeds both surfaces (spec 019).
    let branch_name = info.branch_name.clone();
    let sections = graph::build_sections(info);

    if let Some(stacks) = stacks {
        agent_mode::set_graph(&status_json::build(
            &sections,
            &branch_name,
            &stacks,
            &ids,
            &opts.cwd_prefix,
        ));
    }

    msg::human(graph::render_sections(&sections, &ids, &opts));
    Ok(())
}

/// Every branch's edge to the one below, with a hidden one flagged, and named
/// only when `show_all` keeps it in the graph.
fn stack_edges(
    repo: &git2::Repository,
    info: &repo::RepoInfo,
    show_all: bool,
) -> HashMap<String, status_json::StackEdge> {
    let pattern = hidden::hide_pattern(repo);
    graph::stack_parents(info)
        .into_iter()
        .map(|(branch, below)| {
            let below_hidden = hidden::is_hidden(&below, &pattern);
            let edge = status_json::StackEdge {
                below: (show_all || !below_hidden).then_some(below),
                below_hidden,
            };
            (branch, edge)
        })
        .collect()
}

/// Resolve a list of user-supplied IDs to a set of commit OIDs whose files
/// should be shown. Supports git hashes and loom commit short IDs, including a
/// unique persistent-ID prefix or Change-Id (Spec 002). Unknown and ambiguous
/// IDs are silently skipped.
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
            // Spec 002: a hash abbreviation that is also another commit's
            // short ID is ambiguous, and ambiguous IDs are skipped.
            if !shadows_short_id(repo, id, commit.id(), info, allocator) {
                filter_oids.insert(commit.id());
            }
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

        // 3. A persistent-ID prefix or Change-Id literal naming one commit
        if let [oid] = allocator.find_persistent(id).as_slice() {
            filter_oids.insert(*oid);
        }
    }

    filter_oids
}

/// Whether `id`, which git resolved to `oid` by hash abbreviation, is also
/// the short ID or unique persistent prefix of another commit (Spec 002).
fn shadows_short_id(
    repo: &git2::Repository,
    id: &str,
    oid: git2::Oid,
    info: &repo::RepoInfo,
    allocator: &shortid::IdAllocator,
) -> bool {
    if !repo::is_hash_and_letter_shaped(repo, id) {
        return false;
    }
    let exact = info
        .commits
        .iter()
        .find(|c| allocator.get_commit(c.oid) == id)
        .map(|c| c.oid);
    let other = exact.or_else(|| match allocator.find_persistent(id).as_slice() {
        [one] => Some(*one),
        _ => None,
    });
    other.is_some_and(|o| o != oid)
}

#[cfg(test)]
#[path = "status_test.rs"]
mod tests;
