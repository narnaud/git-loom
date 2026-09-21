//! Flat row model for the interactive status tree of `loom tui`.
//!
//! Rows are rebuilt from the graph sections whenever the expansion state
//! changes; the expansion and selection states are keyed by [`Row::key`] so
//! they survive rebuilds.

use std::collections::HashSet;

use crate::core::graph::{self, Section};
use crate::core::repo::RemoteStatus;
use crate::core::shortid::IdAllocator;

/// Key of the local-changes section in the expansion set.
pub(crate) const LOCAL_CHANGES_KEY: &str = "local";

/// Object name of the commit `c` is placing; also its [`Row::key`]. The null
/// OID names no real object, so that row carries no `target`: it is drawn,
/// not acted on.
pub(crate) const PENDING_COMMIT_OID: git2::Oid = git2::Oid::ZERO_SHA1;

/// [`Row::key`] of the row naming branch `name`.
pub(crate) fn branch_key(name: &str) -> String {
    format!("br:{}", name)
}

/// [`Row::key`] of the row naming the working file at `path`.
pub(crate) fn working_file_key(path: &str) -> String {
    format!("wf:{}", path)
}

/// What the gutter marker on a row says. `Source` and `Covered` occur only
/// while a target is being picked, and outrank `Selected`: what the pending
/// command takes matters more than what is selected (Spec 020).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RowMark {
    None,
    Selected,
    /// A row the pending command names.
    Source,
    /// A row a named source subsumes without naming it: a file under a `zz`
    /// header, or a staged file the index carries in.
    Covered,
}

/// The kind of thing a selection holds. A selection never mixes classes: no
/// loom command takes a heterogeneous target list (Spec 020).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SelectionClass {
    /// The `[local changes]` header — `zz`, which subsumes the files under it.
    LocalChanges,
    WorkingFile,
    Branch,
    Commit,
    CommitFile,
}

impl SelectionClass {
    /// Plural name of the class, for the status-bar notice.
    pub fn label(self) -> &'static str {
        match self {
            SelectionClass::LocalChanges => "local changes",
            SelectionClass::WorkingFile => "files",
            SelectionClass::Branch => "branches",
            SelectionClass::Commit => "commits",
            SelectionClass::CommitFile => "commit files",
        }
    }
}

/// What a tree row represents, with the data needed to render it.
pub(crate) enum RowKind {
    /// The `[local changes]` header. Expandable to its files.
    LocalChanges { count: usize },
    /// A staged/unstaged/untracked file under `[local changes]`.
    WorkingFile {
        path: String,
        index: char,
        worktree: char,
    },
    /// A branch name line (`│╭─ b0 [name] ✓`). Co-located branches produce
    /// one row per name. `range` is `(parent_of_oldest, tip)` for the diff of
    /// all commits the branch owns; `None` when the branch owns no commits.
    BranchName {
        name: String,
        remote: Option<RemoteStatus>,
        connector: &'static str,
        range: Option<(String, String)>,
    },
    /// A commit, on a feature branch or on the integration line (loose).
    /// Expandable to the files it changed.
    Commit {
        oid: git2::Oid,
        message: String,
        /// Index into the theme's rotating dot colors; `None` = loose commit.
        dot_color: Option<usize>,
        file_count: usize,
    },
    /// A file changed by a commit.
    CommitFile {
        oid: git2::Oid,
        path: String,
        index: char,
        worktree: char,
        on_branch: bool,
    },
    /// The upstream / common-base marker.
    Upstream {
        /// Full name of the upstream ref (e.g. "origin/main").
        label: String,
        base_short_id: String,
        base_message: String,
        commits_ahead: usize,
    },
    /// A dimmed context commit before the base.
    Context {
        short_hash: String,
        date: String,
        message: String,
    },
    /// Pure graph structure (`│`, `├╯`, ...) — not focusable.
    Spacer(&'static str),
}

/// One line of the interactive status tree.
pub(crate) struct Row {
    pub kind: RowKind,
    /// Loom short ID shown on the row (empty when the row has none).
    pub sid: String,
    /// Argument to pass to loom commands for this row: a full commit hash, a
    /// branch name, or a short ID for files. `None` = not actionable.
    pub target: Option<String>,
    /// Stable identity across rebuilds, used for expansion and selection.
    pub key: String,
    /// Whether the cursor can rest on this row.
    pub focusable: bool,
    /// Whether the row can be expanded to child rows.
    pub expandable: bool,
    pub expanded: bool,
}

impl Row {
    /// Which selection the row can join; `None` = not selectable.
    pub fn selection_class(&self) -> Option<SelectionClass> {
        Some(match self.kind {
            RowKind::LocalChanges { .. } => SelectionClass::LocalChanges,
            RowKind::WorkingFile { .. } => SelectionClass::WorkingFile,
            RowKind::BranchName { .. } => SelectionClass::Branch,
            RowKind::Commit { .. } => SelectionClass::Commit,
            RowKind::CommitFile { .. } => SelectionClass::CommitFile,
            RowKind::Upstream { .. } | RowKind::Context { .. } | RowKind::Spacer(_) => {
                return None;
            }
        })
    }

    fn structural(kind: RowKind) -> Self {
        Row {
            kind,
            sid: String::new(),
            target: None,
            key: String::new(),
            focusable: false,
            expandable: false,
            expanded: false,
        }
    }

    fn info(kind: RowKind, key: String) -> Self {
        Row {
            key,
            focusable: true,
            ..Row::structural(kind)
        }
    }
}

/// Build the flat row list from graph sections. `expanded` holds the keys of
/// currently expanded rows.
pub(crate) fn build_rows(
    sections: &[Section],
    ids: &IdAllocator,
    expanded: &HashSet<String>,
) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let last_idx = sections.len().saturating_sub(1);
    let mut branch_color_idx: usize = 0;

    for (idx, section) in sections.iter().enumerate() {
        match section {
            Section::WorkingChanges(changes) => {
                let key = LOCAL_CHANGES_KEY.to_string();
                let is_expanded = expanded.contains(&key);
                rows.push(Row {
                    kind: RowKind::LocalChanges {
                        count: changes.len(),
                    },
                    sid: ids.get_unstaged().to_string(),
                    target: Some(ids.get_unstaged().to_string()),
                    key,
                    focusable: true,
                    expandable: !changes.is_empty(),
                    expanded: is_expanded,
                });
                if is_expanded {
                    for change in changes {
                        rows.push(Row {
                            kind: RowKind::WorkingFile {
                                path: change.path.clone(),
                                index: change.index,
                                worktree: change.worktree,
                            },
                            sid: ids.get_file(&change.path).to_string(),
                            target: Some(ids.get_file(&change.path).to_string()),
                            key: working_file_key(&change.path),
                            focusable: true,
                            expandable: false,
                            expanded: false,
                        });
                    }
                }
                rows.push(Row::structural(RowKind::Spacer("│")));
            }
            Section::Branch { names, commits } => {
                let dot_color = branch_color_idx;
                branch_color_idx += 1;

                let prev_stacked = idx > 0 && graph::is_stacked_with_next(sections, idx - 1);
                let next_stacked = graph::is_stacked_with_next(sections, idx);

                // A branch whose oldest commit is a root commit diffs against
                // git's well-known empty tree (SHA-1).
                const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
                let range = match (commits.first(), commits.last()) {
                    (Some(first), Some(last)) => Some((
                        last.parent_oid
                            .map_or_else(|| EMPTY_TREE.to_string(), |p| p.to_string()),
                        first.oid.to_string(),
                    )),
                    _ => None,
                };

                for (i, (name, remote)) in names.iter().enumerate() {
                    let connector = if i == 0 && !prev_stacked {
                        "│╭─"
                    } else {
                        "│├─"
                    };
                    rows.push(Row {
                        kind: RowKind::BranchName {
                            name: name.clone(),
                            remote: remote.clone(),
                            connector,
                            range: range.clone(),
                        },
                        sid: ids.get_branch(name).to_string(),
                        target: Some(name.clone()),
                        key: branch_key(name),
                        focusable: true,
                        expandable: false,
                        expanded: false,
                    });
                }

                push_commit_rows(&mut rows, commits, Some(dot_color), ids, expanded);

                if next_stacked {
                    rows.push(Row::structural(RowKind::Spacer("││")));
                } else {
                    rows.push(Row::structural(RowKind::Spacer("├╯")));
                    if idx < last_idx {
                        rows.push(Row::structural(RowKind::Spacer("│")));
                    }
                }
            }
            Section::Loose(commits) => {
                push_commit_rows(&mut rows, commits, None, ids, expanded);
                if idx < last_idx {
                    rows.push(Row::structural(RowKind::Spacer("│")));
                }
            }
            Section::Upstream(info) => {
                rows.push(Row::info(
                    RowKind::Upstream {
                        label: info.label.clone(),
                        base_short_id: info.base_short_id.clone(),
                        base_message: info.base_message.clone(),
                        commits_ahead: info.commits_ahead,
                    },
                    format!("up:{}", info.merge_base_oid),
                ));
            }
            Section::Context(commits) => {
                for commit in commits {
                    rows.push(Row::info(
                        RowKind::Context {
                            short_hash: commit.short_hash.clone(),
                            date: commit.date.clone(),
                            message: commit.message.clone(),
                        },
                        format!("ctx:{}", commit.short_hash),
                    ));
                }
            }
        }
    }

    rows
}

fn push_commit_rows(
    rows: &mut Vec<Row>,
    commits: &[crate::core::repo::CommitInfo],
    dot_color: Option<usize>,
    ids: &IdAllocator,
    expanded: &HashSet<String>,
) {
    for commit in commits {
        let sid = ids.get_commit(commit.oid).to_string();
        let key = commit.oid.to_string();
        let is_expanded = expanded.contains(&key);
        rows.push(Row {
            kind: RowKind::Commit {
                oid: commit.oid,
                message: commit.message.clone(),
                dot_color,
                file_count: commit.files.len(),
            },
            sid: sid.clone(),
            target: (commit.oid != PENDING_COMMIT_OID).then(|| commit.oid.to_string()),
            key,
            focusable: true,
            expandable: !commit.files.is_empty(),
            expanded: is_expanded,
        });
        if is_expanded {
            for (i, file) in commit.files.iter().enumerate() {
                let file_sid = format!("{}:{}", sid, i);
                rows.push(Row {
                    kind: RowKind::CommitFile {
                        oid: commit.oid,
                        path: file.path.clone(),
                        index: file.index,
                        worktree: file.worktree,
                        on_branch: dot_color.is_some(),
                    },
                    sid: file_sid.clone(),
                    target: Some(file_sid.clone()),
                    key: format!("{}:{}", commit.oid, i),
                    focusable: true,
                    expandable: false,
                    expanded: false,
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "status_tree_test.rs"]
mod tests;
