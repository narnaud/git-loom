use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::channel;

use crossterm::event::{Event, KeyEvent, MouseEvent};
use ratatui::style::Style;

use super::*;
use crate::core::repo::{BranchInfo, CommitInfo, ContextCommit, FileChange, UpstreamInfo};
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

fn commit(c: char, parent: char, message: &str) -> CommitInfo {
    CommitInfo {
        oid: oid(c),
        short_id: c.to_string().repeat(7),
        message: message.to_string(),
        change_id: None,
        parent_oid: Some(oid(parent)),
        files: vec![file("src/parser.rs", 'M', ' ')],
    }
}

fn context_commit(short_hash: &str) -> ContextCommit {
    ContextCommit {
        short_hash: short_hash.to_string(),
        message: "older".to_string(),
        date: "2025-12-31".to_string(),
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

/// `feature-a` owning one commit `a` on base `9`, two local changes.
fn make_info() -> RepoInfo {
    RepoInfo {
        branch_name: "integration".to_string(),
        upstream: upstream(),
        commits: vec![commit('a', '9', "Add parser")],
        branches: vec![BranchInfo {
            name: "feature-a".to_string(),
            tip_oid: oid('a'),
            remote: None,
        }],
        working_changes: vec![file("a.rs", 'M', ' '), file("b.rs", ' ', 'M')],
        context_commits: Vec::new(),
    }
}

fn snapshot_of(info: RepoInfo) -> Snapshot {
    let ids = IdAllocator::new(info.collect_entities());
    Snapshot {
        workdir: PathBuf::from("."),
        git_dir: PathBuf::from("."),
        cwd_prefix: String::new(),
        info,
        ids,
    }
}

fn make_snapshot() -> Snapshot {
    snapshot_of(make_info())
}

fn make_theme() -> TuiTheme {
    TuiTheme::from_graph_theme(&graph::Theme::dark())
}

fn make_app(snapshot: Snapshot, theme: &TuiTheme) -> App<'_> {
    let mut expanded = HashSet::new();
    expanded.insert(LOCAL_CHANGES_KEY.to_string());
    App::new(snapshot, theme, graph::Theme::dark(), expanded, 1)
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
fn selection_holds_one_class_of_row() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();

    move_cursor_to(&mut app, &oid('a').to_string());
    app.toggle_selection();
    assert_eq!(app.selected.len(), 1);
    assert!(app.selected.contains("wf:a.rs"));
    assert_eq!(cursor_key(&app), oid('a').to_string());
    assert_eq!(
        app.notice.as_deref(),
        Some("selection holds files; Esc clears it")
    );
}

/// `zz` subsumes the files under it, so the header is its own class.
#[test]
fn local_changes_header_does_not_mix_with_its_files() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, LOCAL_CHANGES_KEY);
    app.toggle_selection();

    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();
    assert_eq!(app.selected.len(), 1);
    assert!(app.selected.contains(LOCAL_CHANGES_KEY));
}

#[test]
fn deselecting_the_last_row_frees_the_class() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();
    assert!(app.selected.is_empty());

    move_cursor_to(&mut app, &oid('a').to_string());
    app.toggle_selection();
    assert!(app.selected.contains(&oid('a').to_string()));
}

#[test]
fn deselecting_one_of_several_rows_keeps_the_class() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();
    app.toggle_selection();
    move_cursor_to(&mut app, "wf:b.rs");
    app.toggle_selection();
    assert_eq!(app.selected.len(), 1);

    move_cursor_to(&mut app, &oid('a').to_string());
    app.toggle_selection();
    assert!(!app.selected.contains(&oid('a').to_string()));
}

/// A selection the user can no longer see must not veto the next one.
#[test]
fn collapsing_a_parent_forgets_the_rows_it_hides() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, &oid('a').to_string());
    app.expand_current();
    let file_key = format!("{}:0", oid('a'));
    move_cursor_to(&mut app, &file_key);
    app.toggle_selection();

    move_cursor_to(&mut app, LOCAL_CHANGES_KEY);
    app.collapse_current();
    assert!(app.selected.contains(&file_key));

    move_cursor_to(&mut app, &oid('a').to_string());
    app.collapse_current();
    assert!(app.selected.is_empty());

    app.toggle_selection();
    assert!(app.selected.contains(&oid('a').to_string()));
    assert_eq!(app.notice, None);
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

fn pending_commit_key() -> String {
    PENDING_COMMIT_OID.to_string()
}

#[test]
fn commit_collects_the_selected_working_files_and_lands_on_integration() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();
    move_cursor_to(&mut app, "wf:b.rs");
    app.toggle_selection();

    press(&mut app, KeyCode::Char('c'));
    assert_eq!(cursor_key(&app), pending_commit_key(), "the cursor follows");
    assert_eq!(
        app.rows[app.tree.cursor()].target,
        None,
        "the placeholder names no object, so it is no command's argument"
    );
    // Default destination: the integration line, its own section above every
    // feature branch.
    let at = app.tree.cursor();
    let branch = app.rows.iter().position(|r| r.key == "br:feature-a");
    assert_eq!(branch, Some(at + 2), "one spacer between the two sections");

    let Some(Action::Commit { source, dest }) = app.confirm_commit_target() else {
        panic!("expected a commit action");
    };
    let CommitSource::Files(files) = source else {
        panic!("`c` names its files");
    };
    assert_eq!(files.len(), 2);
    assert_eq!(dest, CommitDest::Integration);
    assert!(matches!(app.mode, Mode::Normal));

    // A failed commit leaves no new row to land on, and the placeholder is
    // gone after the reload: go back to where `c` was pressed — the row
    // Space advanced onto after the last selected file.
    app.finish_action(Err(anyhow::anyhow!("boom")));
    assert_eq!(app.next_cursor.as_deref(), Some("br:feature-a"));
    assert_eq!(app.fallback_cursor, None);
}

/// Down moves the commit to `feature-a`'s tip, drawn inside the branch above
/// the commit it owns; there is nothing below it to move to.
#[test]
fn commit_destination_moves_with_the_arrows() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    press(&mut app, KeyCode::Char('c'));

    press(&mut app, KeyCode::Down);
    let at = app.tree.cursor();
    assert_eq!(cursor_key(&app), pending_commit_key());
    assert_eq!(app.rows[at - 1].key, "br:feature-a");
    assert_eq!(app.rows[at + 1].key, oid('a').to_string());

    press(&mut app, KeyCode::Down);
    assert_eq!(app.tree.cursor(), at, "the last destination is the end");

    press(&mut app, KeyCode::Up);
    let Some(Action::Commit { dest, .. }) = app.confirm_commit_target() else {
        panic!("expected a commit action");
    };
    assert_eq!(dest, CommitDest::Integration);
}

#[test]
fn commit_on_the_local_changes_header_takes_everything_as_zz() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, LOCAL_CHANGES_KEY);

    press(&mut app, KeyCode::Char('c'));
    let Some(Action::Commit { source, .. }) = app.confirm_commit_target() else {
        panic!("expected a commit action");
    };
    assert_eq!(
        source,
        CommitSource::Files(vec![app.snapshot.ids.get_unstaged().to_string()])
    );
}

#[test]
fn commit_with_hunks_asks_for_the_terminal_before_placing_anything() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");

    press(&mut app, KeyCode::Char('C'));
    assert!(matches!(app.mode, Mode::Normal), "no destination yet");
    let (files, origin) = app.pending_pick.clone().expect("no pick requested");
    assert_eq!(files, vec![app.snapshot.ids.get_unstaged().to_string()]);
    assert_eq!(origin, "wf:a.rs");
    assert!(matches!(app.poll_background(), Tick::TakeOver));
}

/// The selector rewrites the whole index, so it opens over every local change
/// — the cursor row and the selection only decide where the cursor goes back.
#[test]
fn commit_with_hunks_picks_from_every_local_change() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    app.toggle_selection();
    move_cursor_to(&mut app, &oid('a').to_string());

    press(&mut app, KeyCode::Char('C'));
    let (files, _) = app.pending_pick.clone().expect("no pick requested");
    assert_eq!(files, vec![app.snapshot.ids.get_unstaged().to_string()]);
}

/// `C` is in the key list that is inert while a commit is being placed, so it
/// cannot discard the pending placement to start a pick.
#[test]
fn commit_with_hunks_is_inert_while_a_commit_is_being_placed() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    press(&mut app, KeyCode::Char('c'));
    assert!(matches!(app.mode, Mode::CommitTarget { .. }));

    press(&mut app, KeyCode::Char('C'));
    assert!(app.pending_pick.is_none(), "no pick may start");
    assert!(
        matches!(app.mode, Mode::CommitTarget { .. }),
        "still placing"
    );
}

#[test]
fn commit_with_hunks_refuses_an_empty_working_tree() {
    let mut info = make_info();
    info.working_changes.clear();
    let theme = make_theme();
    let mut app = make_app(snapshot_of(info), &theme);

    press(&mut app, KeyCode::Char('C'));
    assert!(app.pending_pick.is_none());
    assert_eq!(app.notice.as_deref(), Some("commit: no local changes"));
}

/// The pick is staged before the placement, so the placeholder counts the
/// files the index holds — `a.rs` here, not the two local changes.
#[test]
fn a_staged_commit_counts_the_files_the_index_holds() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);

    app.enter_commit_target(CommitSource::Index, LOCAL_CHANGES_KEY.to_string());

    let mut shell = Shell::new(app);
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| shell.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let lines: Vec<String> = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[CREATE COMMIT] new commit (1 file)")),
        "{lines:#?}"
    );

    // The staging already happened, so the commit itself names no files.
    let app = &mut shell.app;
    let Some(action) = app.confirm_commit_target() else {
        panic!("expected a commit action");
    };
    assert_eq!(
        action,
        Action::Commit {
            source: CommitSource::Index,
            dest: CommitDest::Integration,
        }
    );
    assert_eq!(app.command_line(&action), "loom commit -i");
}

/// The half of `C` that writes the index. The selector needs a terminal and
/// cannot run here, so the entries it would return are built directly.
fn picked_entries(repo: &crate::core::test_helpers::TestRepo, keep: bool) -> Vec<FileEntry> {
    let mut entries =
        crate::core::staging::collect_file_entries(&repo.repo, &repo.workdir(), None).unwrap();
    for entry in &mut entries {
        for hunk in &mut entry.hunks {
            hunk.selected = keep;
        }
    }
    entries
}

fn repo_with_one_unstaged_hunk() -> crate::core::test_helpers::TestRepo {
    let repo = crate::core::test_helpers::TestRepo::new();
    repo.write_file(
        "tracked.txt",
        "one
",
    );
    repo.stage_files(&["tracked.txt"]);
    repo.commit_staged("Add file");
    repo.write_file(
        "tracked.txt",
        "one
two
",
    );
    repo
}

fn app_on<'a>(repo: &crate::core::test_helpers::TestRepo, theme: &'a TuiTheme) -> App<'a> {
    let mut info = make_info();
    info.working_changes = vec![file("tracked.txt", ' ', 'M')];
    let snapshot = Snapshot {
        workdir: repo.workdir(),
        git_dir: repo.repo.path().to_path_buf(),
        ..snapshot_of(info)
    };
    make_app(snapshot, theme)
}

#[test]
fn a_pick_stages_the_hunks_it_kept() {
    let repo = repo_with_one_unstaged_hunk();
    let theme = make_theme();
    let mut app = app_on(&repo, &theme);

    let picked = picked_entries(&repo, true);
    assert!(app.apply_pick(&["zz".to_string()], picked).unwrap());

    let status = repo.status_porcelain();
    assert!(status.contains("M  tracked.txt"), "{status}");
    assert_eq!(app.log.last().unwrap().command, "loom add -p zz");
}

/// Keeping nothing is not a failure and not a write: the index is left exactly
/// as it was, and no `loom add -p` is logged for a command that did nothing.
#[test]
fn a_pick_that_kept_nothing_leaves_the_index_alone() {
    let repo = repo_with_one_unstaged_hunk();
    let theme = make_theme();
    let mut app = app_on(&repo, &theme);
    let before = repo.status_porcelain();
    let logged = app.log.len();

    let picked = picked_entries(&repo, false);
    assert!(!app.apply_pick(&["zz".to_string()], picked).unwrap());

    assert_eq!(repo.status_porcelain(), before);
    assert_eq!(app.log.len(), logged, "nothing ran, nothing logged");
}

/// The pane previews the index the selector staged, not the whole worktree.
#[test]
fn a_staged_commit_previews_the_index() {
    let repo = crate::core::test_helpers::TestRepo::new();
    repo.write_file("staged.txt", "original");
    repo.stage_files(&["staged.txt"]);
    repo.commit_staged("Add file");
    repo.write_file("staged.txt", "picked");
    repo.stage_files(&["staged.txt"]);
    repo.write_file("loose.txt", "left behind");

    let mut info = make_info();
    info.working_changes = vec![file("staged.txt", 'M', ' '), file("loose.txt", '?', '?')];
    let snapshot = Snapshot {
        workdir: repo.workdir(),
        git_dir: repo.repo.path().to_path_buf(),
        ..snapshot_of(info)
    };
    let theme = make_theme();
    let mut app = make_app(snapshot, &theme);

    app.enter_commit_target(CommitSource::Index, LOCAL_CHANGES_KEY.to_string());
    app.ensure_diff_cached();

    let text: String = app.diff_cache[&pending_commit_key()]
        .iter()
        .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
        .collect();
    assert!(text.contains("+picked"), "got: {text}");
    assert!(
        !text.contains("loose.txt"),
        "the unstaged file is not in it: {text}"
    );
}

#[test]
fn commit_refuses_rows_that_are_not_local_changes() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, &oid('a').to_string());
    app.toggle_selection();

    press(&mut app, KeyCode::Char('c'));
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(
        app.notice.as_deref(),
        Some("commit: move to local changes or select files")
    );
}

/// Nothing to commit: `zz` on an empty working tree would only fail deeper in.
#[test]
fn commit_refuses_an_empty_working_tree() {
    let mut info = make_info();
    info.working_changes.clear();
    let theme = make_theme();
    let mut app = make_app(snapshot_of(info), &theme);
    move_cursor_to(&mut app, LOCAL_CHANGES_KEY);

    press(&mut app, KeyCode::Char('c'));
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.notice.as_deref(), Some("commit: no local changes"));
}

/// Two branches at one tip are two destinations, and committing to either
/// splits the group — the placeholder ends up under the name it was sent to,
/// never the other one.
#[test]
fn a_co_located_group_gives_one_destination_per_name() {
    let mut info = make_info();
    info.branches.push(BranchInfo {
        name: "feature-b".to_string(),
        tip_oid: oid('a'),
        remote: None,
    });
    let theme = make_theme();
    let mut app = make_app(snapshot_of(info), &theme);

    press(&mut app, KeyCode::Char('c'));
    let Mode::CommitTarget { dests, .. } = &app.mode else {
        panic!("expected commit mode");
    };
    let dests = dests.clone();
    assert_eq!(dests.len(), 3, "the integration line plus both names");

    for dest in &dests[1..] {
        press(&mut app, KeyCode::Down);
        let at = app.tree.cursor();
        let CommitDest::Branch(name) = dest else {
            panic!("only the first destination is the integration line");
        };
        assert_eq!(
            app.rows[at - 1].key,
            branch_key(name),
            "placeholder is not under {name}"
        );
        let other = branch_key(if name == "feature-a" {
            "feature-b"
        } else {
            "feature-a"
        });
        let other_at = app.rows.iter().position(|r| r.key == other);
        assert!(
            other_at > Some(at),
            "the other name did not stay behind with the commit"
        );
    }
}

/// `feature-b` (owning `a`) is stacked on `feature-a` (owning `b`):
/// committing to the lower branch keeps the upper one on top of the new
/// commit, as the relocation rebase will leave it.
#[test]
fn commit_to_a_branch_under_a_stack_keeps_the_stack() {
    let mut info = make_info();
    info.commits = vec![
        commit('a', 'b', "Add parser"),
        commit('b', '9', "Add lexer"),
    ];
    info.branches = vec![
        BranchInfo {
            name: "feature-a".to_string(),
            tip_oid: oid('b'),
            remote: None,
        },
        BranchInfo {
            name: "feature-b".to_string(),
            tip_oid: oid('a'),
            remote: None,
        },
    ];
    let theme = make_theme();
    let mut app = make_app(snapshot_of(info), &theme);

    press(&mut app, KeyCode::Char('c'));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    let Mode::CommitTarget { dests, index, .. } = &app.mode else {
        panic!("expected commit mode");
    };
    assert_eq!(dests[*index], CommitDest::Branch("feature-a".to_string()));

    let at = app.tree.cursor();
    assert_eq!(app.rows[at - 1].key, "br:feature-a");
    assert_eq!(app.rows[at + 1].key, oid('b').to_string());
    assert!(
        app.rows[..at].iter().any(|r| r.key == oid('a').to_string()),
        "feature-b still stacked above"
    );
}

/// A branch with no commits of its own is at the base: its commit forks from
/// there, parallel to the other branches, not stacked on them (Spec 006).
#[test]
fn commit_to_an_empty_branch_forks_from_the_base() {
    let mut info = make_info();
    info.branches.push(BranchInfo {
        name: "feature-b".to_string(),
        tip_oid: oid('9'),
        remote: None,
    });
    let theme = make_theme();
    let mut app = make_app(snapshot_of(info), &theme);

    press(&mut app, KeyCode::Char('c'));
    let Mode::CommitTarget { dests, .. } = &app.mode else {
        panic!("expected commit mode");
    };
    assert_eq!(
        dests.clone(),
        vec![
            CommitDest::Integration,
            CommitDest::Branch("feature-b".to_string()),
            CommitDest::Branch("feature-a".to_string()),
        ]
    );

    press(&mut app, KeyCode::Down);
    let at = app.tree.cursor();
    assert_eq!(app.rows[at - 1].key, "br:feature-b");
    // Its own section, below feature-a's, so the two stay parallel.
    assert!(
        app.rows[..at].iter().any(|r| r.key == oid('a').to_string()),
        "feature-a still owns its commit above"
    );
}

/// Every destination draws the placeholder in the section it names, in the
/// order `↑`/`↓` walk them: a stack and a branch owning nothing included.
#[test]
fn every_commit_destination_draws_the_placeholder_in_its_own_section() {
    let mut info = make_info();
    info.commits = vec![
        commit('a', 'b', "Add parser"),
        commit('b', '9', "Add lexer"),
    ];
    info.branches = vec![
        BranchInfo {
            name: "feature-a".to_string(),
            tip_oid: oid('b'),
            remote: None,
        },
        BranchInfo {
            name: "feature-b".to_string(),
            tip_oid: oid('a'),
            remote: None,
        },
        BranchInfo {
            name: "feature-c".to_string(),
            tip_oid: oid('9'),
            remote: None,
        },
    ];
    let theme = make_theme();
    let mut app = make_app(snapshot_of(info), &theme);

    press(&mut app, KeyCode::Char('c'));
    let Mode::CommitTarget { dests, .. } = &app.mode else {
        panic!("expected commit mode");
    };
    let dests = dests.clone();
    assert_eq!(dests.len(), 4, "the integration line plus three branches");
    assert_eq!(dests[0], CommitDest::Integration);

    for dest in &dests[1..] {
        press(&mut app, KeyCode::Down);
        let at = app.tree.cursor();
        assert_eq!(cursor_key(&app), PENDING_COMMIT_OID.to_string());
        let CommitDest::Branch(name) = dest else {
            panic!("only the first destination is the integration line");
        };
        assert_eq!(
            app.rows[at - 1].key,
            branch_key(name),
            "placeholder is not under {name}"
        );
    }
}

/// Every placement reuses one row key, so a new one must not show the diff
/// of the files the last one picked; moving the commit keeps it, since only
/// the destination changed.
#[test]
fn a_new_placement_drops_the_cached_preview_but_moving_keeps_it() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    press(&mut app, KeyCode::Char('c'));

    let key = pending_commit_key();
    let marker = vec![Line::from("stale")];
    app.diff_cache.insert(key.clone(), marker.clone());
    press(&mut app, KeyCode::Down);
    assert_eq!(
        app.diff_cache.get(&key),
        Some(&marker),
        "moving recomputed a diff that cannot have changed"
    );

    app.handle_escape();
    move_cursor_to(&mut app, "wf:b.rs");
    press(&mut app, KeyCode::Char('c'));
    assert_ne!(
        app.diff_cache.get(&key),
        Some(&marker),
        "the previous placement's diff survived"
    );
}

/// The cursor must stay on the placeholder: the wheel moves the destination
/// like `↑`/`↓`, and a click, which would land anywhere, is ignored.
#[test]
fn mouse_moves_the_destination_but_never_leaves_the_placeholder() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    press(&mut app, KeyCode::Char('c'));

    let mut shell = Shell::new(app);
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| shell.render(f)).unwrap();
    let tree_area = shell.areas()[0];
    let at = |kind| {
        Event::Mouse(MouseEvent {
            kind,
            column: tree_area.x + 1,
            row: tree_area.y + 1,
            modifiers: KeyModifiers::NONE,
        })
    };

    shell.handle_event(at(MouseEventKind::ScrollDown));
    let Mode::CommitTarget { dests, index, .. } = &shell.app.mode else {
        panic!("expected commit mode");
    };
    assert_eq!(dests[*index], CommitDest::Branch("feature-a".to_string()));
    assert_eq!(cursor_key(&shell.app), PENDING_COMMIT_OID.to_string());

    shell.handle_event(at(MouseEventKind::Down(MouseButton::Left)));
    assert_eq!(cursor_key(&shell.app), PENDING_COMMIT_OID.to_string());
    let Mode::CommitTarget { dests, index, .. } = &shell.app.mode else {
        panic!("expected commit mode");
    };
    assert_eq!(
        dests[*index],
        CommitDest::Branch("feature-a".to_string()),
        "a click changed the destination"
    );
}

#[test]
fn commit_is_cancelled_by_escape() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    let rows_before = app.rows.len();

    press(&mut app, KeyCode::Char('c'));
    assert!(app.rows.iter().any(|r| r.key == pending_commit_key()));

    app.handle_escape();
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.rows.len(), rows_before, "the placeholder is gone");
    assert_eq!(cursor_key(&app), "wf:a.rs");
    assert!(app.outcome.is_none());
}

#[test]
fn commit_mode_blocks_action_keys() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    press(&mut app, KeyCode::Char('c'));

    for code in [
        KeyCode::Char(' '),
        KeyCode::Char('c'),
        KeyCode::Char('f'),
        KeyCode::Char('b'),
        KeyCode::Char('d'),
        KeyCode::Char('r'),
        KeyCode::Char('R'),
        KeyCode::Char('+'),
        KeyCode::Char('='),
        KeyCode::Char('-'),
        KeyCode::F(5),
        // Folding walks the cursor off a row, so it is blocked too.
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Char('h'),
        KeyCode::Char('l'),
    ] {
        press(&mut app, code);
        assert!(
            matches!(app.mode, Mode::CommitTarget { .. }),
            "{code:?} left commit mode"
        );
        assert_eq!(cursor_key(&app), pending_commit_key(), "{code:?} moved on");
        assert_eq!(
            app.notice.as_deref(),
            Some("commit: Enter to confirm, Esc to cancel"),
            "{code:?} reported the wrong mode"
        );
    }
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
        KeyCode::Char('+'),
        KeyCode::Char('='),
        KeyCode::Char('-'),
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
    assert_eq!(
        app.action_drop(),
        Some(Action::Drop {
            targets: vec![oid('a').to_string()]
        })
    );
}

#[test]
fn drop_takes_the_local_changes_header_as_zz() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, LOCAL_CHANGES_KEY);
    let zz = app.snapshot.ids.get_unstaged().to_string();
    assert_eq!(app.action_drop(), Some(Action::Drop { targets: vec![zz] }));
}

#[test]
fn drop_takes_every_selected_working_file_but_only_files_together() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char(' '));
    // The cursor moved on past the selection; the selection still wins.
    let ids = &app.snapshot.ids;
    let expected = vec![
        ids.get_file("a.rs").to_string(),
        ids.get_file("b.rs").to_string(),
    ];
    assert_eq!(app.action_drop(), Some(Action::Drop { targets: expected }));

    app.selected.insert(oid('a').to_string());
    assert!(app.action_drop().is_none());
    assert_eq!(
        app.notice.as_deref(),
        Some("drop: only files can be dropped together")
    );
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

/// Key of the fake branch drawn while a new branch is being named.
fn pending_key() -> String {
    branch_key(NEW_BRANCH_NAME)
}

#[test]
fn new_branch_on_a_tip_commit_is_drawn_co_located_with_its_branch() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, &oid('a').to_string());

    press(&mut app, KeyCode::Char('b'));
    let Mode::NewBranch {
        target,
        tip,
        origin,
        ..
    } = &app.mode
    else {
        panic!("expected new-branch mode");
    };
    assert_eq!(target.as_deref(), Some(oid('a').to_string().as_str()));
    assert_eq!(*tip, oid('a'));
    assert_eq!(origin, &oid('a').to_string());
    assert_eq!(
        cursor_key(&app),
        pending_key(),
        "the field takes the cursor"
    );
    assert!(app.modal_active(), "the field must own every key");

    // Same tip as feature-a: one co-located group. The placeholder is drawn
    // on top because the name that will order the group is not typed yet.
    let at = app.tree.cursor();
    assert_eq!(app.rows[at + 1].key, "br:feature-a");
    assert_eq!(app.rows[at + 2].key, oid('a').to_string());
    let RowKind::BranchName {
        connector, range, ..
    } = &app.rows[at + 1].kind
    else {
        panic!("expected a branch row");
    };
    assert_eq!(*connector, "│├─", "feature-a hangs under the placeholder");
    assert!(range.is_some(), "drawn as owning `a`");
    assert_eq!(
        diff_text(&app.snapshot, &app.rows[at]),
        "branch not created yet",
        "but nothing to diff until it exists"
    );

    // Action and quit keys type into the field instead of firing.
    for c in "qb".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    let Some(Action::NewBranch { name, target }) =
        app.handle_name_key(KeyCode::Enter, KeyModifiers::NONE)
    else {
        panic!("expected a branch action");
    };
    assert_eq!(name, "qb");
    assert_eq!(target, Some(oid('a').to_string()));
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.next_cursor.as_deref(), Some("br:qb"));
    // The row keeps the typed name while the command runs.
    let RowKind::BranchName { name, .. } = &app.rows[at].kind else {
        panic!("expected a branch row");
    };
    assert_eq!(name, "qb");

    // A failed create has no `qb` row to land on and the fake row is gone
    // too: go back to where `b` was pressed.
    app.finish_action(Err(anyhow::anyhow!("boom")));
    assert_eq!(
        app.next_cursor.as_deref(),
        Some(oid('a').to_string().as_str())
    );
    assert_eq!(app.fallback_cursor, None);
}

/// feature-a owns `a` and `b`; branching at `b` splits it into a stack.
#[test]
fn new_branch_inside_a_branch_splits_it_into_a_stack() {
    let mut info = make_info();
    info.commits = vec![
        commit('a', 'b', "Add parser"),
        commit('b', '9', "Add lexer"),
    ];
    let theme = make_theme();
    let mut app = make_app(snapshot_of(info), &theme);
    move_cursor_to(&mut app, &oid('b').to_string());

    press(&mut app, KeyCode::Char('b'));
    let at = app.tree.cursor();
    assert_eq!(cursor_key(&app), pending_key());
    assert_eq!(app.rows[at + 1].key, oid('b').to_string());
    assert!(
        matches!(app.rows[at - 1].kind, RowKind::Spacer("││")),
        "stacked under feature-a"
    );
    assert_eq!(app.rows[at - 2].key, oid('a').to_string());
}

#[test]
fn new_branch_on_a_branch_row_targets_that_branch() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "br:feature-a");

    press(&mut app, KeyCode::Char('b'));
    let Mode::NewBranch { target, tip, .. } = &app.mode else {
        panic!("expected new-branch mode");
    };
    assert_eq!(target.as_deref(), Some("feature-a"));
    assert_eq!(*tip, oid('a'), "a branch target means its tip");
    let at = app.tree.cursor();
    assert_eq!(cursor_key(&app), pending_key());
    assert_eq!(app.rows[at + 1].key, "br:feature-a");

    for c in "fb".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    assert!(
        app.handle_name_key(KeyCode::Enter, KeyModifiers::NONE)
            .is_some()
    );
    // Success follows the new branch; the fallback is not used.
    app.finish_action(Ok(()));
    assert_eq!(app.next_cursor.as_deref(), Some("br:fb"));
    assert_eq!(app.fallback_cursor, None);
}

/// `c` is loose on the integration line; `-t` on it weaves it onto the new
/// branch (Spec 005), so the preview must already draw it there.
#[test]
fn new_branch_on_a_loose_commit_pulls_it_off_the_integration_line() {
    let mut info = make_info();
    info.commits = vec![commit('c', 'a', "Tweak"), commit('a', '9', "Add parser")];
    let theme = make_theme();
    let mut app = make_app(snapshot_of(info), &theme);
    move_cursor_to(&mut app, &oid('c').to_string());
    assert!(
        matches!(
            app.rows[app.tree.cursor()].kind,
            RowKind::Commit {
                dot_color: None,
                ..
            }
        ),
        "loose until `b`"
    );

    press(&mut app, KeyCode::Char('b'));
    let Mode::NewBranch { target, tip, .. } = &app.mode else {
        panic!("expected new-branch mode");
    };
    assert_eq!(target.as_deref(), Some(oid('c').to_string().as_str()));
    assert_eq!(*tip, oid('c'));

    let at = app.tree.cursor();
    assert_eq!(cursor_key(&app), pending_key());
    let RowKind::BranchName { range, .. } = &app.rows[at].kind else {
        panic!("expected a branch row");
    };
    assert_eq!(
        range.clone(),
        Some((oid('a').to_string(), oid('c').to_string())),
        "owning `c` alone: feature-a's tip stops the walk"
    );
    assert_eq!(app.rows[at + 1].key, oid('c').to_string());
    assert!(
        matches!(
            app.rows[at + 1].kind,
            RowKind::Commit {
                dot_color: Some(_),
                ..
            }
        ),
        "drawn on the branch, not the integration line"
    );
    let next = app.rows[at + 2..]
        .iter()
        .find(|r| r.focusable)
        .expect("a row after the commit");
    assert_eq!(next.key, "br:feature-a", "stacked on feature-a");
}

#[test]
fn new_branch_off_a_commit_or_branch_lands_at_the_base() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, "wf:a.rs");

    press(&mut app, KeyCode::Char('b'));
    let Mode::NewBranch { target, tip, .. } = &app.mode else {
        panic!("expected new-branch mode");
    };
    assert_eq!(*target, None, "no -t: the branch is created at the base");
    assert_eq!(*tip, oid('9'));
    let at = app.tree.cursor();
    assert_eq!(cursor_key(&app), pending_key());
    let RowKind::BranchName { range, .. } = &app.rows[at].kind else {
        panic!("expected a branch row");
    };
    assert!(range.is_none(), "an empty branch owns nothing");
    let next = app.rows[at + 1..]
        .iter()
        .find(|r| r.focusable)
        .expect("a row after the new branch");
    assert_eq!(
        next.key, "br:feature-a",
        "an empty branch is drawn above the branches that own commits"
    );
    let before = app.rows[..at]
        .iter()
        .rev()
        .find(|r| r.focusable)
        .expect("a row before the new branch");
    assert_eq!(before.key, "wf:b.rs", "right under the local changes");
}

#[test]
fn new_branch_is_cancelled_by_escape_and_by_an_empty_name() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, &oid('a').to_string());
    let rows_before = app.rows.len();

    press(&mut app, KeyCode::Char('b'));
    assert!(
        app.handle_name_key(KeyCode::Esc, KeyModifiers::NONE)
            .is_none()
    );
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.rows.len(), rows_before, "the fake branch is gone");
    assert_eq!(cursor_key(&app), oid('a').to_string());

    // Enter with nothing typed stops the creation, like Esc.
    press(&mut app, KeyCode::Char('b'));
    assert!(
        app.handle_name_key(KeyCode::Enter, KeyModifiers::NONE)
            .is_none()
    );
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.rows.len(), rows_before);

    // A chord is not text: Ctrl-C cancels instead of typing a `c`.
    press(&mut app, KeyCode::Char('b'));
    app.handle_key(PaneId::Left, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(app.rows.len(), rows_before);
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
        app.command_line(&Action::Commit {
            source: CommitSource::Files(vec![ids.get_unstaged().to_string()]),
            dest: CommitDest::Integration,
        }),
        format!("loom commit -i {}", ids.get_unstaged())
    );
    assert_eq!(
        app.command_line(&Action::Commit {
            source: CommitSource::Files(vec![file.clone()]),
            dest: CommitDest::Branch("feature-a".to_string()),
        }),
        format!("loom commit -b {} {}", ids.get_branch("feature-a"), file)
    );
    assert_eq!(
        app.command_line(&Action::NewBranch {
            name: "feature-b".to_string(),
            target: Some("feature-a".to_string()),
        }),
        format!(
            "loom branch new feature-b -t {}",
            ids.get_branch("feature-a")
        )
    );
    assert_eq!(
        app.command_line(&Action::NewBranch {
            name: "feature-b".to_string(),
            target: None,
        }),
        "loom branch new feature-b"
    );
    assert_eq!(
        app.command_line(&Action::Drop {
            targets: vec![oid('a').to_string()],
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

    // Several ✓ lines (`drop a b`): the last one, with a count of the rest.
    for text in ["Restored `a`", "Deleted `b`"] {
        app.handle_request(Request::Message {
            level: Level::Success,
            text: text.to_string(),
        });
    }
    assert!(app.finish_action(Ok(())));
    assert_eq!(
        app.notice.as_deref(),
        Some("✓ Deleted `b` (+2 more, L: log)")
    );

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
fn new_branch_draws_the_field_on_the_fake_branch_row() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, &oid('a').to_string());
    press(&mut app, KeyCode::Char('b'));
    for c in "fix".chars() {
        press(&mut app, KeyCode::Char(c));
    }

    let mut shell = Shell::new(app);
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| shell.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let lines: Vec<String> = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    let at = lines
        .iter()
        // The trailing space of the field carries the cursor cell.
        .position(|line| line.contains("│╭─ [CREATE BRANCH] [fix ]"))
        .expect("no fake branch row");
    assert!(lines[at + 1].contains("│├─ fa [feature-a]"), "{lines:#?}");
    assert!(lines[at + 2].contains("Add parser"), "{lines:#?}");
    assert!(lines.iter().any(|line| line.contains(" New branch ")));
    assert!(lines.iter().any(|line| line.contains("Enter to create")));
}

#[test]
fn commit_draws_a_placeholder_row_at_its_destination() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    move_cursor_to(&mut app, LOCAL_CHANGES_KEY);
    press(&mut app, KeyCode::Char('c'));
    press(&mut app, KeyCode::Down);

    let mut shell = Shell::new(app);
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| shell.render(f)).unwrap();
    let buffer = terminal.backend().buffer();
    let lines: Vec<String> = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    let at = lines
        .iter()
        .position(|line| line.contains("[CREATE COMMIT] new commit (2 files)"))
        .expect("no placeholder commit row");
    assert!(lines[at - 1].contains("[feature-a]"), "{lines:#?}");
    assert!(lines[at + 1].contains("Add parser"), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains(" Commit to [feature-a] ")));
    assert!(lines.iter().any(|l| l.contains("Enter to commit")));
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
            " Navigate: ↑/↓ | Fold/unfold: ←/→ | Select: space | Commit: c/C | Fold: f \
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
        ..make_snapshot()
    }
}

fn row(kind: RowKind, key: &str) -> Row {
    Row {
        kind,
        sid: String::new(),
        target: None,
        key: key.to_string(),
        focusable: true,
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

/// What the pane really shows while a commit is being placed: the changes,
/// not the "not created yet" text `diff_text` falls back to once the mode is
/// over and the row is briefly still on screen.
#[test]
fn the_placeholder_pane_shows_the_preview_while_placing() {
    let repo = crate::core::test_helpers::TestRepo::new();
    repo.write_file("file.txt", "original content\n");
    repo.stage_files(&["file.txt"]);
    repo.commit_staged("Add file");
    repo.write_file("file.txt", "changed\n");

    let mut info = make_info();
    info.working_changes = vec![file("file.txt", ' ', 'M')];
    let snapshot = Snapshot {
        workdir: repo.workdir(),
        git_dir: repo.repo.path().to_path_buf(),
        ..snapshot_of(info)
    };
    let theme = make_theme();
    let mut app = make_app(snapshot, &theme);
    move_cursor_to(&mut app, "wf:file.txt");

    press(&mut app, KeyCode::Char('c'));
    let text: String = app.diff_cache[&pending_commit_key()]
        .iter()
        .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
        .collect();
    assert!(text.contains("+changed"), "got: {text}");
    assert!(!text.contains("not created yet"), "got: {text}");
}

/// A failed tracked diff ends mid-line, so the untracked header that follows
/// must not be glued to it.
#[test]
fn pending_commit_diff_keeps_a_failed_diff_off_the_untracked_header() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("new.txt"), "fresh\n").unwrap();

    let mut info = make_info();
    info.working_changes = vec![file("tracked.rs", ' ', 'M'), file("new.txt", '?', '?')];
    let snapshot = Snapshot {
        workdir: dir.path().to_path_buf(),
        git_dir: dir.path().to_path_buf(),
        ..snapshot_of(info)
    };

    // Not a repository, so the tracked diff fails and returns its message,
    // which is what ends mid-line: without the guard the header is glued on.
    let only_tracked = pending_commit_diff(
        &snapshot,
        &[snapshot.ids.get_file("tracked.rs").to_string()],
    );
    assert!(only_tracked.starts_with("error:"), "got: {only_tracked}");
    assert!(!only_tracked.ends_with('\n'), "got: {only_tracked}");

    let text = pending_commit_diff(&snapshot, &[snapshot.ids.get_unstaged().to_string()]);
    assert!(text.contains("error:"), "got: {text}");
    assert!(text.contains("\nuntracked file: new.txt"), "got: {text}");
}

/// The placeholder commit shows what it will contain, not the row it sits on.
#[test]
fn pending_commit_diff_shows_the_files_it_will_hold() {
    let repo = crate::core::test_helpers::TestRepo::new();
    repo.write_file("file.txt", "original content\n");
    repo.stage_files(&["file.txt"]);
    repo.commit_staged("Add file");
    repo.write_file("file.txt", "changed\n");
    repo.write_file("new.txt", "fresh\n");

    repo.write_file("other.txt", "second\n");
    repo.stage_files(&["other.txt"]);
    repo.commit_staged("Add other");
    repo.write_file("other.txt", "second changed\n");

    let mut info = make_info();
    info.working_changes = vec![
        file("file.txt", ' ', 'M'),
        file("other.txt", ' ', 'M'),
        file("new.txt", '?', '?'),
    ];
    let snapshot = Snapshot {
        workdir: repo.workdir(),
        git_dir: repo.repo.path().to_path_buf(),
        ..snapshot_of(info)
    };
    let ids = &snapshot.ids;

    let one = pending_commit_diff(&snapshot, &[ids.get_file("file.txt").to_string()]);
    assert!(one.contains("+changed"));
    assert!(!one.contains("fresh"), "only the file it holds");
    assert!(!one.contains("second changed"), "only the file it holds");

    // Several tracked files go out as one `git diff`, and both must be in it.
    let two = pending_commit_diff(
        &snapshot,
        &[
            ids.get_file("file.txt").to_string(),
            ids.get_file("other.txt").to_string(),
        ],
    );
    assert!(two.contains("+changed"), "got: {two}");
    assert!(two.contains("+second changed"), "got: {two}");

    // `zz` commits through `git add -A`: the untracked file goes in, so the
    // preview has to show it next to the tracked change.
    let all = pending_commit_diff(&snapshot, &[ids.get_unstaged().to_string()]);
    assert!(all.contains("+changed"));
    assert!(all.contains("+fresh"), "untracked file missing from zz");

    let untracked = pending_commit_diff(&snapshot, &[ids.get_file("new.txt").to_string()]);
    assert!(untracked.contains("+fresh"), "untracked shown as added");

    // Nothing tracked to diff: the whole-tree diff is empty and only the
    // untracked file is left, which must not read as an empty pane.
    repo.write_file("file.txt", "original content\n");
    repo.write_file("other.txt", "second\n");
    let all = pending_commit_diff(&snapshot, &[ids.get_unstaged().to_string()]);
    assert_eq!(all.trim(), "untracked file: new.txt\n+fresh");

    // Nothing at all: never a blank pane.
    let mut info = make_info();
    info.working_changes.clear();
    let empty = Snapshot {
        workdir: repo.workdir(),
        git_dir: repo.repo.path().to_path_buf(),
        ..snapshot_of(info)
    };
    let text = pending_commit_diff(&empty, &[empty.ids.get_unstaged().to_string()]);
    assert_eq!(text, "no changes");
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

/// Shrinking the context (`-`) removes the row the cursor sits on; it must
/// land on the neighbour, not back at the top of the tree.
#[test]
fn cursor_lands_next_to_a_row_that_disappeared() {
    let theme = make_theme();
    let mut info = make_info();
    info.context_commits = vec![context_commit("1111111"), context_commit("2222222")];
    let mut app = make_app(snapshot_of(info), &theme);
    move_cursor_to(&mut app, "ctx:2222222");

    let mut shallower = make_info();
    shallower.context_commits = vec![context_commit("1111111")];
    app.apply_snapshot(snapshot_of(shallower));

    assert_eq!(cursor_key(&app), "ctx:1111111");
}

#[test]
fn shrinking_the_context_below_one_does_nothing() {
    let theme = make_theme();
    let mut app = make_app(make_snapshot(), &theme);
    let before = cursor_key(&app);

    app.change_context(-1);

    assert_eq!(app.context, 1);
    assert!(app.popup.is_none(), "no reload, so no failure popup");
    assert_eq!(cursor_key(&app), before);
}

/// Asking for more context than history holds must leave the depth on what
/// the tree shows, or `-` would take as many presses to change anything.
#[test]
fn growing_the_context_clamps_to_the_history_that_exists() {
    let test_repo = crate::core::test_helpers::TestRepo::new_with_remote();
    let theme = make_theme();

    test_repo.in_dir(|| {
        let mut app = make_app(load_snapshot(1).unwrap(), &theme);

        app.change_context(1);

        assert!(app.snapshot.info.context_commits.is_empty());
        assert_eq!(app.context, 1);
    });
}

fn context_messages(app: &App) -> Vec<String> {
    app.rows
        .iter()
        .filter_map(|row| match &row.kind {
            RowKind::Context { message, .. } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn context_keys_add_and_remove_one_commit_before_the_base() {
    let test_repo = crate::core::test_helpers::TestRepo::new_with_remote();
    let tip = test_repo.add_remote_commits(&["Older", "Newer"]);
    test_repo.fetch_remote();
    test_repo.reset_hard(tip);
    let theme = make_theme();

    test_repo.in_dir(|| {
        let mut app = make_app(load_snapshot(1).unwrap(), &theme);
        let press = |app: &mut App, key: char| {
            app.handle_key(PaneId::Left, KeyCode::Char(key), KeyModifiers::NONE);
        };
        assert!(context_messages(&app).is_empty());

        press(&mut app, '+');
        assert_eq!(app.context, 2);
        assert_eq!(context_messages(&app), ["Older"]);

        press(&mut app, '=');
        assert_eq!(app.context, 3);
        assert_eq!(context_messages(&app), ["Older", "Initial"]);

        press(&mut app, '-');
        assert_eq!(app.context, 2);
        assert_eq!(context_messages(&app), ["Older"]);
    });
}
