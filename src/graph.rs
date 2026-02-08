use crate::git::{CommitInfo, FileChange, RepoInfo, UpstreamInfo};
use colored::{Color, Colorize};
use std::collections::HashMap;

// ── Color palette (edit these to change the theme) ──────────────────────

/// Graph structure: lines, connectors, dots on the integration line.
const COLOR_GRAPH: Color = Color::BrightBlack;
/// Commit hashes.
const COLOR_HASH: Color = Color::Blue;
/// Branch names in brackets (local and remote).
const COLOR_BRANCH: Color = Color::Green;
/// Labels like (upstream) and (common base).
const COLOR_LABEL: Color = Color::Cyan;
/// Dimmed secondary text: messages, dates, file changes, "no changes".
const COLOR_DIM: Color = Color::AnsiColor(240);
/// Colors for the commit message text
const COLOR_MESSAGE: Color = Color::AnsiColor(248);

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
    let sections = build_sections(info);
    render_sections(&sections)
}

// ── Section building ────────────────────────────────────────────────────

/// Group commits into sections: working changes, feature branches, loose
/// commits, and the upstream marker. Commits are assigned to a branch when
/// they follow a branch tip in topological order.
fn build_sections(info: RepoInfo) -> Vec<Section> {
    let mut branch_tips: HashMap<git2::Oid, String> = HashMap::new();
    for b in &info.branches {
        branch_tips.insert(b.tip_oid, b.name.clone());
    }

    let mut sections: Vec<Section> = Vec::new();

    sections.push(Section::WorkingChanges(info.working_changes));

    // Walk commits top-to-bottom. When we hit a branch tip, collect
    // commits into that branch section until the next branch tip.
    // Commits not belonging to any branch go into "loose" sections.
    let mut commits = info.commits.into_iter().peekable();
    while let Some(commit) = commits.next() {
        if let Some(branch_name) = branch_tips.get(&commit.oid) {
            let name = branch_name.clone();
            let mut branch_commits = vec![commit];

            while let Some(next) = commits.peek() {
                if branch_tips.contains_key(&next.oid) {
                    break;
                }
                branch_commits.push(commits.next().unwrap());
            }
            sections.push(Section::Branch {
                name,
                commits: branch_commits,
            });
        } else {
            let mut loose = vec![commit];
            while let Some(next) = commits.peek() {
                if branch_tips.contains_key(&next.oid) {
                    break;
                }
                loose.push(commits.next().unwrap());
            }
            sections.push(Section::Loose(loose));
        }
    }

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
fn render_sections(sections: &[Section]) -> String {
    let mut out = String::new();
    let last_idx = sections.len() - 1;
    let mut branch_color_idx: usize = 0;

    for (idx, section) in sections.iter().enumerate() {
        match section {
            Section::WorkingChanges(changes) => {
                render_working_changes(&mut out, changes);
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
                );
            }
            Section::Loose(commits) => {
                render_loose(&mut out, commits, idx < last_idx);
            }
            Section::Upstream(info) => {
                render_upstream(&mut out, info);
            }
        }
    }

    out
}

fn render_working_changes(out: &mut String, changes: &[FileChange]) {
    out.push_str(&format!(
        "{} {}{}{}\n",
        "╭─".color(COLOR_GRAPH),
        "[".color(COLOR_DIM), "unstaged changes".color(COLOR_LABEL), "]".color(COLOR_DIM)
    ));
    if changes.is_empty() {
        out.push_str(&format!(
            "{}   {}\n",
            "│".color(COLOR_GRAPH),
            "no changes".color(COLOR_DIM)
        ));
    } else {
        for change in changes {
            out.push_str(&format!(
                "{}   {} {}\n",
                "│".color(COLOR_GRAPH),
                change.status.to_string().color(COLOR_DIM),
                change.path.color(COLOR_MESSAGE)
            ));
        }
    }
    out.push_str(&format!("{}\n", "│".color(COLOR_GRAPH)));
}

fn render_branch(
    out: &mut String,
    name: &str,
    commits: &[CommitInfo],
    dot_color: Color,
    prev_stacked: bool,
    next_stacked: bool,
    more_sections: bool,
) {
    let branch_label = format!("{}{}{}", "[".color(COLOR_DIM), name.color(COLOR_BRANCH).bold(), "]".color(COLOR_DIM));
    if prev_stacked {
        out.push_str(&format!(
            "{} {}\n",
            "│├─".color(COLOR_GRAPH),
            branch_label
        ));
    } else {
        out.push_str(&format!(
            "{} {}\n",
            "│╭─".color(COLOR_GRAPH),
            branch_label
        ));
    }
    for commit in commits {
        out.push_str(&format!(
            "{}{}   {} {}\n",
            "│".color(COLOR_GRAPH),
            "●".color(dot_color),
            commit.short_id.color(COLOR_HASH),
            commit.message.color(COLOR_MESSAGE)
        ));
    }
    if next_stacked {
        out.push_str(&format!("{}\n", "││".color(COLOR_GRAPH)));
    } else {
        out.push_str(&format!("{}\n", "├╯".color(COLOR_GRAPH)));
        if more_sections {
            out.push_str(&format!("{}\n", "│".color(COLOR_GRAPH)));
        }
    }
}

fn render_loose(out: &mut String, commits: &[CommitInfo], more_sections: bool) {
    for commit in commits {
        out.push_str(&format!(
            "{}   {} {}\n",
            "●".color(COLOR_GRAPH),
            commit.short_id.color(COLOR_HASH),
            commit.message.color(COLOR_MESSAGE)
        ));
    }
    if more_sections {
        out.push_str(&format!("{}\n", "│".color(COLOR_GRAPH)));
    }
}

fn render_upstream(out: &mut String, info: &UpstreamInfo) {
    if info.commits_ahead > 0 {
        let label = format!("{}{}{}", "[".color(COLOR_DIM), info.label.color(COLOR_BRANCH).bold(), "]".color(COLOR_DIM));
        let count_text = format!(
            "\u{23EB} {} new commit{}",
            info.commits_ahead,
            if info.commits_ahead == 1 { "" } else { "s" }
        ).color(COLOR_MESSAGE);
        out.push_str(&format!(
            "{}{}  {} {}\n",
            "│".color(COLOR_GRAPH),
            "●".color(COLOR_GRAPH),
            label,
            count_text
        ));
        out.push_str(&format!(
            "{} {} {} {} {}\n",
            "├╯".color(COLOR_GRAPH),
            info.base_short_id.color(COLOR_HASH),
            "(common base)".color(COLOR_LABEL),
            info.base_date.color(COLOR_DIM),
            info.base_message.color(COLOR_DIM)
        ));
    } else {
        let label = format!("{}{}{}", "[".color(COLOR_DIM), info.label.color(COLOR_BRANCH).bold(), "]".color(COLOR_DIM));
        out.push_str(&format!(
            "{} {} {} {} {}\n",
            "●".color(COLOR_GRAPH),
            info.base_short_id.color(COLOR_HASH),
            "(upstream)".color(COLOR_LABEL),
            label,
            info.base_message.color(COLOR_DIM)
        ));
    }
}

#[cfg(test)]
#[path = "graph_test.rs"]
mod tests;
