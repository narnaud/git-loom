use git2::Oid;

use crate::git::{
    BranchInfo, CommitInfo, ContextCommit, FileChange, RemoteStatus, RepoInfo, UpstreamInfo,
};
use crate::graph::{self, RenderOpts, Theme};

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

fn default_opts() -> RenderOpts {
    RenderOpts {
        terminal_width: None,
        theme: Theme::dark(),
        cwd_prefix: String::new(),
    }
}

/// Render and strip ANSI codes for plain-text comparison.
fn render_plain(info: RepoInfo) -> String {
    let ids = crate::shortid::IdAllocator::new(info.collect_entities());
    strip_ansi(&graph::render(info, &ids, &default_opts()))
}

/// Render with a specific terminal width for multi-column tests.
fn render_plain_with_width(info: RepoInfo, width: u16) -> String {
    let opts = RenderOpts {
        terminal_width: Some(width),
        theme: Theme::dark(),
        cwd_prefix: String::new(),
    };
    let ids = crate::shortid::IdAllocator::new(info.collect_entities());
    strip_ansi(&graph::render(info, &ids, &opts))
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
        branch_name: "main".to_string(),
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
        remote: None,
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
            remote: None,
        },
        BranchInfo {
            name: "feature-a".to_string(),
            tip_oid: oid(1),
            remote: None,
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
            remote: None,
        },
        BranchInfo {
            name: "feature-a".to_string(),
            tip_oid: oid(2),
            remote: None,
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
        remote: None,
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
        remote: None,
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
            remote: None,
        },
        BranchInfo {
            name: "feature-a-v2".to_string(),
            tip_oid: oid(2),
            remote: None,
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
        remote: None,
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
    // "graph.rs" -> "gr", "git.rs" -> "gi" (same first letter is fine, full IDs are unique)
    assert!(
        output.contains("gr  M src/graph.rs"),
        "expected file short ID 'gr', got:\n{}",
        output
    );
    assert!(
        output.contains("gi  M src/git.rs"),
        "expected file short ID 'gi', got:\n{}",
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
        remote: None,
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
        remote: None,
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
        remote: None,
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

#[test]
fn tracked_files_shown_before_untracked() {
    let mut info = base_info();
    // Mix tracked and untracked in the input order: untracked first, then tracked
    info.working_changes = vec![
        FileChange {
            path: ".claude/".to_string(),
            index: '?',
            worktree: '?',
        },
        FileChange {
            path: "src/main.rs".to_string(),
            index: ' ',
            worktree: 'M',
        },
        FileChange {
            path: "todo.md".to_string(),
            index: '?',
            worktree: '?',
        },
        FileChange {
            path: "new_file.txt".to_string(),
            index: 'A',
            worktree: ' ',
        },
    ];

    let output = render_plain(info);
    // Tracked files should appear before untracked files, regardless of input order
    let main_pos = output.find("src/main.rs").unwrap();
    let new_file_pos = output.find("new_file.txt").unwrap();
    let claude_pos = output.find(".claude/").unwrap();
    let todo_pos = output.find("todo.md").unwrap();
    assert!(
        main_pos < claude_pos && main_pos < todo_pos,
        "tracked files should appear before untracked, got:\n{}",
        output
    );
    assert!(
        new_file_pos < claude_pos && new_file_pos < todo_pos,
        "tracked files should appear before untracked, got:\n{}",
        output
    );
}

#[test]
fn only_untracked_skips_no_changes() {
    let mut info = base_info();
    info.working_changes = vec![
        FileChange {
            path: ".claude/".to_string(),
            index: '?',
            worktree: '?',
        },
        FileChange {
            path: "todo.md".to_string(),
            index: '?',
            worktree: '?',
        },
    ];

    let output = render_plain(info);
    assert!(
        !output.contains("no changes"),
        "should not show 'no changes' when untracked files exist, got:\n{}",
        output
    );
    assert!(
        output.contains(" ⁕ .claude/"),
        "expected untracked file, got:\n{}",
        output
    );
    assert!(
        output.contains(" ⁕ todo.md"),
        "expected untracked file, got:\n{}",
        output
    );
}

#[test]
fn untracked_below_threshold_stays_single_column() {
    let mut info = base_info();
    // 5 untracked files (at threshold, not above) — should stay single-column
    info.working_changes = (1..=5)
        .map(|i| FileChange {
            path: format!("file{}.txt", i),
            index: '?',
            worktree: '?',
        })
        .collect();

    let output = render_plain_with_width(info, 120);
    // Each file should be on its own line
    for i in 1..=5 {
        let pattern = format!(" ⁕ file{}.txt", i);
        assert!(
            output.contains(&pattern),
            "expected single-column entry for file{}.txt, got:\n{}",
            i,
            output
        );
    }
    // Count lines with "⁕" to verify single-column
    let untracked_lines: Vec<&str> = output.lines().filter(|l| l.contains('⁕')).collect();
    assert_eq!(
        untracked_lines.len(),
        5,
        "expected 5 single-column lines, got {} in:\n{}",
        untracked_lines.len(),
        output
    );
}

#[test]
fn untracked_above_threshold_uses_multicolumn() {
    let mut info = base_info();
    // 6 untracked files (above threshold) with short paths — should go multi-column at 80 cols
    info.working_changes = (1..=6)
        .map(|i| FileChange {
            path: format!("f{}.txt", i),
            index: '?',
            worktree: '?',
        })
        .collect();

    let output = render_plain_with_width(info, 80);
    // All files should still appear
    for i in 1..=6 {
        let pattern = format!("f{}.txt", i);
        assert!(
            output.contains(&pattern),
            "expected f{}.txt in output, got:\n{}",
            i,
            output
        );
    }
    // With short paths, multiple entries should fit on one line — fewer lines than entries
    let untracked_lines: Vec<&str> = output.lines().filter(|l| l.contains('⁕')).collect();
    assert!(
        untracked_lines.len() < 6,
        "expected multi-column (fewer than 6 lines), got {} lines:\n{:?}",
        untracked_lines.len(),
        untracked_lines
    );
}

#[test]
fn multicolumn_no_tty_stays_single_column() {
    let mut info = base_info();
    // 10 untracked files, but terminal_width is None (piped output)
    info.working_changes = (1..=10)
        .map(|i| FileChange {
            path: format!("file{}.txt", i),
            index: '?',
            worktree: '?',
        })
        .collect();

    // render_plain uses default_opts() which has terminal_width: None
    let output = render_plain(info);
    let untracked_lines: Vec<&str> = output.lines().filter(|l| l.contains('⁕')).collect();
    assert_eq!(
        untracked_lines.len(),
        10,
        "expected single-column (10 lines) when no TTY, got {} lines:\n{:?}",
        untracked_lines.len(),
        untracked_lines
    );
}

#[test]
fn multicolumn_fills_top_to_bottom_left_to_right() {
    let mut info = base_info();
    // 6 files with short names.
    // Entry "xx  ⁕ aa.txt" = 2+1+2+1+6 = 12 chars; col_slot = 12+5 = 17 (entry + "   │ ").
    // At width 55: available = 55-4 = 51; 51/17 = 3 cols; rows = ceil(6/3) = 2.
    // Column-major layout:
    //   Row 0: aa.txt (idx 0), cc.txt (idx 2), ee.txt (idx 4)
    //   Row 1: bb.txt (idx 1), dd.txt (idx 3), ff.txt (idx 5)
    info.working_changes = vec!["aa", "bb", "cc", "dd", "ee", "ff"]
        .into_iter()
        .map(|name| FileChange {
            path: format!("{}.txt", name),
            index: '?',
            worktree: '?',
        })
        .collect();

    let output = render_plain_with_width(info, 55);
    let untracked_lines: Vec<&str> = output.lines().filter(|l| l.contains('⁕')).collect();

    assert_eq!(
        untracked_lines.len(),
        2,
        "expected 2 rows with 3 columns, got {} lines:\n{:?}",
        untracked_lines.len(),
        untracked_lines
    );

    // Row 0 should contain aa, cc, ee (column-major: indices 0, 2, 4)
    assert!(
        untracked_lines[0].contains("aa.txt")
            && untracked_lines[0].contains("cc.txt")
            && untracked_lines[0].contains("ee.txt"),
        "row 0 should be aa, cc, ee (column-major), got: {}",
        untracked_lines[0]
    );
    // Row 1 should contain bb, dd, ff (column-major: indices 1, 3, 5)
    assert!(
        untracked_lines[1].contains("bb.txt")
            && untracked_lines[1].contains("dd.txt")
            && untracked_lines[1].contains("ff.txt"),
        "row 1 should be bb, dd, ff (column-major), got: {}",
        untracked_lines[1]
    );
}

#[test]
fn multicolumn_has_pipe_separators() {
    let mut info = base_info();
    info.working_changes = (1..=6)
        .map(|i| FileChange {
            path: format!("f{}.txt", i),
            index: '?',
            worktree: '?',
        })
        .collect();

    let output = render_plain_with_width(info, 80);
    let untracked_lines: Vec<&str> = output.lines().filter(|l| l.contains('⁕')).collect();
    // Multi-column lines should contain pipe separators between entries
    assert!(
        untracked_lines[0].contains("   │ "),
        "expected pipe separator between columns, got: {}",
        untracked_lines[0]
    );
}

#[test]
fn remote_synced_shows_checkmark() {
    let mut info = base_info();
    info.commits = vec![commit(2, "A2", Some(1)), commit(1, "A1", None)];
    info.branches = vec![BranchInfo {
        name: "feature-a".to_string(),
        tip_oid: oid(2),
        remote: Some(RemoteStatus::Synced),
    }];

    let output = render_plain(info);
    assert!(
        output.contains("[feature-a] ✓"),
        "expected synced indicator, got:\n{}",
        output
    );
}

#[test]
fn remote_ahead_shows_up_arrow() {
    let mut info = base_info();
    info.commits = vec![commit(2, "A2", Some(1)), commit(1, "A1", None)];
    info.branches = vec![BranchInfo {
        name: "feature-a".to_string(),
        tip_oid: oid(2),
        remote: Some(RemoteStatus::Ahead),
    }];

    let output = render_plain(info);
    assert!(
        output.contains("[feature-a] ↑"),
        "expected ahead indicator, got:\n{}",
        output
    );
}

#[test]
fn remote_gone_shows_cross() {
    let mut info = base_info();
    info.commits = vec![commit(2, "A2", Some(1)), commit(1, "A1", None)];
    info.branches = vec![BranchInfo {
        name: "feature-a".to_string(),
        tip_oid: oid(2),
        remote: Some(RemoteStatus::Gone),
    }];

    let output = render_plain(info);
    assert!(
        output.contains("[feature-a] ✗"),
        "expected gone indicator, got:\n{}",
        output
    );
}

#[test]
fn no_remote_shows_no_indicator() {
    let mut info = base_info();
    info.commits = vec![commit(2, "A2", Some(1)), commit(1, "A1", None)];
    info.branches = vec![BranchInfo {
        name: "feature-a".to_string(),
        tip_oid: oid(2),
        remote: None,
    }];

    let output = render_plain(info);
    // Branch header line should end with ] and nothing after it
    let header_line = output.lines().find(|l| l.contains("[feature-a]")).unwrap();
    assert!(
        header_line.ends_with("[feature-a]"),
        "expected no indicator after ], got: {}",
        header_line
    );
}
