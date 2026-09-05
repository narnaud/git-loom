use crossterm::event::{Event, KeyEvent, MouseEvent};

use super::*;
use crate::core::diff::DiffHunk;
use crate::core::graph::Theme;
use crate::tui::theme::TuiTheme;

fn make_hunk(text: &str) -> DiffHunk {
    DiffHunk {
        text: text.to_string(),
        modified_lines: vec![],
    }
}

fn make_theme() -> TuiTheme {
    TuiTheme::from_graph_theme(&Theme::dark())
}

/// Root-level files (no directory grouping) — keeps existing tests simple.
fn make_files() -> Vec<FileEntry> {
    vec![
        FileEntry {
            path: "main.rs".to_string(),
            hunks: vec![
                HunkEntry {
                    hunk: make_hunk("@@ -1,3 +1,4 @@\n context\n-old\n+new\n"),
                    selected: true,
                    origin: HunkOrigin::Staged,
                },
                HunkEntry {
                    hunk: make_hunk("@@ -10,2 +11,3 @@\n context\n+added\n"),
                    selected: false,
                    origin: HunkOrigin::Unstaged,
                },
            ],
            index_status: 'M',
            worktree_status: 'M',
            binary: false,
        },
        FileEntry {
            path: "lib.rs".to_string(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -5,2 +5,2 @@\n-old line\n+new line\n"),
                selected: false,
                origin: HunkOrigin::Unstaged,
            }],
            index_status: ' ',
            worktree_status: 'M',
            binary: false,
        },
    ]
}

/// Files in a subdirectory — for tree-specific tests.
fn make_files_in_dir() -> Vec<FileEntry> {
    vec![
        FileEntry {
            path: "src/main.rs".to_string(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                selected: true,
                origin: HunkOrigin::Staged,
            }],
            index_status: 'M',
            worktree_status: ' ',
            binary: false,
        },
        FileEntry {
            path: "src/lib.rs".to_string(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -1,1 +1,1 @@\n-x\n+y\n"),
                selected: false,
                origin: HunkOrigin::Unstaged,
            }],
            index_status: ' ',
            worktree_status: 'M',
            binary: false,
        },
    ]
}

/// Mix of root-level files and files in directories.
fn make_files_mixed() -> Vec<FileEntry> {
    vec![
        FileEntry {
            path: "README.md".to_string(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                selected: false,
                origin: HunkOrigin::Unstaged,
            }],
            index_status: ' ',
            worktree_status: 'M',
            binary: false,
        },
        FileEntry {
            path: "src/main.rs".to_string(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                selected: true,
                origin: HunkOrigin::Staged,
            }],
            index_status: 'M',
            worktree_status: ' ',
            binary: false,
        },
        FileEntry {
            path: "src/lib.rs".to_string(),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -1,1 +1,1 @@\n-x\n+y\n"),
                selected: false,
                origin: HunkOrigin::Unstaged,
            }],
            index_status: ' ',
            worktree_status: 'M',
            binary: false,
        },
    ]
}

#[test]
fn new_initializes_correctly() {
    let files = make_files();
    let app = HunkSelectorApp::new(files, make_theme());
    assert_eq!(app.list.cursor(), 0);
    assert_eq!(app.hunks.index(), 0);
    assert_eq!(app.hunks.scroll(), 0);
}

#[test]
fn navigate_files_in_left_pane() {
    let mut app = HunkSelectorApp::new(make_files(), make_theme());
    assert_eq!(app.list.cursor(), 0);

    app.navigate_down(PaneId::Left);
    assert_eq!(app.list.cursor(), 1);

    // Can't go past the last file.
    app.navigate_down(PaneId::Left);
    assert_eq!(app.list.cursor(), 1);

    app.navigate_up(PaneId::Left);
    assert_eq!(app.list.cursor(), 0);

    // Can't go before 0.
    app.navigate_up(PaneId::Left);
    assert_eq!(app.list.cursor(), 0);
}

#[test]
fn navigate_hunks_in_right_pane() {
    let mut app = HunkSelectorApp::new(make_files(), make_theme());

    // File 0 has 2 hunks.
    assert_eq!(app.list.cursor(), 0);
    assert_eq!(app.hunks.index(), 0);
    app.navigate_down(PaneId::Right);
    assert_eq!(app.hunks.index(), 1);

    // Past last hunk → move to next file, first hunk.
    app.navigate_down(PaneId::Right);
    assert_eq!(app.list.cursor(), 1);
    assert_eq!(app.hunks.index(), 0);

    // File 1 has 1 hunk — can't go further.
    app.navigate_down(PaneId::Right);
    assert_eq!(app.list.cursor(), 1);
    assert_eq!(app.hunks.index(), 0);

    // Up from first hunk of file 1 → last hunk of file 0.
    app.navigate_up(PaneId::Right);
    assert_eq!(app.list.cursor(), 0);
    assert_eq!(app.hunks.index(), 1);

    app.navigate_up(PaneId::Right);
    assert_eq!(app.hunks.index(), 0);

    // Can't go before first hunk of first file.
    app.navigate_up(PaneId::Right);
    assert_eq!(app.list.cursor(), 0);
    assert_eq!(app.hunks.index(), 0);
}

#[test]
fn navigate_hunks_cross_file_with_dir_headers() {
    let mut app = HunkSelectorApp::new(make_files_in_dir(), make_theme());
    // display_rows: [Dir(0..1), File(0), File(1)]
    // Start on dir header — right pane nav is no-op.
    assert_eq!(app.list.cursor(), 0);
    assert!(app.current_file_index().is_none());

    // Move cursor to file 0 first.
    app.navigate_down(PaneId::Left);
    assert_eq!(app.list.cursor(), 1);

    // File 0 has 1 hunk. Down → should skip dir headers and land on file 1.
    app.navigate_down(PaneId::Right);
    assert_eq!(app.list.cursor(), 2);
    assert_eq!(app.current_file_index(), Some(1));
    assert_eq!(app.hunks.index(), 0);

    // Up from file 1 → back to file 0's last hunk.
    app.navigate_up(PaneId::Right);
    assert_eq!(app.list.cursor(), 1);
    assert_eq!(app.current_file_index(), Some(0));
    assert_eq!(app.hunks.index(), 0); // file 0 has only 1 hunk
}

#[test]
fn toggle_hunk_in_right_pane() {
    let mut app = HunkSelectorApp::new(make_files(), make_theme());

    assert!(app.files[0].hunks[0].selected);
    app.toggle(PaneId::Right);
    assert!(!app.files[0].hunks[0].selected);
    app.toggle(PaneId::Right);
    assert!(app.files[0].hunks[0].selected);
}

#[test]
fn toggle_file_in_left_pane() {
    let mut app = HunkSelectorApp::new(make_files(), make_theme());
    // First file: one staged (selected), one unstaged (not selected) → any_selected=true
    assert!(app.files[0].hunks[0].selected);
    assert!(!app.files[0].hunks[1].selected);

    app.toggle(PaneId::Left); // deselect all (since any are selected).
    assert!(app.files[0].hunks.iter().all(|h| !h.selected));

    app.toggle(PaneId::Left); // Now none selected → select all.
    assert!(app.files[0].hunks.iter().all(|h| h.selected));
}

#[test]
fn quit_key_cancels_via_shell() {
    let mut shell = Shell::new(HunkSelectorApp::new(make_files(), make_theme()));
    let exit = shell.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
    )));
    assert!(matches!(exit, Some(Verdict::Cancel)));
}

#[test]
fn confirm_key_exits_with_confirm() {
    let mut app = HunkSelectorApp::new(make_files(), make_theme());
    let result = app.handle_key(PaneId::Left, KeyCode::Char('c'), KeyModifiers::NONE);
    assert!(matches!(result, KeyResult::Exit(Verdict::Confirm)));
}

#[test]
fn enter_confirms() {
    let mut app = HunkSelectorApp::new(make_files(), make_theme());
    let result = app.handle_key(PaneId::Left, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(result, KeyResult::Exit(Verdict::Confirm)));
}

#[test]
fn switching_file_resets_hunk_and_scroll() {
    let mut app = HunkSelectorApp::new(make_files(), make_theme());
    app.navigate_down(PaneId::Right); // Move to hunk 1
    assert_eq!(app.hunks.index(), 1);
    assert!(app.hunks.scroll() > 0);

    // Navigate to the next file in the left pane.
    app.navigate_down(PaneId::Left);
    assert_eq!(app.list.cursor(), 1);
    assert_eq!(app.hunks.index(), 0);
    assert_eq!(app.hunks.scroll(), 0);
}

#[test]
fn empty_files_returns_none() {
    let result = run_hunk_selector(vec![], make_theme()).unwrap();
    assert!(result.is_none());
}

#[test]
fn ctrl_c_cancels_via_shell() {
    let mut shell = Shell::new(HunkSelectorApp::new(make_files(), make_theme()));
    let exit = shell.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert!(matches!(exit, Some(Verdict::Cancel)));
}

#[test]
fn esc_cancels() {
    let mut app = HunkSelectorApp::new(make_files(), make_theme());
    let result = app.handle_key(PaneId::Left, KeyCode::Esc, KeyModifiers::NONE);
    assert!(matches!(result, KeyResult::Exit(Verdict::Cancel)));
}

#[test]
fn hunk_origin_preserved_through_toggle() {
    let mut app = HunkSelectorApp::new(make_files(), make_theme());

    // First hunk is Staged
    assert_eq!(app.files[0].hunks[0].origin, HunkOrigin::Staged);
    app.toggle(PaneId::Right);
    // Origin unchanged after toggle
    assert_eq!(app.files[0].hunks[0].origin, HunkOrigin::Staged);
    assert!(!app.files[0].hunks[0].selected);
}

// -- effective_status tests -----------------------------------------------

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

// -- tree display tests ---------------------------------------------------

#[test]
fn display_rows_root_files_no_headers() {
    let files = make_files();
    let rows = build_display_rows(&files);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], DisplayRow::File(0));
    assert_eq!(rows[1], DisplayRow::File(1));
}

#[test]
fn display_rows_dir_files_have_header() {
    let files = make_files_in_dir();
    let rows = build_display_rows(&files);
    // directory header + 2 files = 3 rows
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0],
        DisplayRow::Directory {
            dir_start: 0,
            dir_end: 1
        }
    );
    assert_eq!(rows[1], DisplayRow::File(0));
    assert_eq!(rows[2], DisplayRow::File(1));
}

#[test]
fn display_rows_mixed_root_and_dir() {
    let files = make_files_mixed();
    let rows = build_display_rows(&files);
    // README.md (root), then ▼ src header, then src/main.rs, src/lib.rs
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0], DisplayRow::File(0)); // README.md
    assert_eq!(
        rows[1],
        DisplayRow::Directory {
            dir_start: 1,
            dir_end: 2
        }
    );
    assert_eq!(rows[2], DisplayRow::File(1)); // src/main.rs
    assert_eq!(rows[3], DisplayRow::File(2)); // src/lib.rs
}

#[test]
fn navigate_through_dir_header() {
    let mut app = HunkSelectorApp::new(make_files_in_dir(), make_theme());
    // display_rows: [Dir(0..1), File(0), File(1)]
    assert_eq!(app.list.cursor(), 0);
    assert!(app.current_file_index().is_none()); // on dir header

    app.navigate_down(PaneId::Left);
    assert_eq!(app.list.cursor(), 1);
    assert_eq!(app.current_file_index(), Some(0)); // on first file

    app.navigate_down(PaneId::Left);
    assert_eq!(app.list.cursor(), 2);
    assert_eq!(app.current_file_index(), Some(1)); // on second file

    // Can't go past last row.
    app.navigate_down(PaneId::Left);
    assert_eq!(app.list.cursor(), 2);
}

#[test]
fn toggle_directory_toggles_all_files() {
    let mut app = HunkSelectorApp::new(make_files_in_dir(), make_theme());
    // cursor 0 = dir header
    // File 0: one hunk selected. File 1: one hunk not selected.
    assert!(app.files[0].hunks[0].selected);
    assert!(!app.files[1].hunks[0].selected);

    // Toggle dir: any_selected=true → deselect all.
    app.toggle(PaneId::Left);
    assert!(!app.files[0].hunks[0].selected);
    assert!(!app.files[1].hunks[0].selected);

    // Toggle again: none selected → select all.
    app.toggle(PaneId::Left);
    assert!(app.files[0].hunks[0].selected);
    assert!(app.files[1].hunks[0].selected);
}

#[test]
fn right_pane_noop_on_dir_header() {
    let mut app = HunkSelectorApp::new(make_files_in_dir(), make_theme());

    // On directory header — hunk navigation should be no-op.
    assert_eq!(app.hunks.index(), 0);
    app.navigate_down(PaneId::Right);
    assert_eq!(app.hunks.index(), 0);

    // Toggle on dir in right pane should be no-op.
    app.toggle(PaneId::Right);
    assert!(app.files[0].hunks[0].selected); // unchanged
}

/// Many root-level files, so the list can scroll.
fn make_many_files(n: usize) -> Vec<FileEntry> {
    (0..n)
        .map(|i| FileEntry {
            path: format!("file{:02}.rs", i),
            hunks: vec![HunkEntry {
                hunk: make_hunk("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                selected: false,
                origin: HunkOrigin::Unstaged,
            }],
            index_status: ' ',
            worktree_status: 'M',
            binary: false,
        })
        .collect()
}

/// Render through the real shell path so its pane areas are populated.
fn render_shell(shell: &mut Shell<HunkSelectorApp>, width: u16, height: u16) {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| shell.render(f)).unwrap();
}

fn click(shell: &mut Shell<HunkSelectorApp>, column: u16, row: u16) {
    shell.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }));
}

#[test]
fn mouse_click_accounts_for_list_scroll() {
    let mut shell = Shell::new(HunkSelectorApp::new(make_many_files(30), make_theme()));
    shell.app.list.set_cursor(29); // forces the list to scroll during render
    render_shell(&mut shell, 80, 12);
    let offset = shell.app.list.offset();
    assert!(offset > 0, "list should be scrolled");

    // Click the first visible row — must select the row at the scroll
    // offset, not display row 0.
    let left = shell.areas()[0];
    click(&mut shell, left.x + 1, left.y + 1);
    assert_eq!(shell.app.list.cursor(), offset);
}

#[test]
fn mouse_click_on_bottom_border_is_ignored() {
    let mut shell = Shell::new(HunkSelectorApp::new(make_many_files(30), make_theme()));
    render_shell(&mut shell, 80, 12);

    // The bottom border maps past the last visible row; before the guard
    // it moved the cursor to a row that was not clicked.
    let left = shell.areas()[0];
    click(&mut shell, left.x + 1, left.y + left.height - 1);
    assert_eq!(shell.app.list.cursor(), 0);
}

#[test]
fn directory_of_extracts_parent() {
    assert_eq!(directory_of("src/main.rs"), "src");
    assert_eq!(directory_of("a/b/c.rs"), "a/b");
    assert_eq!(directory_of("file.rs"), "");
}

#[test]
fn filename_of_extracts_name() {
    assert_eq!(filename_of("src/main.rs"), "main.rs");
    assert_eq!(filename_of("a/b/c.rs"), "c.rs");
    assert_eq!(filename_of("file.rs"), "file.rs");
}
