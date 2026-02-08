use git2::Oid;

use crate::git::{BranchInfo, CommitInfo, FileChange, RepoInfo};
use crate::graph;

/// Helper to create a fake OID from a single byte (padded to 20 bytes).
fn oid(byte: u8) -> Oid {
    let mut bytes = [0u8; 20];
    bytes[0] = byte;
    Oid::from_bytes(&bytes).unwrap()
}

fn commit(byte: u8, message: &str, parent: Option<u8>) -> CommitInfo {
    CommitInfo {
        oid: oid(byte),
        short_id: format!("{:07x}", byte),
        message: message.to_string(),
        parent_oid: parent.map(oid),
    }
}

fn base_info() -> RepoInfo {
    RepoInfo {
        upstream_short_id: "aaa0000".to_string(),
        upstream_label: "origin/main".to_string(),
        commits: vec![],
        branches: vec![],
        working_changes: vec![],
    }
}

#[test]
fn no_commits_no_changes() {
    let output = graph::render(base_info());
    assert_eq!(
        output,
        "\
╭─ [unstaged changes]
│   no changes
│
● aaa0000 (upstream) origin/main
"
    );
}

#[test]
fn working_changes_shown() {
    let mut info = base_info();
    info.working_changes = vec![
        FileChange {
            path: "src/main.rs".to_string(),
            status: 'M',
        },
        FileChange {
            path: "new_file.txt".to_string(),
            status: 'A',
        },
    ];

    let output = graph::render(info);
    assert!(output.starts_with("╭─ [unstaged changes]\n│   M src/main.rs\n│   A new_file.txt\n"));
}

#[test]
fn single_branch() {
    let mut info = base_info();
    // Commits: A2 -> A1 -> upstream
    info.commits = vec![commit(2, "A2", Some(1)), commit(1, "A1", None)];
    info.branches = vec![BranchInfo {
        name: "feature-a".to_string(),
        tip_oid: oid(2),
    }];

    let output = graph::render(info);
    assert_eq!(
        output,
        "\
╭─ [unstaged changes]
│   no changes
│
│╭─ [feature-a]
│●   0000002 A2
│●   0000001 A1
├╯
│
● aaa0000 (upstream) origin/main
"
    );
}

#[test]
fn independent_branches() {
    let mut info = base_info();
    // feature-b: B1 -> upstream
    // feature-a: A1 -> upstream
    // Topological order: B1, A1 (both have upstream as parent, not each other)
    info.commits = vec![commit(2, "B1", None), commit(1, "A1", None)];
    info.branches = vec![
        BranchInfo {
            name: "feature-b".to_string(),
            tip_oid: oid(2),
        },
        BranchInfo {
            name: "feature-a".to_string(),
            tip_oid: oid(1),
        },
    ];

    let output = graph::render(info);
    // B1's parent is NOT A1, so they should be independent (├╯ then │╭─)
    assert!(
        output.contains("├╯\n│\n│╭─ [feature-a]"),
        "expected independent branches, got:\n{}",
        output
    );
}

#[test]
fn stacked_branches() {
    let mut info = base_info();
    // feature-b: B2 -> B1 -> A2 -> A1 -> upstream
    // feature-a tip = A2
    // feature-b tip = B2
    info.commits = vec![
        commit(4, "B2", Some(3)),
        commit(3, "B1", Some(2)), // B1's parent is A2 (the tip of feature-a)
        commit(2, "A2", Some(1)),
        commit(1, "A1", None),
    ];
    info.branches = vec![
        BranchInfo {
            name: "feature-b".to_string(),
            tip_oid: oid(4),
        },
        BranchInfo {
            name: "feature-a".to_string(),
            tip_oid: oid(2),
        },
    ];

    let output = graph::render(info);
    // B1's parent IS A2 (oid(2)), so they should be stacked (││ then │├─)
    assert!(
        output.contains("││\n│├─ [feature-a]"),
        "expected stacked branches, got:\n{}",
        output
    );
    // Only one ├╯ at the very end of the stack
    assert_eq!(
        output.matches("├╯").count(),
        1,
        "expected single ├╯, got:\n{}",
        output
    );
}

#[test]
fn loose_commits_on_integration_line() {
    let mut info = base_info();
    // Two commits not belonging to any branch
    info.commits = vec![commit(2, "Fix typo", Some(1)), commit(1, "Refactor", None)];

    let output = graph::render(info);
    assert!(
        output.contains("│●   0000002 Fix typo\n│●   0000001 Refactor"),
        "expected loose commits, got:\n{}",
        output
    );
    // No branch markers (only the working changes header, no │╭─)
    assert!(
        !output.contains("│╭─"),
        "unexpected branch header in:\n{}",
        output
    );
}

#[test]
fn mixed_loose_and_branch() {
    let mut info = base_info();
    // Loose commit on top, then a branch
    // Topological order: loose_commit, branch_tip, branch_commit
    info.commits = vec![
        commit(3, "Loose on top", Some(2)),
        commit(2, "B tip", Some(1)),
        commit(1, "B base", None),
    ];
    info.branches = vec![BranchInfo {
        name: "feature-b".to_string(),
        tip_oid: oid(2),
    }];

    let output = graph::render(info);
    // Loose commit should appear before the branch
    assert!(
        output.contains("│●   0000003 Loose on top\n│\n│╭─ [feature-b]"),
        "expected loose then branch, got:\n{}",
        output
    );
}
