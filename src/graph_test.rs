use git2::Oid;

use crate::git::{BranchInfo, CommitInfo, ContextCommit, FileChange, RepoInfo, UpstreamInfo};
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
        files: vec![],
    }
}

fn commit_with_files(
    byte: u8,
    message: &str,
    parent: Option<u8>,
    files: Vec<FileChange>,
) -> CommitInfo {
    CommitInfo {
        oid: oid(byte),
        short_id: format!("{:07x}", byte),
        message: message.to_string(),
        parent_oid: parent.map(oid),
        files,
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
            merge_base_oid: oid(0xAA),
        },
        commits: vec![],
        branches: vec![],
        working_changes: vec![],
        context_commits: vec![],
    }
}

#[test]
fn no_commits_no_changes() {
    let output = render_plain(base_info());
    assert_eq!(
        output,
        "\
╭─ zz [local changes]
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
            index: ' ',
            worktree: 'M',
        },
        FileChange {
            path: "new_file.txt".to_string(),
            index: 'A',
            worktree: ' ',
        },
    ];

    let output = render_plain(info);
    assert!(
        output
            .starts_with("╭─ zz [local changes]\n│   ma  M src/main.rs\n│   nf A  new_file.txt\n")
    );
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
╭─ zz [local changes]
│   no changes
│
│╭─ fa [feature-a]
│●    0200002 A2
│●    0100001 A1
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
        output.contains("●    0200002 Fix typo\n●    0100001 Refactor"),
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
        output.contains("●    0300003 Loose on top\n│\n│╭─ fb [feature-b]"),
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
fn merge_based_integration_branch() {
    // Simulates a merge-based integration branch where feature-1 is merged
    // via --no-ff. The revwalk produces commits from both sides of the merge,
    // but only the feature-1 ancestors should be assigned to the branch.
    //
    // History (merge commit already filtered by walk_commits):
    //   feature-1: F4 -> F3 -> F2 -> F1 -> upstream
    //   integration first-parent: I3 -> I2 -> I1 -> upstream
    //
    // Topo order after skipping the merge: F4, F3, F2, F1, I3, I2, I1
    let mut info = base_info();
    info.commits = vec![
        commit(0x14, "Feature 1 improvement", Some(0x13)),
        commit(0x13, "fixup Feature 1", Some(0x12)),
        commit(0x12, "Bugfix 1", Some(0x11)),
        commit(0x11, "Feature 1", Some(0xaa)), // parent is upstream base
        commit(0x23, "Feature 3 depends on Feature 2", Some(0x22)),
        commit(0x22, "fixup Feature 2", Some(0x21)),
        commit(0x21, "Feature 2", Some(0xaa)), // parent is upstream base
    ];
    info.branches = vec![BranchInfo {
        name: "feature-1".to_string(),
        tip_oid: oid(0x14),
    }];

    let output = render_plain(info);

    // feature-1 should have exactly 4 commits
    let branch_section_start = output.find("│╭─").expect("no branch header found");
    let branch_section_end = output[branch_section_start..]
        .find("├╯")
        .expect("no branch close found")
        + branch_section_start;
    let branch_section = &output[branch_section_start..branch_section_end];
    let branch_dots = branch_section.matches("│●").count();
    assert_eq!(
        branch_dots, 4,
        "feature-1 should have exactly 4 commits, got {} in:\n{}",
        branch_dots, output
    );

    // Integration-line commits should be loose (plain ● without │ prefix)
    assert!(
        output.contains("●    2300023 Feature 3 depends on Feature 2"),
        "expected loose integration commit, got:\n{}",
        output
    );
}

#[test]
fn co_located_branches_show_all_names() {
    let mut info = base_info();
    // Two branches pointing to the same tip commit
    info.commits = vec![commit(2, "A2", Some(1)), commit(1, "A1", None)];
    info.branches = vec![
        BranchInfo {
            name: "feature-a".to_string(),
            tip_oid: oid(2),
        },
        BranchInfo {
            name: "feature-a-v2".to_string(),
            tip_oid: oid(2),
        },
    ];

    let output = render_plain(info);
    // Both branch names should appear as headers
    assert!(
        output.contains("[feature-a]"),
        "expected [feature-a] header, got:\n{}",
        output
    );
    assert!(
        output.contains("[feature-a-v2]"),
        "expected [feature-a-v2] header, got:\n{}",
        output
    );
    // Newest (alphabetically last) on top with ╭─, oldest uses ├─
    let v2_pos = output.find("[feature-a-v2]").unwrap();
    let a_pos = output.find("[feature-a]").unwrap();
    assert!(
        v2_pos < a_pos,
        "expected feature-a-v2 on top of feature-a, got:\n{}",
        output
    );
    assert!(
        output.contains("│╭─") && output.contains("│├─"),
        "expected ╭─ then ├─ for co-located branches, got:\n{}",
        output
    );
    // Only one set of commits (not duplicated)
    assert_eq!(
        output.matches("│●").count(),
        2,
        "expected 2 commit dots (not duplicated), got:\n{}",
        output
    );
}

#[test]
fn branch_at_upstream_shown_as_section() {
    let mut info = base_info();
    // A branch whose tip is the merge-base (upstream) commit
    info.branches = vec![BranchInfo {
        name: "feature-4".to_string(),
        tip_oid: oid(0xaa), // same as upstream base_oid
    }];

    let output = render_plain(info);
    // Should appear as a branch section header, not on the upstream line
    assert!(
        output.contains("│╭─") && output.contains("[feature-4]"),
        "expected branch section for feature-4, got:\n{}",
        output
    );
    assert!(
        output.contains("├╯"),
        "expected branch close, got:\n{}",
        output
    );
}

#[test]
fn short_ids_for_files_use_filename() {
    let mut info = base_info();
    info.working_changes = vec![
        FileChange {
            path: "src/graph.rs".to_string(),
            index: ' ',
            worktree: 'M',
        },
        FileChange {
            path: "src/git.rs".to_string(),
            index: ' ',
            worktree: 'M',
        },
    ];

    let output = render_plain(info);
    // "graph.rs" -> "gr", "git.rs" -> "it" (skip 'g' since already used)
    assert!(
        output.contains("gr  M src/graph.rs"),
        "expected file short ID 'gr', got:\n{}",
        output
    );
    assert!(
        output.contains("it  M src/git.rs"),
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
            index: ' ',
            worktree: 'M',
        },
        FileChange {
            path: "src/manifest.rs".to_string(),
            index: 'A',
            worktree: ' ',
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

#[test]
fn files_shown_under_branch_commits() {
    let mut info = base_info();
    info.commits = vec![
        commit_with_files(
            2,
            "A2",
            Some(1),
            vec![
                FileChange {
                    path: "src/graph.rs".to_string(),
                    index: 'M',
                    worktree: ' ',
                },
                FileChange {
                    path: "new_file.txt".to_string(),
                    index: 'A',
                    worktree: ' ',
                },
            ],
        ),
        commit_with_files(
            1,
            "A1",
            None,
            vec![FileChange {
                path: "src/status.rs".to_string(),
                index: 'M',
                worktree: ' ',
            }],
        ),
    ];
    info.branches = vec![BranchInfo {
        name: "feature-a".to_string(),
        tip_oid: oid(2),
    }];

    let output = render_plain(info);
    // File shortids use commit_sid:index format
    assert!(
        output.contains(
            "│●    0200002 A2\n│┊      02:0 M  src/graph.rs\n│┊      02:1 A  new_file.txt\n"
        ),
        "expected files under A2, got:\n{}",
        output
    );
    assert!(
        output.contains("│●    0100001 A1\n│┊      01:0 M  src/status.rs\n"),
        "expected files under A1, got:\n{}",
        output
    );
}

#[test]
fn files_shown_under_loose_commits() {
    let mut info = base_info();
    info.commits = vec![commit_with_files(
        2,
        "Fix typo",
        Some(1),
        vec![FileChange {
            path: "README.md".to_string(),
            index: 'M',
            worktree: ' ',
        }],
    )];

    let output = render_plain(info);
    // Loose commit file should have ┊ prefix with commit_sid:index format
    assert!(
        output.contains("●    0200002 Fix typo\n┊       02:0 M  README.md\n"),
        "expected files under loose commit, got:\n{}",
        output
    );
}

#[test]
fn commit_file_ids_use_commit_sid_colon_index() {
    let mut info = base_info();
    info.commits = vec![commit_with_files(
        2,
        "A1",
        None,
        vec![
            FileChange {
                path: "foo.rs".to_string(),
                index: 'A',
                worktree: ' ',
            },
            FileChange {
                path: "bar.rs".to_string(),
                index: 'M',
                worktree: ' ',
            },
        ],
    )];
    info.branches = vec![BranchInfo {
        name: "feature-a".to_string(),
        tip_oid: oid(2),
    }];

    let output = render_plain(info);
    assert!(
        output.contains("02:0 A  foo.rs"),
        "expected 02:0 for first file, got:\n{}",
        output
    );
    assert!(
        output.contains("02:1 M  bar.rs"),
        "expected 02:1 for second file, got:\n{}",
        output
    );
}

#[test]
fn root_commit_files_shown() {
    // A commit with no parent (root commit) should still show files
    let mut info = base_info();
    info.commits = vec![commit_with_files(
        1,
        "Initial",
        None,
        vec![FileChange {
            path: "init.rs".to_string(),
            index: 'A',
            worktree: ' ',
        }],
    )];

    let output = render_plain(info);
    assert!(
        output.contains("●    0100001 Initial\n┊       01:0 A  init.rs\n"),
        "expected file under root commit, got:\n{}",
        output
    );
}

#[test]
fn same_file_in_multiple_commits_gets_unique_ids() {
    let mut info = base_info();
    // Same file "src/main.rs" modified in both commits
    info.commits = vec![
        commit_with_files(
            2,
            "A2",
            Some(1),
            vec![FileChange {
                path: "src/main.rs".to_string(),
                index: 'M',
                worktree: ' ',
            }],
        ),
        commit_with_files(
            1,
            "A1",
            None,
            vec![FileChange {
                path: "src/main.rs".to_string(),
                index: 'M',
                worktree: ' ',
            }],
        ),
    ];
    info.branches = vec![BranchInfo {
        name: "feature-a".to_string(),
        tip_oid: oid(2),
    }];

    let output = render_plain(info);
    // Same file in different commits should get different IDs (02:0 vs 01:0)
    assert!(
        output.contains("02:0 M  src/main.rs"),
        "expected 02:0 for commit 2's file, got:\n{}",
        output
    );
    assert!(
        output.contains("01:0 M  src/main.rs"),
        "expected 01:0 for commit 1's file, got:\n{}",
        output
    );
}

#[test]
fn context_commits_shown_below_upstream() {
    let mut info = base_info();
    info.context_commits = vec![
        ContextCommit {
            short_hash: "bbb0001".to_string(),
            message: "Earlier commit".to_string(),
            date: "2025-07-05".to_string(),
        },
        ContextCommit {
            short_hash: "bbb0002".to_string(),
            message: "Even earlier".to_string(),
            date: "2025-07-04".to_string(),
        },
    ];

    let output = render_plain(info);
    // Context commits should appear after the upstream line
    assert!(
        output.contains("· bbb0001 2025-07-05 Earlier commit\n· bbb0002 2025-07-04 Even earlier\n"),
        "expected context commits below upstream, got:\n{}",
        output
    );
    // Upstream line should come before context
    let upstream_pos = output.find("(upstream)").unwrap();
    let context_pos = output.find("· bbb0001").unwrap();
    assert!(
        upstream_pos < context_pos,
        "upstream should appear before context commits, got:\n{}",
        output
    );
}

#[test]
fn context_commits_shown_below_diverged_upstream() {
    let mut info = base_info();
    info.upstream.commits_ahead = 2;
    info.context_commits = vec![ContextCommit {
        short_hash: "ccc0001".to_string(),
        message: "Before base".to_string(),
        date: "2025-07-01".to_string(),
    }];

    let output = render_plain(info);
    // Context should appear after the common base line
    let base_pos = output.find("(common base)").unwrap();
    let context_pos = output.find("· ccc0001").unwrap();
    assert!(
        base_pos < context_pos,
        "context should appear below common base, got:\n{}",
        output
    );
}

#[test]
fn no_context_when_default() {
    let info = base_info();
    let output = render_plain(info);
    assert!(
        !output.contains('·'),
        "no middle-dot lines expected with empty context_commits, got:\n{}",
        output
    );
}
