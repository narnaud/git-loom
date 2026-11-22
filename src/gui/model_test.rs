use super::*;
use crate::core::repo::{FileChange, RepoInfo};
use crate::core::test_helpers::{base_info, branch};
use crate::core::ui::Level;
use crate::gui::state::tests::{info, row, select, state, state_in, state_of};
use crate::session::paused::test_support::repo_with_a_paused_drop;
use crate::session::snapshot::CommitDest;

#[test]
fn the_local_changes_are_headed_by_how_many_files_there_are() {
    let mut state = state();

    let header = local_changes(&state);

    assert_eq!(header.row, row(&state, "local changes") as i32);
    assert_eq!((header.hint.as_str(), header.selected), ("3 files", false));
    select(&mut state, "local changes", &[]);
    assert!(local_changes(&state).selected);
    let mut one = info();
    one.working_changes.truncate(1);
    assert_eq!(local_changes(&state_of(one)).hint, "1 file");
    let mut none = info();
    none.working_changes.clear();
    assert_eq!(local_changes(&state_of(none)).hint, "no changes");
}

#[test]
fn a_local_change_reads_its_status_letters_or_its_mark_then_its_path() {
    let mut info = info();
    info.working_changes.push(FileChange {
        path: "docs/clash.md".to_string(),
        index: '!',
        worktree: '!',
    });
    let mut state = state_in(info, "docs");
    select(&mut state, "b.rs", &[]);

    let changes = changes(&state);

    let reading = changes.iter().map(|change| {
        let letters = format!("{}{}", change.staged, change.unstaged);
        (change.kind, letters, change.path.as_str(), change.selected)
    });
    assert_eq!(
        reading.collect::<Vec<_>>(),
        [
            (ChangeKind::Conflicted, String::new(), "clash.md", false),
            (ChangeKind::Tracked, "M ".to_string(), "../a.rs", false),
            (ChangeKind::Tracked, " M".to_string(), "../b.rs", true),
            (ChangeKind::Untracked, String::new(), "../new.rs", false),
        ]
    );
    assert_eq!(changes[2].row, row(&state, "b.rs") as i32);
    // The window scrolls the pane by that position.
    assert_eq!(state.cursor_change(), Some(2));
}

#[test]
fn nothing_stands_for_the_local_changes_while_an_operation_is_paused() {
    let (repo, _) = repo_with_a_paused_drop();

    let state = repo.in_dir(State::load).unwrap();

    let nothing = LocalChanges {
        row: -1,
        ..Default::default()
    };
    assert_eq!(local_changes(&state), nothing);
    assert!(changes(&state).is_empty());
}

fn find<'a>(pieces: &'a [Piece], reading: &str) -> &'a Piece {
    let found = pieces
        .iter()
        .find(|p| p.title == reading || p.text == reading);
    found.unwrap_or_else(|| panic!("no piece reads {reading}"))
}

#[test]
fn a_card_reads_the_short_id_the_subject_and_the_file_count_of_its_commit() {
    let state = state();
    let pieces = pieces(&state);

    let card = find(&pieces, "Second on b");

    assert_eq!(card.kind, Kind::Card);
    let sid = state.rows().iter().find_map(|row| match &row.kind {
        RowKind::Commit { message, .. } if message == "Second on b" => Some(row.sid.clone()),
        _ => None,
    });
    assert_eq!(Some(card.sid.to_string()), sid);
    assert!(!card.sid.is_empty());
    assert_eq!(card.hint, "2 files");
    assert_eq!(find(&pieces, "First on b").hint, "1 file");
}

#[test]
fn a_tag_reads_the_name_of_its_branch_and_how_it_stands_against_its_remote() {
    let pieces = pieces(&state());

    let pushed = find(&pieces, "feature-a");
    let unpushed = find(&pieces, "feature-b");

    assert_eq!((pushed.kind, pushed.remote), (Kind::Tag, Remote::Synced));
    assert_eq!(unpushed.remote, Remote::None);
}

#[test]
fn a_tag_tells_a_branch_ahead_of_its_remote_from_one_whose_remote_is_gone() {
    let mut info: RepoInfo = base_info();
    info.branches = vec![
        branch("ahead", 0xAA, Some(RemoteStatus::Different)),
        branch("orphan", 0xAA, Some(RemoteStatus::Gone)),
    ];
    let pieces = pieces(&state_of(info));

    assert_eq!(find(&pieces, "ahead").remote, Remote::Different);
    assert_eq!(find(&pieces, "orphan").remote, Remote::Gone);
}

#[test]
fn a_tag_says_how_many_commits_its_branch_owns_and_whether_they_show() {
    let mut state = state();
    state.row_toggled(row(&state, "feature-a"));
    let pieces = pieces(&state);

    let open = find(&pieces, "feature-b");
    let closed = find(&pieces, "feature-a");

    assert_eq!(open.hint, "2 commits");
    assert!(open.expandable && open.expanded);
    assert_eq!(closed.hint, "1 commit");
    assert!(closed.expandable && !closed.expanded);
}

#[test]
fn the_tag_of_a_branch_owning_no_commit_says_so_and_does_not_open() {
    let mut stale: RepoInfo = base_info();
    stale.branches = vec![branch("stale", 0xAA, None)];
    let pieces = pieces(&state_of(stale));

    let tag = find(&pieces, "stale");

    assert_eq!(tag.hint, "no commits");
    assert!(!tag.expandable);
}

/// The window draws a card's own line `line` high, whatever the card holds
/// under it.
#[test]
fn an_open_card_keeps_the_line_it_had_closed() {
    let mut state = state();
    let closed = find(&pieces(&state), "Second on b").clone();
    assert!(closed.expandable && !closed.expanded);
    assert_eq!(closed.line, closed.h);

    state.row_toggled(row(&state, "Second on b"));
    let pieces = pieces(&state);
    let open = find(&pieces, "Second on b");

    assert!(open.expanded);
    assert_eq!(open.line, closed.line);
    assert!(open.h > open.line);
}

#[test]
fn a_chip_reads_the_status_and_the_path_of_a_file_of_its_commit() {
    let mut state = state();
    state.row_toggled(row(&state, "Second on b"));
    let pieces = pieces(&state);

    let chip = find(&pieces, "y.rs");

    assert_eq!((chip.kind, chip.status.as_str()), (Kind::Chip, "M"));
    assert!(!chip.expandable);
    assert_eq!(chip.line, chip.h);
}

/// As `loom status` writes paths from where it runs (Spec 001).
#[test]
fn a_chip_reads_its_path_from_the_directory_the_window_started_in() {
    let mut state = state_in(info(), "docs");
    state.row_toggled(row(&state, "On a"));

    let pieces = pieces(&state);

    assert_eq!(find(&pieces, "../z.rs").kind, Kind::Chip);
}

#[test]
fn a_piece_names_the_row_it_stands_for_and_the_tip_none() {
    let state = state();
    let pieces = pieces(&state);

    let card = find(&pieces, "On a");

    assert_eq!(card.row, row(&state, "On a") as i32);
    assert_eq!(find(&pieces, "integration").row, -1);
}

#[test]
fn the_tip_reads_the_integration_branch() {
    let pieces = pieces(&state());

    let tip = find(&pieces, "integration");

    assert_eq!(tip.kind, Kind::Tip);
}

#[test]
fn the_base_reads_the_upstream_and_the_commit_the_line_stands_on() {
    let pieces = pieces(&state());

    let base = find(&pieces, "origin/main");

    assert_eq!(base.kind, Kind::Base);
    assert_eq!(
        (base.sid.as_str(), base.text.as_str()),
        ("aaa0000", "Initial commit")
    );
    assert_eq!(base.hint, "", "nothing is new upstream");
}

#[test]
fn the_base_says_how_many_commits_are_new_upstream() {
    let behind = |commits_ahead| {
        let mut info: RepoInfo = base_info();
        info.upstream.commits_ahead = commits_ahead;
        pieces(&state_of(info))
    };

    assert_eq!(find(&behind(1), "origin/main").hint, "1 new commit");
    assert_eq!(find(&behind(3), "origin/main").hint, "3 new commits");
}

#[test]
fn a_piece_is_where_the_board_placed_it() {
    let state = state();

    let pieces = pieces(&state);

    let placed = &state.board().pieces;
    assert_eq!(pieces.len(), placed.len());
    for (piece, placed) in pieces.iter().zip(placed) {
        let rect = placed.rect;
        assert_eq!(
            (piece.x, piece.y, piece.w, piece.h),
            (rect.x, rect.y, rect.w, rect.h)
        );
    }
}

#[test]
fn a_line_carries_its_strand_its_color_and_where_it_is_in_its_stack() {
    let state = state_of(info());

    let lines = lines(state.board());

    let trunk = &lines[0];
    assert_eq!((trunk.strand, trunk.color), (Strand::Thread, -1));
    let frames: Vec<&BoardLine> = lines.iter().filter(|l| l.strand == Strand::Frame).collect();
    assert_eq!(frames.len(), 2);
    assert_eq!((frames[0].color, frames[1].color), (0, 1));
    assert!(
        frames.iter().all(|f| f.first && f.last),
        "neither is stacked"
    );
    let rings = lines.iter().filter(|l| l.strand == Strand::Ring).count();
    assert_eq!(rings, 3, "the tip and the two branch names");
}

#[test]
fn a_model_of_as_many_rows_is_updated_in_place() {
    let model: ModelRc<i32> = Rc::new(VecModel::from(vec![1, 2, 3])).into();
    let mut replaced = false;

    update_model(&model, vec![1, 5, 3], |_| replaced = true);

    assert!(!replaced);
    assert_eq!(model.iter().collect::<Vec<_>>(), [1, 5, 3]);
}

#[test]
fn a_model_of_another_length_is_replaced() {
    let model: ModelRc<i32> = Rc::new(VecModel::from(vec![1, 2, 3])).into();
    let mut installed = None;

    update_model(&model, vec![7, 8], |new| installed = Some(new));

    let installed = installed.expect("a new model");
    assert_eq!(installed.iter().collect::<Vec<_>>(), [7, 8]);
    assert_eq!(
        model.iter().collect::<Vec<_>>(),
        [1, 2, 3],
        "left as it was"
    );
}

#[test]
fn a_piece_says_whether_its_row_is_selected() {
    let mut state = state();
    select(&mut state, "feature-a", &["feature-b"]);

    let pieces = pieces(&state);

    let selected: Vec<&str> = pieces
        .iter()
        .filter(|piece| piece.selected)
        .map(|piece| piece.title.as_str())
        .collect();
    assert_eq!(selected, ["feature-b", "feature-a"]);
}

#[test]
fn diff_text_becomes_one_row_a_line_each_of_its_kind_with_tabs_as_spaces() {
    let rows = diff_rows(
        "diff --git a/x b/x\nindex 1..2\n@@ -1 +1 @@\n-\told\n+\tnew\n same",
        2,
    );

    let kinds: Vec<TextKind> = rows.iter().map(|row| row.kind).collect();
    assert_eq!(
        kinds,
        [
            TextKind::FileHeader,
            TextKind::Index,
            TextKind::HunkHeader,
            TextKind::Removed,
            TextKind::Added,
            TextKind::Context,
        ]
    );
    assert_eq!(rows[3].text, "-  old");
}

/// The message of a commit is indented as context lines are.
#[test]
fn what_git_show_says_before_the_first_file_is_not_read_as_a_diff() {
    let rows = diff_rows(
        "commit abc\nAuthor: A\n\n    + not an added line\n\ndiff --git a/x b/x\n+added",
        4,
    );

    let kinds: Vec<TextKind> = rows.iter().map(|row| row.kind).collect();
    assert_eq!(
        kinds,
        [
            TextKind::CommitLine,
            TextKind::CommitHeader,
            TextKind::CommitHeader,
            TextKind::CommitHeader,
            TextKind::CommitHeader,
            TextKind::FileHeader,
            TextKind::Added,
        ]
    );
}

#[test]
fn no_diff_is_no_row() {
    assert!(diff_rows("", 4).is_empty());
}

#[test]
fn the_longest_line_counts_its_characters_not_its_bytes() {
    let rows = diff_rows("+short\n+\tlonger, é\n same", 4);

    assert_eq!(columns(&rows), 14);
    assert_eq!(columns(&[]), 0);
}

#[test]
fn a_message_drops_the_backticks_the_cli_highlights_with() {
    assert_eq!(
        plain("Dropped `a.rs` from `feature`"),
        "Dropped a.rs from feature"
    );
    assert_eq!(plain("an `odd one"), "an `odd one", "one alone is kept");
    assert_eq!(plain(""), "");
}

#[test]
fn the_log_reads_each_command_then_what_it_said() {
    let log = [
        LogEntry {
            command: "loom drop a".to_string(),
            lines: vec![
                (Level::Plain, "git said".to_string()),
                (
                    Level::Success,
                    "Dropped `a`\nIts commits went too".to_string(),
                ),
            ],
        },
        LogEntry {
            command: "loom push b".to_string(),
            lines: vec![
                (Level::Warn, "Nothing new".to_string()),
                (Level::Error, "Push refused".to_string()),
            ],
        },
    ];

    let rows = log_rows(&log);

    let read: Vec<(&str, TextKind)> = rows
        .iter()
        .map(|row| (row.text.as_str(), row.kind))
        .collect();
    assert_eq!(
        read,
        [
            ("$ loom drop a", TextKind::Command),
            ("git said", TextKind::Context),
            ("• Dropped a", TextKind::Success),
            ("  › Its commits went too", TextKind::Index),
            ("", TextKind::Context),
            ("$ loom push b", TextKind::Command),
            ("! Nothing new", TextKind::Warning),
            ("! Push refused", TextKind::Failure),
        ]
    );
    assert_eq!(log_rows(&[])[0].text, "No command has run yet.");
}

#[test]
fn the_keys_step_over_the_lines_and_greyed_entries_of_a_menu() {
    let entry = |id: i32, enabled: bool| MenuEntry {
        id,
        enabled,
        ..Default::default()
    };
    let menu = [
        entry(0, true),
        entry(-1, false),
        entry(1, false),
        entry(2, true),
    ];

    assert_eq!(menu_step(&menu, -1, true), 0);
    assert_eq!(menu_step(&menu, 0, true), 3);
    assert_eq!(menu_step(&menu, 3, true), 3);
    assert_eq!(menu_step(&menu, 3, false), 0);
    assert_eq!(menu_step(&menu, -1, false), 3);
    assert_eq!(menu_step(&[entry(0, false)], -1, true), -1);
}

#[test]
fn a_menu_draws_a_line_between_its_groups() {
    let mut state = state();
    select(&mut state, "local changes", &[]);

    let menu = menu(&state.menu());

    let read: Vec<(&str, bool)> = menu
        .iter()
        .map(|entry| (entry.title.as_str(), entry.id < 0))
        .collect();
    assert_eq!(
        read,
        [
            ("Commit…", false),
            ("", true),
            ("Absorb", false),
            ("", true),
            ("Drop all local changes", false)
        ]
    );
}

#[test]
fn a_dialog_reads_its_form_without_backticks_but_for_the_command() {
    let form = Form::notice("loom drop `a`", "Could not drop `a`\nRun `loom trace`");

    let dialog = dialog(&form, "integration");

    assert_eq!(dialog.title, "loom drop `a`");
    assert_eq!(dialog.body, "Could not drop a");
    assert_eq!(dialog.detail, "Run loom trace");
    assert_eq!(
        (dialog.confirm.as_str(), dialog.cancellable),
        ("Close", false)
    );
}

#[test]
fn a_commit_dialog_names_where_a_commit_can_go() {
    let mut form = Form::complete("Commit 1 file".to_string(), Input::Text, "Commit");
    form.dests = vec![
        CommitDest::Integration,
        CommitDest::Branch("feature-b".to_string()),
        CommitDest::Branch("feature-a".to_string()),
    ];

    let dialog = dialog(&form, "integration");

    assert_eq!(dialog.input, DialogInput::Text);
    let destinations: Vec<String> = dialog.destinations.iter().map(|d| d.to_string()).collect();
    assert_eq!(
        destinations,
        ["integration (loose commit)", "feature-b", "feature-a"]
    );
}
