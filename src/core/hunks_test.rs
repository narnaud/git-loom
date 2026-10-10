use super::*;

fn make_hunk(text: &str) -> DiffHunk {
    DiffHunk {
        text: text.to_string(),
        modified_lines: vec![],
    }
}

#[test]
fn effective_status_staged_only_deselect_some() {
    // M  → deselect one of two staged hunks → MM
    let file = FileEntry {
        path: "f.rs".into(),
        hunks: vec![
            HunkEntry {
                hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                selected: true,
                origin: HunkOrigin::Staged,
            },
            HunkEntry {
                hunk: make_hunk("@@ -10,1 +10,1 @@\n-c\n+d\n"),
                selected: false, // deselected
                origin: HunkOrigin::Staged,
            },
        ],
        index_status: 'M',
        worktree_status: ' ',
        binary: false,
    };
    assert_eq!(file.effective_status(), ('M', 'M'));
}

#[test]
fn effective_status_staged_only_deselect_all() {
    // M  → deselect all → _M
    let file = FileEntry {
        path: "f.rs".into(),
        hunks: vec![HunkEntry {
            hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
            selected: false,
            origin: HunkOrigin::Staged,
        }],
        index_status: 'M',
        worktree_status: ' ',
        binary: false,
    };
    assert_eq!(file.effective_status(), (' ', 'M'));
}

#[test]
fn effective_status_unstaged_only_select_all() {
    // _M → select all → M_
    let file = FileEntry {
        path: "f.rs".into(),
        hunks: vec![HunkEntry {
            hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
            selected: true,
            origin: HunkOrigin::Unstaged,
        }],
        index_status: ' ',
        worktree_status: 'M',
        binary: false,
    };
    assert_eq!(file.effective_status(), ('M', ' '));
}

#[test]
fn effective_status_untracked_select() {
    // ?? → select → A_
    let file = FileEntry {
        path: "new.rs".into(),
        hunks: vec![HunkEntry {
            hunk: make_hunk("@@ -0,0 +1,1 @@\n+new\n"),
            selected: true,
            origin: HunkOrigin::Unstaged,
        }],
        index_status: '?',
        worktree_status: '?',
        binary: false,
    };
    assert_eq!(file.effective_status(), ('A', ' '));
}

#[test]
fn effective_status_untracked_no_select() {
    // ?? stays ??
    let file = FileEntry {
        path: "new.rs".into(),
        hunks: vec![HunkEntry {
            hunk: make_hunk("@@ -0,0 +1,1 @@\n+new\n"),
            selected: false,
            origin: HunkOrigin::Unstaged,
        }],
        index_status: '?',
        worktree_status: '?',
        binary: false,
    };
    assert_eq!(file.effective_status(), ('?', '?'));
}

#[test]
fn effective_status_new_file_deselect() {
    // A_ → deselect → ??
    let file = FileEntry {
        path: "new.rs".into(),
        hunks: vec![HunkEntry {
            hunk: make_hunk("@@ -0,0 +1,1 @@\n+new\n"),
            selected: false,
            origin: HunkOrigin::Staged,
        }],
        index_status: 'A',
        worktree_status: ' ',
        binary: false,
    };
    assert_eq!(file.effective_status(), ('?', '?'));
}

#[test]
fn effective_status_deletion_deselect() {
    // D_ → deselect → _D
    let file = FileEntry {
        path: "old.rs".into(),
        hunks: vec![HunkEntry {
            hunk: make_hunk("(file deleted)"),
            selected: false,
            origin: HunkOrigin::Staged,
        }],
        index_status: 'D',
        worktree_status: ' ',
        binary: false,
    };
    assert_eq!(file.effective_status(), (' ', 'D'));
}
