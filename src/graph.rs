use crate::git::{CommitInfo, ContextCommit, FileChange, RemoteStatus, RepoInfo, UpstreamInfo};
use crate::shortid::IdAllocator;
use colored::{Color, Colorize};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use terminal_size::{Width, terminal_size};

// ── Theme ────────────────────────────────────────────────────────────────

/// Color palette for graph output. Use [`Theme::dark`] or [`Theme::light`].
pub struct Theme {
    /// Graph structure: lines, connectors, dots on the integration line.
    pub graph: Color,
    /// Branch names in brackets (local and remote).
    pub branch: Color,
    /// Labels like (upstream) and (common base).
    pub label: Color,
    /// Dimmed secondary text: messages, dates, file changes, "no changes".
    pub dim: Color,
    /// Commit message text.
    pub message: Color,
    /// Short ID prefix (also underlined in rendering).
    pub shortid: Color,
    /// Staged (index) file status: green, matching git convention.
    pub staged: Color,
    /// Unstaged (worktree) file status: red, matching git convention.
    pub unstaged: Color,
    /// Untracked file marker and path color.
    pub untracked: Color,
    /// Branch remote tracking ref exists and is in sync.
    pub remote_synced: Color,
    /// Branch has unpushed commits ahead of its remote.
    pub remote_ahead: Color,
    /// Branch remote tracking ref is gone (deleted on remote).
    pub remote_gone: Color,
    /// Rotating colors for commit dots on feature branches.
    pub branch_dots: &'static [Color],
}

impl Theme {
    /// Dark terminal background theme (default).
    pub fn dark() -> Self {
        Theme {
            graph: Color::BrightBlack,
            branch: Color::Green,
            label: Color::Cyan,
            dim: Color::AnsiColor(240),
            message: Color::AnsiColor(248),
            shortid: Color::Blue,
            staged: Color::Green,
            unstaged: Color::Red,
            untracked: Color::Magenta,
            remote_synced: Color::Green,
            remote_ahead: Color::Yellow,
            remote_gone: Color::Red,
            branch_dots: BRANCH_DOTS,
        }
    }

    /// Light terminal background theme.
    pub fn light() -> Self {
        Theme {
            graph: Color::AnsiColor(248),
            branch: Color::Green,
            label: Color::Blue,
            dim: Color::AnsiColor(248),
            message: Color::AnsiColor(243),
            shortid: Color::Blue,
            staged: Color::Green,
            unstaged: Color::Red,
            untracked: Color::Magenta,
            remote_synced: Color::Green,
            remote_ahead: Color::Yellow,
            remote_gone: Color::Red,
            branch_dots: BRANCH_DOTS,
        }
    }
}

/// Rotating colors for commit dots on feature branches (shared by all themes).
const BRANCH_DOTS: &[Color] = &[
    Color::Yellow,
    Color::Cyan,
    Color::Magenta,
    Color::Blue,
    Color::Red,
    Color::Green,
];

/// Number of untracked files above which the display switches to multi-column layout.
const UNTRACKED_MULTICOLUMN_THRESHOLD: usize = 5;

/// Width of the graph prefix ("│   ") in visible columns.
const GRAPH_PREFIX_WIDTH: usize = 4;

// ── Data types ──────────────────────────────────────────────────────────

/// Display configuration for the status graph renderer.
pub struct RenderOpts {
    /// Terminal column width, used for multi-column untracked layout.
    /// `None` means non-TTY (e.g. pipe) — fall back to single-column.
    pub terminal_width: Option<u16>,
    /// Color theme for the graph output.
    pub theme: Theme,
}

/// A logical section in the rendered status output. Sections are built from
/// RepoInfo and rendered top-to-bottom with UTF-8 box-drawing characters.
enum Section {
    /// Working tree status (always present, may contain zero changes).
    WorkingChanges(Vec<FileChange>),
    /// A feature branch: its name(s) and the commits it owns.
    /// Multiple names occur when several branches point to the same tip commit.
    /// Each name is paired with its remote tracking status (None = never pushed).
    Branch {
        names: Vec<(String, Option<RemoteStatus>)>,
        commits: Vec<CommitInfo>,
    },
    /// Commits on the integration line that don't belong to any feature branch.
    Loose(Vec<CommitInfo>),
    /// The upstream tracking branch / common base marker at the bottom of the status.
    Upstream(UpstreamInfo),
    /// Context commits before the base (display-only, dimmed).
    Context(Vec<ContextCommit>),
}

// ── Public API ──────────────────────────────────────────────────────────

/// Build sections from repo data and render them as a UTF-8 graph string.
pub fn render(info: RepoInfo, opts: &RenderOpts) -> String {
    let ids = IdAllocator::new(info.collect_entities());
    let sections = build_sections(info);
    render_sections(&sections, &ids, opts)
}

/// Detect terminal width and build render options for the given theme.
pub fn default_render_opts(theme: Theme) -> RenderOpts {
    RenderOpts {
        terminal_width: terminal_size().map(|(Width(w), _)| w),
        theme,
    }
}

// ── Section building ────────────────────────────────────────────────────

/// Group commits into sections: working changes, feature branches, loose
/// commits, and the upstream marker. Commits are assigned to a branch when
/// they follow a branch tip in topological order.
fn build_sections(info: RepoInfo) -> Vec<Section> {
    // Build a set of branch tip OIDs for quick lookup.
    let branch_tip_set: HashSet<git2::Oid> = info.branches.iter().map(|b| b.tip_oid).collect();

    // Group branches by tip OID to handle co-located branches (multiple
    // branch names pointing to the same commit).
    let mut tip_to_names: HashMap<git2::Oid, Vec<(String, Option<RemoteStatus>)>> = HashMap::new();
    for b in &info.branches {
        tip_to_names
            .entry(b.tip_oid)
            .or_default()
            .push((b.name.clone(), b.remote.clone()));
    }

    // Build a parent lookup from the commit list so we can walk ancestry chains.
    let parent_map: HashMap<git2::Oid, Option<git2::Oid>> =
        info.commits.iter().map(|c| (c.oid, c.parent_oid)).collect();

    // For each unique branch tip, compute the set of commits belonging to it.
    // Walk from the branch tip along parent links, stopping at:
    //   - A commit not in our range (outside upstream..HEAD)
    //   - Another branch's tip (stacked-branch boundary)
    // Use the first name in the group as the canonical name for commit assignment.
    let mut commit_to_branch: HashMap<git2::Oid, String> = HashMap::new();
    let mut seen_tips: HashSet<git2::Oid> = HashSet::new();
    for b in &info.branches {
        if !seen_tips.insert(b.tip_oid) {
            continue; // Already processed commits for this tip
        }
        let canonical_name = tip_to_names[&b.tip_oid][0].0.clone();
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
            commit_to_branch.insert(oid, canonical_name.clone());
            current = parent_map.get(&oid).and_then(|p| *p);
        }
    }

    // Map canonical name → all names for that branch group.
    // Reverse so the newest (alphabetically last) branch appears on top.
    let mut canonical_to_names: HashMap<String, Vec<(String, Option<RemoteStatus>)>> =
        HashMap::new();
    for branches in tip_to_names.values() {
        let mut reversed = branches.clone();
        reversed.reverse();
        canonical_to_names.insert(branches[0].0.clone(), reversed);
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
            let names = canonical_to_names
                .get(&name)
                .cloned()
                .unwrap_or_else(|| vec![(name.clone(), None)]);
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
                names,
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

    // Add empty sections for branches at the merge-base (no commits in range).
    let represented: HashSet<&String> = commit_to_branch.values().collect();
    for branches in canonical_to_names.values() {
        // canonical key is the pre-reversal first name; check if any name is represented
        if !branches.iter().any(|(n, _)| represented.contains(n)) {
            branch_sections.push(Section::Branch {
                names: branches.clone(),
                commits: vec![],
            });
        }
    }

    // Loose commits first, then feature branches.
    if !loose_commits.is_empty() {
        sections.push(Section::Loose(loose_commits));
    }
    sections.extend(branch_sections);

    sections.push(Section::Upstream(info.upstream));

    if !info.context_commits.is_empty() {
        sections.push(Section::Context(info.context_commits));
    }

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
fn render_sections(sections: &[Section], ids: &IdAllocator, opts: &RenderOpts) -> String {
    let mut out = String::new();
    let last_idx = sections.len() - 1;
    let mut branch_color_idx: usize = 0;

    for (idx, section) in sections.iter().enumerate() {
        match section {
            Section::WorkingChanges(changes) => {
                render_working_changes(&mut out, changes, ids, opts);
            }
            Section::Branch { names, commits } => {
                let dot_color =
                    opts.theme.branch_dots[branch_color_idx % opts.theme.branch_dots.len()];
                branch_color_idx += 1;

                let prev_stacked = idx > 0 && is_stacked_with_next(sections, idx - 1);
                let next_stacked = is_stacked_with_next(sections, idx);

                render_branch(
                    &mut out,
                    names,
                    commits,
                    dot_color,
                    prev_stacked,
                    next_stacked,
                    idx < last_idx,
                    ids,
                    &opts.theme,
                );
            }
            Section::Loose(commits) => {
                render_loose(&mut out, commits, idx < last_idx, ids, &opts.theme);
            }
            Section::Upstream(info) => {
                render_upstream(&mut out, info, &opts.theme);
            }
            Section::Context(commits) => {
                render_context(&mut out, commits, &opts.theme);
            }
        }
    }

    out
}

fn render_working_changes(
    out: &mut String,
    changes: &[FileChange],
    ids: &IdAllocator,
    opts: &RenderOpts,
) {
    let theme = &opts.theme;
    writeln!(
        out,
        "{} {} {}{}{}",
        "╭─".color(theme.graph),
        ids.get_unstaged().color(theme.shortid).underline(),
        "[".color(theme.dim),
        "local changes".color(theme.label),
        "]".color(theme.dim)
    )
    .unwrap();

    let tracked: Vec<&FileChange> = changes
        .iter()
        .filter(|f| !(f.index == '?' && f.worktree == '?'))
        .collect();
    let untracked: Vec<&FileChange> = changes
        .iter()
        .filter(|f| f.index == '?' && f.worktree == '?')
        .collect();

    if tracked.is_empty() && untracked.is_empty() {
        writeln!(
            out,
            "{}   {}",
            "│".color(theme.graph),
            "no changes".color(theme.dim)
        )
        .unwrap();
    } else {
        for change in &tracked {
            writeln!(
                out,
                "{}   {} {}{} {}",
                "│".color(theme.graph),
                ids.get_file(&change.path).color(theme.shortid).underline(),
                change.index.to_string().color(theme.staged),
                change.worktree.to_string().color(theme.unstaged),
                change.path
            )
            .unwrap();
        }
        if !untracked.is_empty() {
            render_untracked(out, &untracked, ids, opts);
        }
    }

    writeln!(out, "{}", "│".color(theme.graph)).unwrap();
}

fn render_untracked(
    out: &mut String,
    untracked: &[&FileChange],
    ids: &IdAllocator,
    opts: &RenderOpts,
) {
    if untracked.len() > UNTRACKED_MULTICOLUMN_THRESHOLD
        && let Some(width) = opts.terminal_width
    {
        render_untracked_multicolumn(out, untracked, ids, width, &opts.theme);
        return;
    }
    render_untracked_single_column(out, untracked, ids, &opts.theme);
}

fn render_untracked_single_column(
    out: &mut String,
    untracked: &[&FileChange],
    ids: &IdAllocator,
    theme: &Theme,
) {
    for change in untracked {
        writeln!(
            out,
            "{}   {} {} {}",
            "│".color(theme.graph),
            ids.get_file(&change.path).color(theme.shortid).underline(),
            " ⁕".color(theme.untracked),
            change.path
        )
        .unwrap();
    }
}

/// Render untracked files in a multi-column grid (top-to-bottom, left-to-right).
fn render_untracked_multicolumn(
    out: &mut String,
    untracked: &[&FileChange],
    ids: &IdAllocator,
    term_width: u16,
    theme: &Theme,
) {
    let available = (term_width as usize).saturating_sub(GRAPH_PREFIX_WIDTH);
    let separator = "   │ "; // 5 display columns
    let separator_width: usize = 5;

    // Compute plain-text width of each entry: "{sid}  ⁕ {path}"
    let entry_widths: Vec<usize> = untracked
        .iter()
        .map(|f| {
            let sid = ids.get_file(&f.path);
            sid.len() + 1 + 2 + 1 + f.path.len()
        })
        .collect();

    let max_entry_width = entry_widths.iter().copied().max().unwrap_or(1);
    let col_slot = max_entry_width + separator_width;

    let num_cols = (available / col_slot).max(1).min(untracked.len());
    let num_rows = untracked.len().div_ceil(num_cols);

    for row in 0..num_rows {
        write!(out, "{}   ", "│".color(theme.graph)).unwrap();
        for col in 0..num_cols {
            let idx = col * num_rows + row;
            if idx >= untracked.len() {
                break;
            }
            let f = untracked[idx];
            let sid = ids.get_file(&f.path);

            write!(
                out,
                "{} {} {}",
                sid.color(theme.shortid).underline(),
                " ⁕".color(theme.untracked),
                f.path
            )
            .unwrap();

            // Pad entry to max_entry_width, then add pipe separator
            let next_idx = (col + 1) * num_rows + row;
            if col + 1 < num_cols && next_idx < untracked.len() {
                let padding = max_entry_width.saturating_sub(entry_widths[idx]);
                write!(out, "{}{}", " ".repeat(padding), separator.color(theme.dim)).unwrap();
            }
        }
        writeln!(out).unwrap();
    }
}

#[allow(clippy::too_many_arguments)]
fn render_branch(
    out: &mut String,
    names: &[(String, Option<RemoteStatus>)],
    commits: &[CommitInfo],
    dot_color: Color,
    prev_stacked: bool,
    next_stacked: bool,
    more_sections: bool,
    ids: &IdAllocator,
    theme: &Theme,
) {
    for (i, (name, remote)) in names.iter().enumerate() {
        let branch_id = ids.get_branch(name);
        let connector = if i == 0 && !prev_stacked {
            "│╭─"
        } else {
            "│├─"
        };
        let remote_indicator = match remote {
            Some(RemoteStatus::Synced) => format!(" {}", "✓".color(theme.remote_synced)),
            Some(RemoteStatus::Ahead) => format!(" {}", "↑".color(theme.remote_ahead)),
            Some(RemoteStatus::Gone) => format!(" {}", "✗".color(theme.remote_gone)),
            None => String::new(),
        };
        writeln!(
            out,
            "{} {} {}{}{}{}",
            connector.color(theme.graph),
            branch_id.color(theme.shortid).underline(),
            "[".color(theme.dim),
            name.color(theme.branch).bold(),
            "]".color(theme.dim),
            remote_indicator,
        )
        .unwrap();
    }

    for commit in commits {
        let sid = ids.get_commit(commit.oid);
        let rest: String = commit.short_id.chars().skip(sid.len()).collect();
        writeln!(
            out,
            "{}{}    {}{} {}",
            "│".color(theme.graph),
            "●".color(dot_color),
            sid.color(theme.shortid).underline(),
            rest.color(theme.dim),
            commit.message
        )
        .unwrap();
        for (i, file) in commit.files.iter().enumerate() {
            let file_sid = format!("{}:{}", sid, i);
            writeln!(
                out,
                "{}{}      {} {}{} {}",
                "│".color(theme.graph),
                "┊".color(dot_color),
                file_sid.color(theme.shortid).underline(),
                file.index.to_string().color(theme.staged),
                file.worktree.to_string().color(theme.unstaged),
                file.path
            )
            .unwrap();
        }
    }
    if next_stacked {
        writeln!(out, "{}", "││".color(theme.graph)).unwrap();
    } else {
        writeln!(out, "{}", "├╯".color(theme.graph)).unwrap();
        if more_sections {
            writeln!(out, "{}", "│".color(theme.graph)).unwrap();
        }
    }
}

fn render_loose(
    out: &mut String,
    commits: &[CommitInfo],
    more_sections: bool,
    ids: &IdAllocator,
    theme: &Theme,
) {
    for commit in commits {
        let sid = ids.get_commit(commit.oid);
        let rest: String = commit.short_id.chars().skip(sid.len()).collect();
        writeln!(
            out,
            "{}    {}{} {}",
            "●".color(theme.graph),
            sid.color(theme.shortid).underline(),
            rest.color(theme.dim),
            commit.message
        )
        .unwrap();
        for (i, file) in commit.files.iter().enumerate() {
            let file_sid = format!("{}:{}", sid, i);
            writeln!(
                out,
                "{}       {} {}{} {}",
                "┊".color(theme.graph),
                file_sid.color(theme.shortid).underline(),
                file.index.to_string().color(theme.staged),
                file.worktree.to_string().color(theme.unstaged),
                file.path
            )
            .unwrap();
        }
    }
    if more_sections {
        writeln!(out, "{}", "│".color(theme.graph)).unwrap();
    }
}

fn render_upstream(out: &mut String, info: &UpstreamInfo, theme: &Theme) {
    if info.commits_ahead > 0 {
        let count_text = format!(
            "\u{23EB} {} new commit{}",
            info.commits_ahead,
            if info.commits_ahead == 1 { "" } else { "s" }
        )
        .color(theme.message);
        writeln!(
            out,
            "{}{}  {}{}{} {}",
            "│".color(theme.graph),
            "●".color(theme.graph),
            "[".color(theme.dim),
            info.label.color(theme.branch).bold(),
            "]".color(theme.dim),
            count_text
        )
        .unwrap();
        writeln!(
            out,
            "{} {} {} {} {}",
            "├╯".color(theme.graph),
            info.base_short_id.color(theme.dim),
            "(common base)".color(theme.label),
            info.base_date.color(theme.dim),
            info.base_message.color(theme.message)
        )
        .unwrap();
    } else {
        writeln!(
            out,
            "{} {} {} {}{}{} {}",
            "●".color(theme.graph),
            info.base_short_id.color(theme.dim),
            "(upstream)".color(theme.label),
            "[".color(theme.dim),
            info.label.color(theme.branch).bold(),
            "]".color(theme.dim),
            info.base_message.color(theme.message)
        )
        .unwrap();
    }
}

fn render_context(out: &mut String, commits: &[ContextCommit], theme: &Theme) {
    for commit in commits {
        writeln!(
            out,
            "{} {} {} {}",
            "·".color(theme.dim),
            commit.short_hash.color(theme.dim),
            commit.date.color(theme.dim),
            commit.message.color(theme.message),
        )
        .unwrap();
    }
}

#[cfg(test)]
#[path = "graph_test.rs"]
mod tests;
