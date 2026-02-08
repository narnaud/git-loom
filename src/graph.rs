use crate::git::{CommitInfo, FileChange, RepoInfo, UpstreamInfo};
use std::collections::HashMap;

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

/// Build sections from repo data and render them as a UTF-8 graph string.
pub fn render(info: RepoInfo) -> String {
    let sections = build_sections(info);
    render_sections(&sections)
}

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

/// Render sections as a UTF-8 graph. Stacked branches (where the last commit
/// of a branch is a parent of the first commit of the next) are connected
/// with `││` and `│├─`, while independent branches get `├╯` then `│╭─`.
fn render_sections(sections: &[Section]) -> String {
    let mut out = String::new();
    let last_idx = sections.len() - 1;

    for (idx, section) in sections.iter().enumerate() {
        match section {
            Section::WorkingChanges(changes) => {
                out.push_str("╭─ [unstaged changes]\n");
                if changes.is_empty() {
                    out.push_str("│   no changes\n");
                } else {
                    for change in changes {
                        out.push_str(&format!("│   {} {}\n", change.status, change.path));
                    }
                }
                out.push_str("│\n");
            }
            Section::Branch { name, commits } => {
                let prev_stacked = idx > 0 && is_stacked_with_next(sections, idx - 1);
                let next_stacked = is_stacked_with_next(sections, idx);

                if prev_stacked {
                    out.push_str(&format!("│├─ [{}]\n", name));
                } else {
                    out.push_str(&format!("│╭─ [{}]\n", name));
                }
                for commit in commits {
                    out.push_str(&format!("│●   {} {}\n", commit.short_id, commit.message));
                }
                if next_stacked {
                    out.push_str("││\n");
                } else {
                    out.push_str("├╯\n");
                    if idx < last_idx {
                        out.push_str("│\n");
                    }
                }
            }
            Section::Loose(commits) => {
                for commit in commits {
                    out.push_str(&format!("●   {} {}\n", commit.short_id, commit.message));
                }
                if idx < last_idx {
                    out.push_str("│\n");
                }
            }
            Section::Upstream(info) => {
                if info.commits_ahead > 0 {
                    out.push_str(&format!(
                        "│●  [{}] \u{23EB} {} new commit{}\n",
                        info.label,
                        info.commits_ahead,
                        if info.commits_ahead == 1 { "" } else { "s" }
                    ));
                    out.push_str(&format!(
                        "├╯ {} (common base) {} {}\n",
                        info.base_short_id, info.base_date, info.base_message
                    ));
                } else {
                    out.push_str(&format!(
                        "● {} (upstream) [{}] {}\n",
                        info.base_short_id, info.label, info.base_message
                    ));
                }
            }
        }
    }

    out
}

#[cfg(test)]
#[path = "graph_test.rs"]
mod tests;
