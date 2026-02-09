use git2::Oid;

use crate::git::{BranchInfo, CommitInfo, FileChange, RepoInfo, UpstreamInfo};
use crate::graph;

/// Strip ANSI escape codes so tests can compare plain text.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until 'm' (end of ANSI escape sequence)
            for inner in chars.by_ref() {
                if inner == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Render and strip ANSI codes for plain-text comparison.
fn render_plain(info: RepoInfo) -> String {
    strip_ansi(&graph::render(info))
}

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
        upstream: UpstreamInfo {
            label: "origin/main".to_string(),
            base_short_id: "aaa0000".to_string(),
            base_message: "Initial commit".to_string(),
            base_date: "2025-07-06".to_string(),
            commits_ahead: 0,
        },
        commits: vec![],
        branches: vec![],
        working_changes: vec![],
    }
}

#[test]
fn no_commits_no_changes() {
    let output = render_plain(base_info());
    assert_eq!(
        output,
        "\
╭─ zz [unstaged changes]
│   no changes
│
● aaa0000 (upstream) [origin/main] Initial commit
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

    let output = render_plain(info);
    assert!(output.starts_with("╭─ zz [unstaged changes]\n│   ma M src/main.rs\n│   nf A new_file.txt\n"));
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

    let output = render_plain(info);
    assert_eq!(
        output,
        "\
╭─ zz [unstaged changes]
│   no changes
│
│╭─ fa [feature-a]
│●   0200002 A2
│●   0100001 A1
├╯
│
● aaa0000 (upstream) [origin/main] Initial commit
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

    let output = render_plain(info);
    // Both branches start with "feature-" so IDs will be extended
    // B1's parent is NOT A1, so they should be independent (├╯ then │╭─)
    assert!(
        output.contains("├╯\n│\n│╭─"),
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

    let output = render_plain(info);
    // B1's parent IS A2 (oid(2)), so they should be stacked (││ then │├─)
    assert!(
        output.contains("││\n│├─"),
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

    let output = render_plain(info);
    assert!(
        output.contains("●   0200002 Fix typo\n●   0100001 Refactor"),
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

    let output = render_plain(info);
    // Loose commit should appear before the branch
    assert!(
        output.contains("●   0300003 Loose on top\n│\n│╭─ fb [feature-b]"),
        "expected loose then branch, got:\n{}",
        output
    );
}

#[test]
fn upstream_ahead_shows_indicator() {
    let mut info = base_info();
    info.upstream.commits_ahead = 3;

    let output = render_plain(info);
    assert!(
        output.contains("│●  [origin/main] ⏫ 3 new commits\n├╯ aaa0000 (common base) 2025-07-06 Initial commit\n"),
        "expected upstream-ahead indicator, got:\n{}",
        output
    );
}

#[test]
fn upstream_ahead_singular() {
    let mut info = base_info();
    info.upstream.commits_ahead = 1;

    let output = render_plain(info);
    assert!(
        output.contains("⏫ 1 new commit\n"),
        "expected singular 'commit', got:\n{}",
        output
    );
    assert!(
        !output.contains("commits\n"),
        "should not contain plural 'commits', got:\n{}",
        output
    );
}

#[test]
fn short_ids_for_files_use_filename() {
    let mut info = base_info();
    info.working_changes = vec![
        FileChange {
            path: "src/graph.rs".to_string(),
            status: 'M',
        },
        FileChange {
            path: "src/git.rs".to_string(),
            status: 'M',
        },
    ];

    let output = render_plain(info);
    // "graph.rs" -> "gr", "git.rs" -> "it" (skip 'g' since already used)
    assert!(
        output.contains("gr M src/graph.rs"),
        "expected file short ID 'gr', got:\n{}",
        output
    );
    assert!(
        output.contains("it M src/git.rs"),
        "expected file short ID 'it', got:\n{}",
        output
    );
}

#[test]
fn short_ids_collision_extends() {
    let mut info = base_info();
    info.working_changes = vec![
        FileChange {
            path: "src/main.rs".to_string(),
            status: 'M',
        },
        FileChange {
            path: "src/manifest.rs".to_string(),
            status: 'A',
        },
    ];

    let output = render_plain(info);
    // Both filenames start with "ma", so IDs should extend to 3+ chars
    let lines: Vec<&str> = output.lines().collect();
    let main_line = lines.iter().find(|l| l.contains("main.rs")).unwrap();
    let manifest_line = lines.iter().find(|l| l.contains("manifest.rs")).unwrap();
    // They should have different IDs
    assert_ne!(main_line, manifest_line);
}
