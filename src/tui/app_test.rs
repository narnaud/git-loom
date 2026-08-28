use std::collections::HashSet;
use std::path::PathBuf;

use crossterm::event::{Event, KeyEvent, MouseEvent};
use ratatui::style::Style;

use super::*;
use crate::core::graph::Section;
use crate::core::repo::{CommitInfo, FileChange, UpstreamInfo};
use crate::core::shortid::Entity;
use crate::tui::widgets::common::diff_line_style;

fn oid(hex_char: char) -> git2::Oid {
    git2::Oid::from_str(&hex_char.to_string().repeat(40)).unwrap()
}

fn file(path: &str, index: char, worktree: char) -> FileChange {
    FileChange {
        path: path.to_string(),
        index,
        worktree,
    }
}

fn make_snapshot() -> Snapshot {
    let sections = vec![
        Section::WorkingChanges(vec![file("a.rs", 'M', ' '), file("b.rs", ' ', 'M')]),
        Section::Branch {
            names: vec![("feature-a".to_string(), None)],
            commits: vec![CommitInfo {
                oid: oid('a'),
                short_id: "aaaaaaa".to_string(),
                message: "Add parser".to_string(),
                parent_oid: Some(oid('9')),
                files: vec![file("src/parser.rs", 'M', ' ')],
            }],
        },
        Section::Upstream(UpstreamInfo {
            label: "origin/main".to_string(),
            tip_oid: oid('9'),
            merge_base_oid: oid('9'),
            base_short_id: "9999999".to_string(),
            base_message: "base".to_string(),
            base_date: "2026-01-01".to_string(),
            commits_ahead: 0,
        }),
    ];
    let ids = IdAllocator::new(vec![
        Entity::Unstaged,
        Entity::Branch("feature-a".to_string()),
        Entity::Commit(oid('a')),
        Entity::File("a.rs".to_string()),
        Entity::File("b.rs".to_string()),
    ]);
    Snapshot {
        workdir: PathBuf::from("."),
        git_dir: PathBuf::from("."),
        cwd_prefix: String::new(),
        sections,
        ids,
    }
}

fn make_theme() -> TuiTheme {
    TuiTheme::from_graph_theme(&graph::Theme::dark())
}

fn make_app<'a>(snapshot: &'a Snapshot, theme: &'a TuiTheme) -> App<'a> {
    let mut expanded = HashSet::new();
    expanded.insert(LOCAL_CHANGES_KEY.to_string());
    App::new(snapshot, theme, expanded, None)
}

fn cursor_key(app: &App) -> String {
    app.rows[app.tree.cursor()].key.clone()
}

fn move_cursor_to(app: &mut App, key: &str) {
    let pos = app.rows.iter().position(|r| r.key == key).unwrap();
    app.tree.set_cursor(pos);
}

#[test]
fn cursor_starts_on_first_focusable_row() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let app = make_app(&snapshot, &theme);
    assert_eq!(cursor_key(&app), LOCAL_CHANGES_KEY);
}

#[test]
fn cursor_skips_spacer_rows() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);

    // local changes → a.rs → b.rs → (spacer skipped) → branch name
    app.move_cursor(1);
    app.move_cursor(1);
    app.move_cursor(1);
    assert_eq!(cursor_key(&app), "br:feature-a");
}

#[test]
fn expand_collapse_commit_keeps_cursor() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    let key = oid('a').to_string();
    move_cursor_to(&mut app, &key);

    app.expand_current();
    assert_eq!(cursor_key(&app), key);
    assert!(
        app.rows
            .iter()
            .any(|r| matches!(r.kind, RowKind::CommitFile { .. }))
    );

    app.collapse_current();
    assert_eq!(cursor_key(&app), key);
    assert!(
        !app.rows
            .iter()
            .any(|r| matches!(r.kind, RowKind::CommitFile { .. }))
    );
}

#[test]
fn collapse_on_child_row_collapses_parent() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    move_cursor_to(&mut app, "wf:b.rs");

    app.collapse_current();
    assert_eq!(cursor_key(&app), LOCAL_CHANGES_KEY);
    assert!(
        !app.rows
            .iter()
            .any(|r| matches!(r.kind, RowKind::WorkingFile { .. }))
    );
}

#[test]
fn space_toggles_selection_and_advances() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    move_cursor_to(&mut app, "wf:a.rs");

    app.toggle_selection();
    assert!(app.selected.contains("wf:a.rs"));
    assert_eq!(cursor_key(&app), "wf:b.rs");

    app.toggle_selection();
    assert!(app.selected.contains("wf:b.rs"));
}

#[test]
fn escape_clears_selection_before_quitting() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();

    app.handle_escape();
    assert!(app.selected.is_empty());
    assert!(app.outcome.is_none());

    app.handle_escape();
    assert!(matches!(app.outcome, Some(Outcome::Quit)));
}

#[test]
fn commit_action_collects_selected_working_files() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();
    move_cursor_to(&mut app, "wf:b.rs");
    app.toggle_selection();

    app.action_commit();
    let Some(Outcome::Run(Action::Commit { files })) = app.outcome.take() else {
        panic!("expected a commit action");
    };
    assert_eq!(files.len(), 2);
}

#[test]
fn commit_action_without_selection_uses_index_as_is() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    // Cursor on the local-changes header → no file args (index as-is).
    app.action_commit();
    let Some(Outcome::Run(Action::Commit { files })) = app.outcome.take() else {
        panic!("expected a commit action");
    };
    assert!(files.is_empty());
}

#[test]
fn commit_action_rejects_commit_rows_in_selection() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    move_cursor_to(&mut app, &oid('a').to_string());
    app.toggle_selection();

    app.action_commit();
    assert!(app.outcome.is_none());
    assert!(app.notice.is_some());
}

#[test]
fn fold_flow_uses_selection_as_sources_and_cursor_as_target() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();

    app.action_fold_start();
    assert!(matches!(app.mode, Mode::FoldTarget { .. }));

    move_cursor_to(&mut app, &oid('a').to_string());
    app.confirm_fold_target();
    let Some(Outcome::Run(Action::Fold { sources, target })) = app.outcome.take() else {
        panic!("expected a fold action");
    };
    assert_eq!(sources, vec![snapshot.ids.get_file("a.rs").to_string()]);
    assert_eq!(target, oid('a').to_string());
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn fold_rejects_target_among_sources() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    move_cursor_to(&mut app, &oid('a').to_string());
    app.toggle_selection();

    app.action_fold_start();
    move_cursor_to(&mut app, &oid('a').to_string());
    app.confirm_fold_target();

    assert!(app.outcome.is_none());
    assert!(matches!(app.mode, Mode::FoldTarget { .. }));
}

#[test]
fn fold_mode_blocks_action_keys() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.action_fold_start();
    assert!(matches!(app.mode, Mode::FoldTarget { .. }));

    for code in [
        KeyCode::Char(' '),
        KeyCode::Char('c'),
        KeyCode::Char('f'),
        KeyCode::Char('b'),
        KeyCode::Char('d'),
        KeyCode::Char('r'),
        KeyCode::Char('R'),
        KeyCode::F(5),
    ] {
        let result = app.handle_key(PaneId::Left, code, KeyModifiers::NONE);
        assert!(
            matches!(result, KeyResult::Handled),
            "{code:?} must not exit fold mode"
        );
        assert!(
            matches!(app.mode, Mode::FoldTarget { .. }),
            "{code:?} left fold mode"
        );
        assert!(app.selected.is_empty(), "{code:?} changed the selection");
        assert!(app.notice.is_some(), "{code:?} showed no notice");
    }
}

#[test]
fn escape_cancels_fold_mode() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.action_fold_start();
    assert!(matches!(app.mode, Mode::FoldTarget { .. }));

    app.handle_escape();
    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.outcome.is_none());
}

#[test]
fn drop_and_reword_require_an_actionable_row() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);

    // Upstream row: not actionable.
    let up_key = app
        .rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::Upstream { .. }))
        .unwrap()
        .key
        .clone();
    move_cursor_to(&mut app, &up_key);
    app.action_drop();
    assert!(app.outcome.is_none());
    app.action_reword();
    assert!(app.outcome.is_none());

    // Commit row: actionable.
    move_cursor_to(&mut app, &oid('a').to_string());
    app.action_reword();
    let Some(Outcome::Run(Action::Reword { target })) = app.outcome.take() else {
        panic!("expected a reword action");
    };
    assert_eq!(target, oid('a').to_string());
}

#[test]
fn new_branch_uses_cursor_commit_as_target() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    move_cursor_to(&mut app, &oid('a').to_string());

    app.action_new_branch();
    let Some(Outcome::Run(Action::NewBranch { target })) = app.outcome.take() else {
        panic!("expected a branch action");
    };
    assert_eq!(target, Some(oid('a').to_string()));
}

#[test]
fn render_smoke_test_on_every_focusable_row() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    // Expand everything so file rows render too.
    app.expanded.insert(oid('a').to_string());
    app.rebuild_rows(LOCAL_CHANGES_KEY);

    let mut shell = Shell::new(app);
    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    for i in 0..shell.app.rows.len() {
        if shell.app.rows[i].focusable {
            shell.app.tree.set_cursor(i);
            terminal.draw(|f| shell.render(f)).unwrap();
        }
    }
}

#[test]
fn mouse_click_on_tree_bottom_border_is_ignored() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut shell = Shell::new(make_app(&snapshot, &theme));

    // Small terminal so rows exist below the visible tree area.
    let backend = ratatui::backend::TestBackend::new(80, 8);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| shell.render(f)).unwrap();
    let tree_area = shell.areas()[0];
    assert!(shell.app.rows.len() > (tree_area.height as usize).saturating_sub(2));

    // Before the guard, a click on the bottom border mapped to the row just
    // past the last visible one and moved the cursor there.
    shell.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: tree_area.x + 1,
        row: tree_area.y + tree_area.height - 1,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(shell.app.tree.cursor(), 0);
}

#[test]
fn refresh_key_returns_refresh_outcome() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut app = make_app(&snapshot, &theme);
    let result = app.handle_key(PaneId::Left, KeyCode::Char('R'), KeyModifiers::NONE);
    assert!(matches!(result, KeyResult::Exit(Outcome::Refresh)));
}

/// The composed status bar must match the string pinned in specs/020-tui.md.
#[test]
fn status_bar_matches_spec() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut shell = Shell::new(make_app(&snapshot, &theme));

    let backend = ratatui::backend::TestBackend::new(150, 12);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| shell.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let last_row: String = (0..buffer.area.width)
        .map(|x| buffer[(x, buffer.area.height - 1)].symbol())
        .collect();
    assert!(
        last_row.starts_with(
            " Navigate: ↑/↓ | Fold/unfold: ←/→ | Select: space | Commit: c | Fold: f \
             | Branch: b | Drop: d | Reword: r | Refresh: R | Quit: q"
        ),
        "got: {:?}",
        last_row
    );
}

#[test]
fn ctrl_c_quits_via_shell() {
    let snapshot = make_snapshot();
    let theme = make_theme();
    let mut shell = Shell::new(make_app(&snapshot, &theme));
    let exit = shell.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert!(matches!(exit, Some(Outcome::Quit)));
}

// -- diff_text against a real repo -------------------------------------------

fn repo_snapshot(repo: &crate::core::test_helpers::TestRepo) -> Snapshot {
    Snapshot {
        workdir: repo.workdir(),
        git_dir: repo.repo.path().to_path_buf(),
        cwd_prefix: String::new(),
        sections: Vec::new(),
        ids: IdAllocator::new(Vec::new()),
    }
}

fn row(kind: RowKind, key: &str) -> Row {
    Row {
        kind,
        sid: String::new(),
        target: None,
        key: key.to_string(),
        focusable: true,
        selectable: false,
        expandable: false,
        expanded: false,
    }
}

#[test]
fn diff_text_per_row_kind() {
    let repo = crate::core::test_helpers::TestRepo::new();
    let base = repo.head_oid();
    repo.write_file("file.txt", "original content\n");
    repo.stage_files(&["file.txt"]);
    repo.commit_staged("Add file");
    let commit_oid = repo.head_oid();
    repo.write_file("file.txt", "changed\n");
    let snapshot = repo_snapshot(&repo);

    // Local changes: diff HEAD; count 0 short-circuits.
    let text = diff_text(&snapshot, &row(RowKind::LocalChanges { count: 1 }, "z"));
    assert!(text.contains("-original content"));
    assert!(text.contains("+changed"));
    let text = diff_text(&snapshot, &row(RowKind::LocalChanges { count: 0 }, "z"));
    assert_eq!(text, "no changes");

    // Working file: diff of that path.
    let kind = RowKind::WorkingFile {
        path: "file.txt".to_string(),
        index: ' ',
        worktree: 'M',
    };
    assert!(diff_text(&snapshot, &row(kind, "wf")).contains("+changed"));

    // Untracked working file routes through untracked_file_text.
    repo.write_file("new.txt", "fresh\n");
    let kind = RowKind::WorkingFile {
        path: "new.txt".to_string(),
        index: '?',
        worktree: '?',
    };
    let text = diff_text(&snapshot, &row(kind, "wf"));
    assert!(text.starts_with("untracked file: new.txt"));
    assert!(text.contains("+fresh"));

    // Branch with a range vs. one without commits of its own.
    let kind = RowKind::BranchName {
        name: "b".to_string(),
        remote: None,
        connector: "",
        range: Some((base.to_string(), commit_oid.to_string())),
    };
    assert!(diff_text(&snapshot, &row(kind, "br")).contains("original content"));
    let kind = RowKind::BranchName {
        name: "b".to_string(),
        remote: None,
        connector: "",
        range: None,
    };
    assert_eq!(
        diff_text(&snapshot, &row(kind, "br")),
        "branch has no commits of its own"
    );

    // Commit: git show with stat and patch.
    let kind = RowKind::Commit {
        oid: commit_oid,
        message: String::new(),
        sid_rest: String::new(),
        dot_color: None,
        file_count: 1,
    };
    let text = diff_text(&snapshot, &row(kind, "c"));
    assert!(text.contains("Add file"));
    assert!(text.contains("+original content"));

    // Commit file: the patch without the commit header.
    let kind = RowKind::CommitFile {
        oid: commit_oid,
        path: "file.txt".to_string(),
        index: 'M',
        worktree: ' ',
        on_branch: false,
    };
    let text = diff_text(&snapshot, &row(kind, "cf"));
    assert!(text.contains("+original content"));
    assert!(!text.contains("Add file"));

    // Upstream: informational text, no git call.
    let kind = RowKind::Upstream {
        label: "origin/main".to_string(),
        base_short_id: "9999999".to_string(),
        base_message: "base".to_string(),
        commits_ahead: 2,
    };
    let text = diff_text(&snapshot, &row(kind, "up"));
    assert!(text.contains("upstream: origin/main"));
    assert!(text.contains("2 new commit(s)"));

    // Spacer: empty.
    assert_eq!(diff_text(&snapshot, &row(RowKind::Spacer("│"), "")), "");
}

#[test]
fn untracked_file_text_renders_added_lines() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("new.txt"), "one\ntwo\n").unwrap();
    let text = untracked_file_text(dir.path(), "new.txt");
    assert!(text.starts_with("untracked file: new.txt\n"));
    assert!(text.contains("+one\n"));
    assert!(text.contains("+two\n"));
    assert!(!text.contains("..."));
}

#[test]
fn untracked_file_text_detects_binary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("blob"), b"\x00\x01\x02").unwrap();
    assert_eq!(untracked_file_text(dir.path(), "blob"), "(binary file)");
}

#[test]
fn untracked_file_text_reports_unreadable_file() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(untracked_file_text(dir.path(), "nope"), "(unreadable file)");
}

#[test]
fn untracked_file_text_truncates_oversized_files() {
    let dir = tempfile::tempdir().unwrap();
    let line = format!("{}\n", "x".repeat(1023)); // 1 KB per line
    std::fs::write(dir.path().join("big.txt"), line.repeat(300)).unwrap();
    let text = untracked_file_text(dir.path(), "big.txt");
    assert!(text.ends_with("...\n"));
    // Only the 256 KB prefix is rendered, not the whole 300 KB.
    assert!(text.len() <= 257 * 1024);
}

#[test]
fn untracked_file_text_caps_line_count() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("many.txt"), "a\n".repeat(3000)).unwrap();
    let text = untracked_file_text(dir.path(), "many.txt");
    assert!(text.ends_with("...\n"));
    assert_eq!(text.lines().filter(|l| l.starts_with('+')).count(), 2000);
}

#[test]
fn diff_line_styles_map_prefixes() {
    let theme = make_theme();
    assert_eq!(diff_line_style("+added", &theme), theme.added);
    assert_eq!(diff_line_style("-removed", &theme), theme.removed);
    assert_eq!(diff_line_style("@@ -1 +1 @@", &theme), theme.hunk_header);
    assert_eq!(diff_line_style(" context", &theme), theme.context);
    assert_ne!(diff_line_style("+++ b/file", &theme), theme.added);
    assert_ne!(diff_line_style("--- a/file", &theme), theme.removed);
}

#[test]
fn git_show_header_renders_in_normal_colors() {
    let theme = make_theme();
    let text = "commit aaaa\nAuthor: Someone\nDate: today\n\n    Add parser\n\n\
                 src/parser.rs | 2 +-\n\ndiff --git a/src/parser.rs b/src/parser.rs\n\
                 @@ -1 +1 @@\n-old\n+new\n    indented context\n";
    let lines = colorize_diff(text, &theme);
    let style_of = |needle: &str| {
        lines
            .iter()
            .find(|l| l.spans[0].content.contains(needle))
            .unwrap()
            .spans[0]
            .style
    };

    // Header: commit line dimmed, message and metadata in default colors.
    assert_eq!(style_of("commit aaaa"), theme.dim);
    assert_eq!(style_of("Add parser"), Style::default());
    assert_eq!(style_of("Author:"), Style::default());
    // Patch region still colorized, including indented context lines.
    assert_eq!(style_of("+new"), theme.added);
    assert_eq!(style_of("-old"), theme.removed);
    assert_eq!(style_of("indented context"), theme.context);
}

#[test]
fn plain_diff_text_has_no_header_region() {
    let theme = make_theme();
    let lines = colorize_diff("diff --git a/f b/f\n+new\n", &theme);
    assert_eq!(lines[1].spans[0].style, theme.added);
}
