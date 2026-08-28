use std::collections::HashSet;

use super::*;
use crate::core::graph::Section;
use crate::core::repo::{CommitInfo, FileChange, UpstreamInfo};
use crate::core::shortid::Entity;

fn oid(hex_char: char) -> git2::Oid {
    git2::Oid::from_str(&hex_char.to_string().repeat(40)).unwrap()
}

fn commit(id: char, parent: Option<char>, message: &str, files: Vec<FileChange>) -> CommitInfo {
    CommitInfo {
        oid: oid(id),
        short_id: hex_short(id),
        message: message.to_string(),
        parent_oid: parent.map(oid),
        files,
    }
}

fn hex_short(id: char) -> String {
    id.to_string().repeat(7)
}

fn file(path: &str, index: char, worktree: char) -> FileChange {
    FileChange {
        path: path.to_string(),
        index,
        worktree,
    }
}

fn upstream() -> UpstreamInfo {
    UpstreamInfo {
        label: "origin/main".to_string(),
        tip_oid: oid('9'),
        merge_base_oid: oid('9'),
        base_short_id: "9999999".to_string(),
        base_message: "base".to_string(),
        base_date: "2026-01-01".to_string(),
        commits_ahead: 0,
    }
}

fn sample_sections() -> Vec<Section> {
    vec![
        Section::WorkingChanges(vec![file("wt.rs", 'M', ' ')]),
        Section::Branch {
            names: vec![("feature-a".to_string(), None)],
            commits: vec![commit(
                'a',
                Some('9'),
                "Add parser",
                vec![file("src/parser.rs", 'M', ' ')],
            )],
        },
        Section::Upstream(upstream()),
    ]
}

fn sample_ids() -> IdAllocator {
    IdAllocator::new(vec![
        Entity::Unstaged,
        Entity::Branch("feature-a".to_string()),
        Entity::Commit(oid('a')),
        Entity::File("wt.rs".to_string()),
    ])
}

#[test]
fn commits_are_collapsed_by_default() {
    let sections = sample_sections();
    let ids = sample_ids();
    let mut expanded = HashSet::new();
    expanded.insert(LOCAL_CHANGES_KEY.to_string());

    let rows = build_rows(&sections, &ids, &expanded);

    // No CommitFile rows when the commit is collapsed.
    assert!(
        !rows
            .iter()
            .any(|r| matches!(r.kind, RowKind::CommitFile { .. }))
    );
    // Working file shown because local changes start expanded.
    assert!(
        rows.iter()
            .any(|r| matches!(&r.kind, RowKind::WorkingFile { path, .. } if path == "wt.rs"))
    );
    // The commit row is expandable.
    let commit_row = rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::Commit { .. }))
        .unwrap();
    assert!(commit_row.expandable);
    assert!(!commit_row.expanded);
}

#[test]
fn expanding_a_commit_adds_its_file_rows() {
    let sections = sample_sections();
    let ids = sample_ids();
    let mut expanded = HashSet::new();
    expanded.insert(oid('a').to_string());

    let rows = build_rows(&sections, &ids, &expanded);

    let file_row = rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::CommitFile { .. }))
        .expect("commit file row present when expanded");
    assert_eq!(file_row.key, format!("{}:0", oid('a')));
    // Commit-file targets are short IDs of the form "<commit sid>:<index>".
    let commit_sid = ids.get_commit(oid('a'));
    assert_eq!(
        file_row.target.as_deref(),
        Some(format!("{commit_sid}:0").as_str())
    );
}

#[test]
fn collapsed_local_changes_hides_working_files() {
    let sections = sample_sections();
    let ids = sample_ids();
    let expanded = HashSet::new();

    let rows = build_rows(&sections, &ids, &expanded);

    assert!(
        !rows
            .iter()
            .any(|r| matches!(r.kind, RowKind::WorkingFile { .. }))
    );
}

#[test]
fn row_keys_are_unique() {
    let sections = sample_sections();
    let ids = sample_ids();
    let mut expanded = HashSet::new();
    expanded.insert(LOCAL_CHANGES_KEY.to_string());
    expanded.insert(oid('a').to_string());

    let rows = build_rows(&sections, &ids, &expanded);

    let keys: Vec<&str> = rows
        .iter()
        .filter(|r| r.focusable)
        .map(|r| r.key.as_str())
        .collect();
    let unique: HashSet<&str> = keys.iter().copied().collect();
    assert_eq!(keys.len(), unique.len());
}

#[test]
fn commit_targets_are_full_hashes_and_branch_targets_are_names() {
    let sections = sample_sections();
    let ids = sample_ids();
    let rows = build_rows(&sections, &ids, &HashSet::new());

    let commit_row = rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::Commit { .. }))
        .unwrap();
    assert_eq!(
        commit_row.target.as_deref(),
        Some(oid('a').to_string().as_str())
    );

    let branch_row = rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::BranchName { .. }))
        .unwrap();
    assert_eq!(branch_row.target.as_deref(), Some("feature-a"));
}

#[test]
fn branch_range_spans_owned_commits() {
    let sections = vec![
        Section::WorkingChanges(vec![]),
        Section::Branch {
            names: vec![("feature-a".to_string(), None)],
            commits: vec![
                commit('b', Some('a'), "newest", vec![]),
                commit('a', Some('9'), "oldest", vec![]),
            ],
        },
        Section::Upstream(upstream()),
    ];
    let ids = sample_ids();
    let rows = build_rows(&sections, &ids, &HashSet::new());

    let RowKind::BranchName { range, .. } = &rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::BranchName { .. }))
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    let (base, tip) = range.as_ref().unwrap();
    assert_eq!(base, &oid('9').to_string());
    assert_eq!(tip, &oid('b').to_string());
}

#[test]
fn spacers_are_not_focusable() {
    let sections = sample_sections();
    let ids = sample_ids();
    let rows = build_rows(&sections, &ids, &HashSet::new());

    for row in &rows {
        if matches!(row.kind, RowKind::Spacer(_)) {
            assert!(!row.focusable);
            assert!(!row.selectable);
        }
    }
}

#[test]
fn upstream_row_is_focusable_but_not_selectable() {
    let sections = sample_sections();
    let ids = sample_ids();
    let rows = build_rows(&sections, &ids, &HashSet::new());

    let row = rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::Upstream { .. }))
        .unwrap();
    assert!(row.focusable);
    assert!(!row.selectable);
    assert!(row.target.is_none());
}
