use crate::git::{CommitInfo, FileChange, RepoInfo, UpstreamInfo};
use crate::shortid::IdAllocator;
use colored::{Color, Colorize};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

// ── Color palette (edit these to change the theme) ──────────────────────

/// Graph structure: lines, connectors, dots on the integration line.
const COLOR_GRAPH: Color = Color::BrightBlack;
/// Branch names in brackets (local and remote).
const COLOR_BRANCH: Color = Color::Green;
/// Labels like (upstream) and (common base).
const COLOR_LABEL: Color = Color::Cyan;
/// Dimmed secondary text: messages, dates, file changes, "no changes".
const COLOR_DIM: Color = Color::AnsiColor(240);
/// Colors for the commit message text
const COLOR_MESSAGE: Color = Color::AnsiColor(248);
/// Short ID prefix (blue + underline, applied in rendering).
const COLOR_SHORTID: Color = Color::Blue;

/// Rotating colors for commit dots on feature branches.
/// Each branch gets the next color in this cycle.
const BRANCH_DOT_COLORS: &[Color] = &[
    Color::Yellow,
    Color::Cyan,
    Color::Magenta,
    Color::Blue,
    Color::Red,
    Color::Green,
];

// ── Data types ──────────────────────────────────────────────────────────

/// A logical section in the rendered status output. Sections are built from
/// RepoInfo and rendered top-to-bottom with UTF-8 box-drawing characters.
enum Section {
    /// Working tree status (always present, may contain zero changes).
    WorkingChanges(Vec<FileChange>),
    /// A feature branch: its name and the commits it owns.
    Branch {
        name: String,
        commits: Vec<CommitInfo>,
    },
    /// Commits on the integration line that don't belong to any feature branch.
    Loose(Vec<CommitInfo>),
    /// The upstream tracking branch / common base marker at the bottom of the status.
    Upstream(UpstreamInfo),
}

// ── Public API ──────────────────────────────────────────────────────────

/// Build sections from repo data and render them as a UTF-8 graph string.
pub fn render(info: RepoInfo) -> String {
    let ids = IdAllocator::new(info.collect_entities());
    let sections = build_sections(info);
    render_sections(&sections, &ids)
}

// ── Section building ────────────────────────────────────────────────────

/// Group commits into sections: working changes, feature branches, loose
/// commits, and the upstream marker. Commits are assigned to a branch when
/// they follow a branch tip in topological order.
fn build_sections(info: RepoInfo) -> Vec<Section> {
    // Build a set of branch tip OIDs for quick lookup.
    let branch_tip_set: HashSet<git2::Oid> = info.branches.iter().map(|b| b.tip_oid).collect();

    // Build a parent lookup from the commit list so we can walk ancestry chains.
    let parent_map: HashMap<git2::Oid, Option<git2::Oid>> =
        info.commits.iter().map(|c| (c.oid, c.parent_oid)).collect();

    // For each branch, compute the set of commits belonging to it.
    // Walk from the branch tip along parent links, stopping at:
    //   - A commit not in our range (outside upstream..HEAD)
    //   - Another branch's tip (stacked-branch boundary)
    let mut commit_to_branch: HashMap<git2::Oid, String> = HashMap::new();
    for b in &info.branches {
        let mut current = Some(b.tip_oid);
        let mut is_tip = true;
        while let Some(oid) = current {
            if !parent_map.contains_key(&oid) {
                break; // outside our commit range
            }
            // Stop at another branch's tip (but not at our own tip)
            if !is_tip && branch_tip_set.contains(&oid) {
                break;
            }
            is_tip = false;
            commit_to_branch.insert(oid, b.name.clone());
            current = parent_map.get(&oid).and_then(|p| *p);
        }
    }

    let mut sections: Vec<Section> = Vec::new();

    sections.push(Section::WorkingChanges(info.working_changes));

    // Separate commits into loose and branch groups, preserving topo order within each.
    let mut loose_commits: Vec<CommitInfo> = Vec::new();
    let mut branch_sections: Vec<Section> = Vec::new();

    let mut commits = info.commits.into_iter().peekable();
    while let Some(commit) = commits.next() {
        if let Some(branch_name) = commit_to_branch.get(&commit.oid) {
            let name = branch_name.clone();
            let mut branch_commits = vec![commit];

            // Collect subsequent commits that belong to the same branch.
            while let Some(next) = commits.peek() {
                if commit_to_branch.get(&next.oid) == Some(&name) {
                    branch_commits.push(commits.next().unwrap());
                } else {
                    break;
                }
            }
            branch_sections.push(Section::Branch {
                name,
                commits: branch_commits,
            });
        } else {
            loose_commits.push(commit);
            while let Some(next) = commits.peek() {
                if commit_to_branch.contains_key(&next.oid) {
                    break;
                }
                loose_commits.push(commits.next().unwrap());
            }
        }
    }

    // Loose commits first, then feature branches.
    if !loose_commits.is_empty() {
        sections.push(Section::Loose(loose_commits));
    }
    sections.extend(branch_sections);

    sections.push(Section::Upstream(info.upstream));

    sections
}

/// Check if the next branch section is stacked on top of the current one.
/// Two branches are stacked if the first commit of the next branch is a parent
/// of the last commit of the current branch.
fn is_stacked_with_next(sections: &[Section], idx: usize) -> bool {
    let Section::Branch { commits, .. } = &sections[idx] else {
        return false;
    };
    let Some(Section::Branch {
        commits: next_commits,
        ..
    }) = sections.get(idx + 1)
    else {
        return false;
    };
    let Some(last) = commits.last() else {
        return false;
    };
    let Some(next_first) = next_commits.first() else {
        return false;
    };
    last.parent_oid == Some(next_first.oid)
}

// ── Rendering ───────────────────────────────────────────────────────────

/// Render sections as a UTF-8 graph. Stacked branches (where the last commit
/// of a branch is a parent of the first commit of the next) are connected
/// with `││` and `│├─`, while independent branches get `├╯` then `│╭─`.
fn render_sections(sections: &[Section], ids: &IdAllocator) -> String {
    let mut out = String::new();
    let last_idx = sections.len() - 1;
    let mut branch_color_idx: usize = 0;

    for (idx, section) in sections.iter().enumerate() {
        match section {
            Section::WorkingChanges(changes) => {
                render_working_changes(&mut out, changes, ids);
            }
            Section::Branch { name, commits } => {
                let dot_color = BRANCH_DOT_COLORS[branch_color_idx % BRANCH_DOT_COLORS.len()];
                branch_color_idx += 1;

                let prev_stacked = idx > 0 && is_stacked_with_next(sections, idx - 1);
                let next_stacked = is_stacked_with_next(sections, idx);

                render_branch(
                    &mut out,
                    name,
                    commits,
                    dot_color,
                    prev_stacked,
                    next_stacked,
                    idx < last_idx,
                    ids,
                );
            }
            Section::Loose(commits) => {
                render_loose(&mut out, commits, idx < last_idx, ids);
            }
            Section::Upstream(info) => {
                render_upstream(&mut out, info);
            }
        }
    }

    out
}

fn render_working_changes(out: &mut String, changes: &[FileChange], ids: &IdAllocator) {
    writeln!(
        out,
        "{} {} {}{}{}",
        "╭─".color(COLOR_GRAPH),
        ids.get_unstaged().color(COLOR_SHORTID).underline(),
        "[".color(COLOR_DIM),
        "unstaged changes".color(COLOR_LABEL),
        "]".color(COLOR_DIM)
    )
    .unwrap();
    if changes.is_empty() {
        writeln!(
            out,
            "{}   {}",
            "│".color(COLOR_GRAPH),
            "no changes".color(COLOR_DIM)
        )
        .unwrap();
    } else {
        for change in changes {
            writeln!(
                out,
                "{}   {} {} {}",
                "│".color(COLOR_GRAPH),
                ids.get_file(&change.path).color(COLOR_SHORTID).underline(),
                change.status.to_string().color(COLOR_DIM),
                change.path.color(COLOR_MESSAGE)
            )
            .unwrap();
        }
    }
    writeln!(out, "{}", "│".color(COLOR_GRAPH)).unwrap();
}

#[allow(clippy::too_many_arguments)]
fn render_branch(
    out: &mut String,
    name: &str,
    commits: &[CommitInfo],
    dot_color: Color,
    prev_stacked: bool,
    next_stacked: bool,
    more_sections: bool,
    ids: &IdAllocator,
) {
    let branch_id = ids.get_branch(name);
    let connector = if prev_stacked {
        "│├─"
    } else {
        "│╭─"
    };
    writeln!(
        out,
        "{} {} {}{}{}",
        connector.color(COLOR_GRAPH),
        branch_id.color(COLOR_SHORTID).underline(),
        "[".color(COLOR_DIM),
        name.color(COLOR_BRANCH).bold(),
        "]".color(COLOR_DIM)
    )
    .unwrap();

    for commit in commits {
        let sid = ids.get_commit(commit.oid);
        let rest: String = commit.short_id.chars().skip(sid.len()).collect();
        writeln!(
            out,
            "{}{}   {}{} {}",
            "│".color(COLOR_GRAPH),
            "●".color(dot_color),
            sid.color(COLOR_SHORTID).underline(),
            rest.color(COLOR_DIM),
            commit.message.color(COLOR_MESSAGE)
        )
        .unwrap();
    }
    if next_stacked {
        writeln!(out, "{}", "││".color(COLOR_GRAPH)).unwrap();
    } else {
        writeln!(out, "{}", "├╯".color(COLOR_GRAPH)).unwrap();
        if more_sections {
            writeln!(out, "{}", "│".color(COLOR_GRAPH)).unwrap();
        }
    }
}

fn render_loose(out: &mut String, commits: &[CommitInfo], more_sections: bool, ids: &IdAllocator) {
    for commit in commits {
        let sid = ids.get_commit(commit.oid);
        let rest: String = commit.short_id.chars().skip(sid.len()).collect();
        writeln!(
            out,
            "{}   {}{} {}",
            "●".color(COLOR_GRAPH),
            sid.color(COLOR_SHORTID).underline(),
            rest.color(COLOR_DIM),
            commit.message.color(COLOR_MESSAGE)
        )
        .unwrap();
    }
    if more_sections {
        writeln!(out, "{}", "│".color(COLOR_GRAPH)).unwrap();
    }
}

fn render_upstream(out: &mut String, info: &UpstreamInfo) {
    if info.commits_ahead > 0 {
        let count_text = format!(
            "\u{23EB} {} new commit{}",
            info.commits_ahead,
            if info.commits_ahead == 1 { "" } else { "s" }
        )
        .color(COLOR_MESSAGE);
        writeln!(
            out,
            "{}{}  {}{}{} {}",
            "│".color(COLOR_GRAPH),
            "●".color(COLOR_GRAPH),
            "[".color(COLOR_DIM),
            info.label.color(COLOR_BRANCH).bold(),
            "]".color(COLOR_DIM),
            count_text
        )
        .unwrap();
        writeln!(
            out,
            "{} {} {} {} {}",
            "├╯".color(COLOR_GRAPH),
            info.base_short_id.color(COLOR_DIM),
            "(common base)".color(COLOR_LABEL),
            info.base_date.color(COLOR_DIM),
            info.base_message.color(COLOR_DIM)
        )
        .unwrap();
    } else {
        writeln!(
            out,
            "{} {} {} {}{}{} {}",
            "●".color(COLOR_GRAPH),
            info.base_short_id.color(COLOR_DIM),
            "(upstream)".color(COLOR_LABEL),
            "[".color(COLOR_DIM),
            info.label.color(COLOR_BRANCH).bold(),
            "]".color(COLOR_DIM),
            info.base_message.color(COLOR_DIM)
        )
        .unwrap();
    }
}

#[cfg(test)]
#[path = "graph_test.rs"]
mod tests;
