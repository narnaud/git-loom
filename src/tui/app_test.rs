use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::channel;

use crossterm::event::{Event, KeyEvent, MouseEvent};
use ratatui::style::Style;

use super::*;
use crate::core::graph::Section;
use crate::core::repo::{CommitInfo, FileChange, UpstreamInfo};
use crate::core::shortid::Entity;
use crate::core::ui::PromptKind;
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

fn make_app(snapshot: Snapshot, theme: &TuiTheme) -> App<'_> {
    let mut expanded = HashSet::new();
    expanded.insert(LOCAL_CHANGES_KEY.to_string());
    App::new(snapshot, theme, graph::Theme::dark(), expanded)
}

fn cursor_key(app: &App) -> String {
    app.rows[app.tree.cursor()].key.clone()
}

fn move_cursor_to(app: &mut App, key: &str) {
    let pos = app.rows.iter().position(|r| r.key == key).unwrap();
    app.tree.set_cursor(pos);
}

fn press(app: &mut App, code: KeyCode) -> KeyResult<Outcome> {
    app.handle_key(PaneId::Left, code, KeyModifiers::NONE)
}

/// A worker that blocks until `release` is dropped, then ends as cancelled
/// (so finishing it touches no repository).
fn blocked_worker(app: &mut App) -> std::sync::mpsc::Sender<()> {
    let (release, wait) = channel::<()>();
    let handle = std::thread::spawn(move || {
        let _ = wait.recv();
        Err(Cancelled.into())
    });
    app.running = Some(Running {
        handle,
        command: "loom reword a1".to_string(),
        spinner: None,
        ticks: 0,
    });
    release
}

/// Poll until the worker has been reaped.
fn poll_until_idle(app: &mut App) {
    for _ in 0..200 {
        app.poll_background();
        if app.running.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("worker never finished");
}

#[test]
fn cursor_starts_on_first_focusable_row() {
    let theme = make_theme();
    let app = make_app(make_snapshot(), &theme);
    assert_eq!(cursor_key(&app), LOCAL_CHANGES_KEY);
}

#[test]
fn cursor_skips_spacer_rows() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);

    // local changes → a.rs → b.rs → (spacer skipped) → branch name
    app.move_cursor(1);
    app.move_cursor(1);
    app.move_cursor(1);
    assert_eq!(cursor_key(&app), "br:feature-a");
}

#[test]
fn expand_collapse_commit_keeps_cursor() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
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
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
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
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");

    app.toggle_selection();
    assert!(app.selected.contains("wf:a.rs"));
    assert_eq!(cursor_key(&app), "wf:b.rs");

    app.toggle_selection();
    assert!(app.selected.contains("wf:b.rs"));
}

#[test]
fn escape_clears_selection_before_quitting() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
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
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();
    move_cursor_to(&mut app, "wf:b.rs");
    app.toggle_selection();

    let Some(Action::Commit { files }) = app.action_commit() else {
        panic!("expected a commit action");
    };
    assert_eq!(files.len(), 2);
}

#[test]
fn commit_action_without_selection_uses_index_as_is() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    // Cursor on the local-changes header → no file args (index as-is).
    let Some(Action::Commit { files }) = app.action_commit() else {
        panic!("expected a commit action");
    };
    assert!(files.is_empty());
}

#[test]
fn commit_action_rejects_commit_rows_in_selection() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, &oid('a').to_string());
    app.toggle_selection();

    assert!(app.action_commit().is_none());
    assert!(app.notice.is_some());
}

#[test]
fn fold_flow_uses_selection_as_sources_and_cursor_as_target() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();

    app.action_fold_start();
    assert!(matches!(app.mode, Mode::FoldTarget { .. }));

    move_cursor_to(&mut app, &oid('a').to_string());
    let Some(Action::Fold { sources, target }) = app.confirm_fold_target() else {
        panic!("expected a fold action");
    };
    assert_eq!(sources, vec![app.snapshot.ids.get_file("a.rs").to_string()]);
    assert_eq!(target, oid('a').to_string());
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn fold_rejects_target_among_sources() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, &oid('a').to_string());
    app.toggle_selection();

    app.action_fold_start();
    move_cursor_to(&mut app, &oid('a').to_string());
    assert!(app.confirm_fold_target().is_none());
    assert!(matches!(app.mode, Mode::FoldTarget { .. }));
}

#[test]
fn fold_mode_blocks_action_keys() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
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
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.action_fold_start();
    assert!(matches!(app.mode, Mode::FoldTarget { .. }));

    app.handle_escape();
    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.outcome.is_none());
}

#[test]
fn drop_and_reword_require_an_actionable_row() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);

    // Upstream row: not actionable.
    let up_key = app
        .rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::Upstream { .. }))
        .unwrap()
        .key
        .clone();
    move_cursor_to(&mut app, &up_key);
    assert!(app.action_drop().is_none());
    assert!(app.action_reword().is_none());

    // Commit row: actionable.
    move_cursor_to(&mut app, &oid('a').to_string());
    let Some(Action::Reword { target, name }) = app.action_reword() else {
        panic!("expected a reword action");
    };
    assert_eq!(target, oid('a').to_string());
    assert_eq!(name, None);
}

#[test]
fn reword_on_a_branch_renames_it_in_the_tree() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "br:feature-a");

    assert!(matches!(
        press(&mut app, KeyCode::Char('r')),
        KeyResult::Handled
    ));
    let Mode::RenameBranch { branch, field } = &app.mode else {
        panic!("expected rename mode");
    };
    assert_eq!(branch, "feature-a");
    assert_eq!(
        field.value(),
        "feature-a",
        "the field starts at the old name"
    );
    assert!(app.modal_active(), "the field must own every key");

    // Action and quit keys type into the field instead of firing.
    for code in [KeyCode::Backspace, KeyCode::Char('q')] {
        assert!(matches!(press(&mut app, code), KeyResult::Handled));
    }
    // A chord is not text: Ctrl-U must not rename the branch to `feature-qu`.
    app.handle_key(PaneId::Left, KeyCode::Char('u'), KeyModifiers::CONTROL);
    let Mode::RenameBranch { field, .. } = &app.mode else {
        panic!("expected rename mode");
    };
    assert_eq!(field.value(), "feature-q");
    assert!(
        app.handle_rename_key(KeyCode::Backspace, KeyModifiers::NONE)
            .is_none()
    );
    assert!(
        app.handle_rename_key(KeyCode::Char('b'), KeyModifiers::NONE)
            .is_none()
    );
    let Some(Action::Reword { target, name }) =
        app.handle_rename_key(KeyCode::Enter, KeyModifiers::NONE)
    else {
        panic!("expected a reword action");
    };
    assert_eq!(target, "feature-a");
    assert_eq!(name.as_deref(), Some("feature-b"));
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.next_cursor.as_deref(), Some("br:feature-b"));

    // A failed rename leaves the cursor on the row that kept its name.
    app.finish_action(Err(anyhow::anyhow!("boom")));
    assert_eq!(app.next_cursor, None);
}

#[test]
fn rename_is_cancelled_by_escape_and_by_an_unchanged_name() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "br:feature-a");

    app.action_reword();
    assert!(
        app.handle_rename_key(KeyCode::Esc, KeyModifiers::NONE)
            .is_none()
    );
    assert!(matches!(app.mode, Mode::Normal));

    app.action_reword();
    assert!(
        app.handle_rename_key(KeyCode::Enter, KeyModifiers::NONE)
            .is_none()
    );
    assert!(matches!(app.mode, Mode::Normal));

    // An empty name keeps the field open rather than renaming to nothing.
    app.action_reword();
    for _ in 0.."feature-a".len() {
        app.handle_rename_key(KeyCode::Backspace, KeyModifiers::NONE);
    }
    assert!(
        app.handle_rename_key(KeyCode::Enter, KeyModifiers::NONE)
            .is_none()
    );
    assert!(matches!(app.mode, Mode::RenameBranch { .. }));
}

#[test]
fn new_branch_uses_cursor_commit_as_target() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, &oid('a').to_string());

    let Some(Action::NewBranch { target }) = app.action_new_branch() else {
        panic!("expected a branch action");
    };
    assert_eq!(target, Some(oid('a').to_string()));
}

#[test]
fn command_line_uses_the_short_ids_the_tree_shows() {
    let theme = make_theme();
    let app = make_app(make_snapshot(), &theme);
    let ids = &app.snapshot.ids;
    let commit = ids.get_commit(oid('a')).to_string();
    let file = ids.get_file("a.rs").to_string();

    assert_eq!(
        app.command_line(&Action::Fold {
            sources: vec![file.clone(), ids.get_unstaged().to_string()],
            target: oid('a').to_string(),
        }),
        format!("loom fold {} {} {}", file, ids.get_unstaged(), commit)
    );
    assert_eq!(
        app.command_line(&Action::Commit { files: vec![] }),
        "loom commit"
    );
    assert_eq!(
        app.command_line(&Action::NewBranch {
            target: Some("feature-a".to_string()),
        }),
        format!("loom branch new -t {}", ids.get_branch("feature-a"))
    );
    assert_eq!(
        app.command_line(&Action::NewBranch { target: None }),
        "loom branch new"
    );
    assert_eq!(
        app.command_line(&Action::Drop {
            target: oid('a').to_string(),
        }),
        format!("loom drop {}", commit)
    );
    // A target no row shows (rewritten meanwhile) is passed through.
    assert_eq!(
        app.command_line(&Action::Reword {
            target: "deadbeef".to_string(),
            name: None,
        }),
        "loom reword deadbeef"
    );
    assert_eq!(
        app.command_line(&Action::Reword {
            target: "feature-a".to_string(),
            name: Some("feature-b".to_string()),
        }),
        format!("loom reword {} -m feature-b", ids.get_branch("feature-a"))
    );
}

#[test]
fn prompt_request_opens_a_popup_that_owns_the_keys_and_replies() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    let (reply, answer) = channel();
    app.handle_request(Request::Prompt {
        kind: PromptKind::Input { placeholder: None },
        prompt: "Branch name".to_string(),
        error: None,
        reply,
    });
    assert!(app.modal_active());
    let Some(Popup::Prompt { prompt, .. }) = &app.popup else {
        panic!("expected a prompt popup");
    };
    assert_eq!(prompt.title(), "Branch name");

    // Action keys type into the field instead of firing.
    for c in ['f', 'd', 'q'] {
        assert!(matches!(
            press(&mut app, KeyCode::Char(c)),
            KeyResult::Handled
        ));
    }
    assert!(app.popup.is_some());
    press(&mut app, KeyCode::Enter);
    assert!(app.popup.is_none());
    assert_eq!(
        answer.recv().unwrap(),
        Some(Answer::Text("fdq".to_string()))
    );

    let (reply, answer) = channel();
    app.handle_request(Request::Prompt {
        kind: PromptKind::Confirm,
        prompt: "Drop?".to_string(),
        error: None,
        reply,
    });
    press(&mut app, KeyCode::Esc);
    assert_eq!(answer.recv().unwrap(), None);
    assert!(app.popup.is_none());
}

#[test]
fn messages_land_in_the_current_log_entry() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    app.log.push(LogEntry {
        command: "loom reword a1".to_string(),
        lines: Vec::new(),
    });
    app.handle_request(Request::Message {
        level: Level::Success,
        text: "Reworded `a1`".to_string(),
    });
    assert_eq!(
        app.log[0].lines,
        vec![(Level::Success, "Reworded `a1`".to_string())]
    );

    // Success: the last ✓ line becomes the status-bar notice.
    assert!(app.finish_action(Ok(())));
    assert_eq!(app.notice.as_deref(), Some("✓ Reworded `a1`"));
    assert!(app.popup.is_none());

    // Failure: an error popup, and the line is logged.
    assert!(!app.finish_action(Err(anyhow::anyhow!("Nothing to commit"))));
    assert!(matches!(
        app.popup,
        Some(Popup::Notice {
            then: AfterNotice::Reload,
            ..
        })
    ));
    assert_eq!(
        app.log[0].lines.last(),
        Some(&(Level::Error, "Nothing to commit".to_string()))
    );
    press(&mut app, KeyCode::Char('x'));
    assert!(app.popup.is_some(), "only Enter/Esc dismiss a notice");

    // A cancelled prompt is not an error.
    app.popup = None;
    assert!(!app.finish_action(Err(Cancelled.into())));
    assert_eq!(app.notice.as_deref(), Some("cancelled"));
    assert!(app.popup.is_none());
}

/// A failure while the log is open must not yank the log out from under the
/// reader; the reload happens behind it instead.
#[test]
fn a_failure_keeps_an_open_log() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    app.log.push(LogEntry {
        command: "loom drop a1".to_string(),
        lines: Vec::new(),
    });
    app.open_log();

    assert!(app.finish_action(Err(anyhow::anyhow!("Nothing to commit"))));
    assert!(matches!(app.popup, Some(Popup::Log { .. })));
    assert_eq!(
        app.log[0].lines.last(),
        Some(&(Level::Error, "Nothing to commit".to_string()))
    );
}

#[test]
fn running_action_blocks_tree_keys_until_it_finishes() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    let release = blocked_worker(&mut app);
    assert!(app.modal_active());

    press(&mut app, KeyCode::Char('c'));
    assert!(app.popup.is_none());
    assert_eq!(app.notice.as_deref(), Some("an action is running…"));
    assert!(matches!(app.poll_background(), Tick::Redraw));
    assert!(app.mode_hint().unwrap().contains("loom reword a1"));

    // The log stays reachable while waiting.
    press(&mut app, KeyCode::Char('L'));
    assert!(matches!(app.popup, Some(Popup::Log { .. })));
    press(&mut app, KeyCode::Esc);
    assert!(app.popup.is_none());

    drop(release);
    poll_until_idle(&mut app);
    assert!(!app.modal_active());
    assert_eq!(app.notice.as_deref(), Some("cancelled"));
    assert!(matches!(app.poll_background(), Tick::Idle));
}

#[test]
fn suspend_request_reaches_the_shell_and_resume_ends_the_wait() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    let release = blocked_worker(&mut app);
    let (ack, acked) = channel();
    app.request_tx.send(Request::Suspend(ack)).unwrap();
    let Tick::Suspend(ack) = app.poll_background() else {
        panic!("expected a suspend tick");
    };
    ack.send(()).unwrap();
    assert!(acked.recv().is_ok());

    app.request_tx.send(Request::Resume).unwrap();
    app.wait_for_resume();

    // A worker gone without a Resume ends the wait too.
    drop(release);
    app.wait_for_resume();
    poll_until_idle(&mut app);
}

#[test]
fn log_popup_toggles_and_scrolls() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    press(&mut app, KeyCode::Char('L'));
    assert!(matches!(app.popup, Some(Popup::Log { .. })));
    press(&mut app, KeyCode::Down);
    assert!(matches!(app.popup, Some(Popup::Log { .. })));
    press(&mut app, KeyCode::Char('L'));
    assert!(app.popup.is_none());
}

#[test]
fn apply_snapshot_keeps_cursor_and_expansion_but_drops_selection() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    let key = oid('a').to_string();
    move_cursor_to(&mut app, &key);
    app.expand_current();
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();
    move_cursor_to(&mut app, &key);

    app.apply_snapshot(make_snapshot());
    assert_eq!(cursor_key(&app), key);
    assert!(
        app.rows
            .iter()
            .any(|r| matches!(r.kind, RowKind::CommitFile { .. }))
    );
    assert!(app.selected.is_empty());
    assert!(
        app.diff_cache.len() <= 1,
        "cache holds at most the cursor row"
    );
}

#[test]
fn popups_render_over_the_panes() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    let (reply, _answer) = channel();
    app.handle_request(Request::Prompt {
        kind: PromptKind::Select {
            items: vec!["feature-a".to_string()],
            allow_other: true,
        },
        prompt: "Target branch".to_string(),
        error: None,
        reply,
    });
    let mut shell = Shell::new(app);
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| shell.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains(" Target branch "));
    assert!(text.contains("feature-a"));
}

#[test]
fn rename_draws_the_edited_name_on_the_branch_row() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "br:feature-a");
    press(&mut app, KeyCode::Char('r'));
    for code in [KeyCode::Backspace, KeyCode::Char('z')] {
        press(&mut app, code);
    }

    let mut shell = Shell::new(app);
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| shell.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    // The trailing space of the field carries the cursor cell.
    assert!(
        text.contains("[feature-z ]"),
        "field drawn in place: {text}"
    );
    assert!(text.contains(" Rename branch "));
    assert!(text.contains("Esc to cancel"));

    let (x, y) = (0..buffer.area.height)
        .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
        .find(|&(x, y)| {
            buffer[(x, y)]
                .style()
                .add_modifier
                .contains(Modifier::REVERSED)
        })
        .expect("no cursor cell");
    assert_eq!(
        buffer[(x - 1, y)].symbol(),
        "z",
        "cursor sits after the name"
    );
}

#[test]
fn render_smoke_test_on_every_focusable_row() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
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
    let theme = make_theme();
    let mut shell = Shell::new(make_app(make_snapshot(), &theme));

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

/// The composed status bar must match the string pinned in specs/020-tui.md.
#[test]
fn status_bar_matches_spec() {
    let theme = make_theme();
    let mut shell = Shell::new(make_app(make_snapshot(), &theme));

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
             | Branch: b | Drop: d | Reword: r | Log: L | Refresh: R | Quit: q"
        ),
        "got: {:?}",
        last_row
    );
}

#[test]
fn ctrl_c_quits_via_shell() {
    let theme = make_theme();
    let mut shell = Shell::new(make_app(make_snapshot(), &theme));
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
