use std::path::PathBuf;

use super::*;
use crate::core::repo::{CommitInfo, FileChange, RemoteStatus, RepoInfo};
use crate::core::shortid::IdAllocator;
use crate::core::test_helpers::{TestRepo, base_info, branch, commit};
use crate::gui::board::{LineKind, PieceKind};
use crate::session::paused::test_support::repo_with_a_paused_drop;

fn file(path: &str, index: char, worktree: char) -> FileChange {
    FileChange {
        path: path.to_string(),
        index,
        worktree,
    }
}

/// Two branches off the base, a loose commit, three local changes.
///
/// ```text
/// ╭─ [local changes]   a.rs (staged), b.rs, new.rs (untracked)
/// ● Loose              w.rs
/// │╭─ [feature-b]
/// │● Second on b       x.rs, y.rs
/// │● First on b        x.rs
/// ├╯
/// │╭─ [feature-a] ✓
/// │● On a              z.rs
/// ├╯
/// ● upstream
/// ```
pub(crate) fn info() -> RepoInfo {
    let with_files = |mut c: CommitInfo, files: &[&str]| {
        c.files = files.iter().map(|f| file(f, 'M', ' ')).collect();
        c
    };
    RepoInfo {
        commits: vec![
            with_files(commit(0xB2, "Second on b", Some(0xB1)), &["x.rs", "y.rs"]),
            with_files(commit(0xB1, "First on b", Some(0xAA)), &["x.rs"]),
            with_files(commit(0xA1, "On a", Some(0xAA)), &["z.rs"]),
            with_files(commit(0x01, "Loose", Some(0xAA)), &["w.rs"]),
        ],
        branches: vec![
            branch("feature-b", 0xB2, None),
            branch("feature-a", 0xA1, Some(RemoteStatus::Synced)),
        ],
        working_changes: vec![
            file("a.rs", 'M', ' '),
            file("b.rs", ' ', 'M'),
            file("new.rs", '?', '?'),
        ],
        ..base_info()
    }
}

/// A state over `info`, in a working tree that does not exist.
pub(crate) fn state_of(info: RepoInfo) -> State {
    state_in(info, "")
}

/// [`state_of`], the window started in the directory `cwd_prefix` of the
/// repository.
pub(crate) fn state_in(info: RepoInfo, cwd_prefix: &str) -> State {
    let ids = IdAllocator::new(info.collect_entities());
    let nowhere = PathBuf::from("gui-test-fixture-has-no-workdir");
    let snapshot = Snapshot {
        workdir: nowhere.clone(),
        git_dir: nowhere,
        cwd_prefix: cwd_prefix.to_string(),
        info,
        ids,
    };
    State::new(snapshot, None)
}

pub(crate) fn state() -> State {
    state_of(info())
}

/// Index of the row that reads `text`: a path, a branch name, a message.
pub(crate) fn row(state: &State, text: &str) -> usize {
    let found = state.rows.iter().position(|r| match &r.kind {
        RowKind::CommitFile { path, .. } => path == text,
        RowKind::BranchName { name, .. } => name == text,
        RowKind::Commit { message, .. } => message == text,
        RowKind::Upstream { .. } => text == "upstream",
        RowKind::LocalChanges { .. } => text == "local changes",
        _ => false,
    });
    found.unwrap_or_else(|| panic!("no row reads {text}"))
}

/// Click the piece reading `first`, then `Ctrl` click each of `more`.
pub(crate) fn select(state: &mut State, first: &str, more: &[&str]) {
    state.row_pressed(row(state, first), false, false);
    for text in more {
        state.row_pressed(row(state, text), true, false);
    }
}

/// What the selected rows read, in tree order.
pub(crate) fn selected(state: &State) -> Vec<String> {
    let rows = state.rows.iter().filter(|row| state.is_selected(row));
    let reading = rows.map(|row| match &row.kind {
        RowKind::CommitFile { path, .. } => path.clone(),
        RowKind::BranchName { name, .. } => name.clone(),
        RowKind::Commit { message, .. } => message.clone(),
        RowKind::Upstream { .. } => "upstream".to_string(),
        _ => "not on the board".to_string(),
    });
    reading.collect()
}

/// Whether a row of the tree reads `text`.
fn shows(state: &State, text: &str) -> bool {
    state.rows.iter().any(|r| match &r.kind {
        RowKind::CommitFile { path, .. } => path == text,
        RowKind::Commit { message, .. } => message == text,
        _ => false,
    })
}

#[test]
fn the_window_is_headed_by_the_integration_branch() {
    let repo = TestRepo::new_with_remote();

    let state = repo.in_dir(State::load).unwrap();

    assert_eq!(state.heading(), "integration");
    let workdir = repo.workdir();
    let directory = workdir.file_name().unwrap().to_string_lossy();
    assert_eq!(state.repository(), directory);
}

#[test]
fn the_window_is_headed_by_a_paused_operation() {
    let (repo, _) = repo_with_a_paused_drop();

    let state = repo.in_dir(State::load).unwrap();

    assert!(
        state.heading().starts_with("Paused: loom drop"),
        "{}",
        state.heading()
    );
}

#[test]
fn the_board_places_the_repository_it_opened_on() {
    let repo = TestRepo::new_with_remote();
    repo.commit("Loose on the line", "loose.txt");

    let state = repo.in_dir(State::load).unwrap();

    let cards =
        state
            .board()
            .pieces
            .iter()
            .filter_map(|piece| match &state.rows()[piece.row?].kind {
                RowKind::Commit { message, .. } => Some(message.as_str()),
                _ => None,
            });
    assert_eq!(cards.collect::<Vec<_>>(), ["Loose on the line"]);
}

/// The snapshot of a paused operation is a placeholder: nothing of it is
/// the repository's to show.
#[test]
fn the_board_is_bare_while_an_operation_is_paused() {
    let (repo, _) = repo_with_a_paused_drop();

    let mut state = repo.in_dir(State::load).unwrap();
    state.set_room(800.0);

    assert_eq!(state.board(), &Board::default());
}

#[test]
fn the_board_is_laid_out_again_for_the_room_its_pane_has() {
    let mut state = state();
    let narrow = state.board().width;

    state.set_room(600.0);

    assert_eq!(state.board().width, 600.0);
    assert!(narrow < 600.0, "{narrow}");
}

#[test]
fn a_commit_opens_onto_its_files_and_closes_again() {
    let mut state = state();
    let commit = row(&state, "Second on b");
    assert!(state.rows[commit].expandable && !state.rows[commit].expanded);

    state.row_toggled(commit);

    assert!(state.rows[commit].expanded);
    assert!(shows(&state, "x.rs") && shows(&state, "y.rs"));
    assert!(!shows(&state, "z.rs"), "another commit's");
    let chips = state.board().pieces.iter();
    assert_eq!(chips.filter(|p| p.kind == PieceKind::Chip).count(), 2);

    state.row_toggled(commit);

    assert!(!shows(&state, "y.rs"));
    assert_eq!(state.board(), state_of(info()).board(), "as it opened");
}

#[test]
fn several_commits_can_be_open_at_once() {
    let mut state = state();

    state.row_toggled(row(&state, "Second on b"));
    state.row_toggled(row(&state, "On a"));

    assert!(shows(&state, "y.rs") && shows(&state, "z.rs"));
}

#[test]
fn closing_a_branch_hides_its_commits_and_keeps_their_count() {
    let mut state = state();
    let tag = row(&state, "feature-b");
    assert!(state.rows[tag].expandable && state.rows[tag].expanded);

    state.row_toggled(tag);

    let tag = &state.rows[row(&state, "feature-b")];
    assert!(tag.expandable && !tag.expanded);
    assert!(!shows(&state, "Second on b") && !shows(&state, "First on b"));
    assert!(shows(&state, "On a"), "the other branch keeps its own");
    assert_eq!(state.owned(tag), 2);

    state.row_toggled(row(&state, "feature-b"));

    assert!(shows(&state, "Second on b") && shows(&state, "First on b"));
}

/// What is open is kept by row key, not by where the row is.
#[test]
fn a_commit_is_still_open_once_the_branch_closed_over_it_opens_again() {
    let mut state = state();
    state.row_toggled(row(&state, "First on b"));

    state.row_toggled(row(&state, "feature-b"));
    assert!(!shows(&state, "x.rs"), "hidden with its commit");
    state.row_toggled(row(&state, "feature-b"));

    assert!(shows(&state, "x.rs"));
}

#[test]
fn names_at_the_same_commit_close_and_open_together() {
    let together = RepoInfo {
        commits: vec![commit(0x01, "Shared", Some(0xAA))],
        branches: vec![branch("one", 0x01, None), branch("two", 0x01, None)],
        ..base_info()
    };
    let mut state = state_of(together);

    state.row_toggled(row(&state, "one"));

    assert!(!shows(&state, "Shared"));
    let closed = |name: &str| !state.rows[row(&state, name)].expanded;
    assert!(closed("one") && closed("two"));
    let owned = |name: &str| state.owned(&state.rows[row(&state, name)]);
    assert_eq!((owned("one"), owned("two")), (1, 1));

    state.row_toggled(row(&state, "two"));

    assert!(shows(&state, "Shared"));
}

/// ```text
/// │╭─ [top]      ● T
/// ││
/// │├─ [bottom]   ● B
/// ├╯
/// ```
#[test]
fn each_branch_of_a_stack_closes_on_its_own() {
    let stacked = RepoInfo {
        commits: vec![commit(0x02, "T", Some(0x01)), commit(0x01, "B", Some(0xAA))],
        branches: vec![branch("top", 0x02, None), branch("bottom", 0x01, None)],
        ..base_info()
    };
    let mut state = state_of(stacked);

    state.row_toggled(row(&state, "top"));

    assert!(!shows(&state, "T") && shows(&state, "B"));
    assert!(state.rows[row(&state, "bottom")].expanded);
}

/// The frames of a stack are told apart by the spacer between its branches,
/// which a closed branch keeps: without it the two names would read as
/// branches at the same commit, in one frame.
#[test]
fn a_stack_keeps_a_frame_and_a_color_for_each_branch_closed_or_not() {
    let stacked = RepoInfo {
        commits: vec![commit(0x02, "T", Some(0x01)), commit(0x01, "B", Some(0xAA))],
        branches: vec![branch("top", 0x02, None), branch("bottom", 0x01, None)],
        ..base_info()
    };
    let mut state = state_of(stacked);
    let frames = |state: &State| -> Vec<(LineKind, Option<usize>)> {
        let lines = state.board().lines.iter();
        let frames = lines.filter(|line| matches!(line.kind, LineKind::Frame { .. }));
        frames.map(|frame| (frame.kind, frame.color)).collect()
    };
    let open = frames(&state);
    assert_eq!(
        open,
        [
            (
                LineKind::Frame {
                    first: true,
                    last: false
                },
                Some(0)
            ),
            (
                LineKind::Frame {
                    first: false,
                    last: true
                },
                Some(1)
            ),
        ]
    );
    let merges = |state: &State| {
        let lines = state.board().lines.iter();
        let beads = lines.filter(|line| line.kind == LineKind::Bead && line.color.is_none());
        // The base is a bead of the trunk too.
        beads.count() - 1
    };
    assert_eq!(merges(&state), 1);

    state.row_toggled(row(&state, "top"));
    assert_eq!(frames(&state), open, "the upper one closed");
    assert_eq!(merges(&state), 1);

    state.row_toggled(row(&state, "bottom"));
    assert_eq!(frames(&state), open, "both closed");
    assert_eq!(merges(&state), 1, "still woven in");
    let tags = state.board().pieces.iter();
    assert_eq!(tags.filter(|p| p.kind == PieceKind::Tag).count(), 2);
}

#[test]
fn a_branch_owning_no_commit_has_nothing_to_close_onto() {
    let stale = RepoInfo {
        branches: vec![branch("stale", 0xAA, None)],
        ..base_info()
    };
    let mut state = state_of(stale);
    let tag = row(&state, "stale");

    state.row_toggled(tag);

    assert!(!state.rows[tag].expandable);
    assert_eq!(state.owned(&state.rows[tag]), 0);
    assert!(state.collapsed.is_empty());
}

#[test]
fn what_does_not_open_ignores_being_toggled() {
    let mut state = state();
    let rows = state.rows.len();

    state.row_toggled(row(&state, "upstream"));
    state.row_toggled(row(&state, "local changes"));
    state.row_toggled(rows);

    assert_eq!(state.rows.len(), rows);
    assert!(state.expanded.is_empty() && state.collapsed.is_empty());
}

#[test]
fn nothing_is_selected_when_the_window_opens() {
    let state = state();

    assert!(selected(&state).is_empty());
    assert_eq!(state.cursor_rect(), None);
}

#[test]
fn a_click_selects_one_piece_and_ctrl_click_adds_pieces_of_its_class() {
    let mut state = state();

    select(&mut state, "First on b", &["On a"]);
    assert_eq!(selected(&state), ["First on b", "On a"]);

    select(&mut state, "Second on b", &[]);
    assert_eq!(
        selected(&state),
        ["Second on b"],
        "a plain click starts over"
    );
}

#[test]
fn ctrl_click_takes_a_selected_piece_out_again() {
    let mut state = state();

    select(&mut state, "First on b", &["On a", "First on b"]);

    assert_eq!(selected(&state), ["On a"]);
}

#[test]
fn ctrl_click_on_a_piece_of_another_class_leaves_the_selection_as_it_is() {
    let mut state = state();

    select(&mut state, "First on b", &["feature-a", "upstream"]);

    assert_eq!(selected(&state), ["First on b"]);
}

#[test]
fn shift_click_selects_up_to_the_piece_keeping_to_the_class_of_the_first() {
    let mut state = state();
    select(&mut state, "Loose", &[]);

    state.row_pressed(row(&state, "On a"), false, true);
    assert_eq!(
        selected(&state),
        ["Loose", "Second on b", "First on b", "On a"],
        "the names between are left out"
    );

    state.row_pressed(row(&state, "Second on b"), false, true);
    assert_eq!(
        selected(&state),
        ["Loose", "Second on b"],
        "still from the first one"
    );
}

#[test]
fn shift_click_with_nothing_selected_before_selects_that_piece_alone() {
    let mut state = state();

    state.row_pressed(row(&state, "On a"), false, true);

    assert_eq!(selected(&state), ["On a"]);
}

#[test]
fn shift_click_with_no_cursor_or_from_the_base_puts_the_cursor_on_the_piece() {
    let mut state = state();

    state.row_pressed(row(&state, "First on b"), false, true);
    state.move_cursor(true);
    assert_eq!(selected(&state), ["feature-a"], "with no cursor");

    select(&mut state, "upstream", &[]);
    state.row_pressed(row(&state, "Second on b"), false, true);
    state.move_cursor(true);
    assert_eq!(selected(&state), ["First on b"], "from the base");
}

#[test]
fn shift_click_across_a_closed_branch_leaves_its_hidden_commits_out() {
    let mut state = state();
    state.row_toggled(row(&state, "feature-b"));
    select(&mut state, "Loose", &[]);

    state.row_pressed(row(&state, "On a"), false, true);

    assert_eq!(selected(&state), ["Loose", "On a"]);
}

#[test]
fn shift_click_upwards_selects_the_same_range() {
    let mut state = state();
    select(&mut state, "On a", &[]);

    state.row_pressed(row(&state, "First on b"), false, true);

    assert_eq!(selected(&state), ["First on b", "On a"]);
}

#[test]
fn the_upstream_is_selected_alone() {
    let mut state = state();

    select(&mut state, "upstream", &["On a"]);
    assert_eq!(selected(&state), ["upstream"]);

    state.row_pressed(row(&state, "First on b"), false, true);
    assert_eq!(selected(&state), ["First on b"]);
}

#[test]
fn a_press_on_a_row_the_board_has_no_piece_for_selects_nothing() {
    let mut state = state();
    select(&mut state, "On a", &[]);

    state.row_pressed(row(&state, "local changes"), false, false);
    state.row_pressed(state.rows.len(), false, false);

    assert_eq!(selected(&state), ["On a"]);
}

#[test]
fn the_arrow_keys_step_through_the_pieces_of_the_board_and_stop_at_its_ends() {
    let mut state = state();
    let mut walk = Vec::new();

    for _ in 0..8 {
        state.move_cursor(true);
        walk.extend(selected(&state));
    }

    assert_eq!(
        walk,
        [
            "Loose",
            "feature-b",
            "Second on b",
            "First on b",
            "feature-a",
            "On a",
            "upstream",
            "upstream",
        ]
    );
    for _ in 0..8 {
        state.move_cursor(false);
    }
    assert_eq!(selected(&state), ["Loose"]);
}

#[test]
fn with_nothing_selected_the_up_arrow_starts_from_the_base() {
    let mut state = state();

    state.move_cursor(false);

    assert_eq!(selected(&state), ["upstream"]);
}

#[test]
fn an_arrow_key_narrows_a_selection_to_the_piece_it_moves_to() {
    let mut state = state();
    select(&mut state, "Second on b", &["On a"]);

    state.move_cursor(false);

    assert_eq!(selected(&state), ["feature-a"], "from the last one picked");
}

#[test]
fn right_opens_and_left_closes_what_the_selection_was_last_put_on() {
    let mut state = state();
    select(&mut state, "Second on b", &[]);

    state.expand_cursor();
    assert!(shows(&state, "y.rs"));
    state.expand_cursor();
    assert!(shows(&state, "y.rs"), "open already");

    state.collapse_cursor();
    assert!(!shows(&state, "y.rs"));
    assert_eq!(selected(&state), ["Second on b"]);

    select(&mut state, "feature-a", &[]);
    state.collapse_cursor();
    assert!(!shows(&state, "On a"));
    state.expand_cursor();
    assert!(shows(&state, "On a"));
}

#[test]
fn left_on_a_file_goes_up_to_its_commit() {
    let mut state = state();
    state.row_toggled(row(&state, "Second on b"));
    select(&mut state, "y.rs", &[]);

    state.collapse_cursor();

    assert_eq!(selected(&state), ["Second on b"]);
    assert!(shows(&state, "y.rs"), "which stays open");
}

#[test]
fn what_is_closed_over_selected_rows_takes_them_out_of_the_selection() {
    let mut state = state();
    select(&mut state, "First on b", &["On a"]);

    state.row_toggled(row(&state, "feature-b"));

    assert_eq!(selected(&state), ["On a"]);
    state.row_toggled(row(&state, "feature-b"));
    assert_eq!(selected(&state), ["On a"], "and does not give them back");
}

#[test]
fn escape_selects_nothing_and_remembers_where_the_selection_was() {
    let mut state = state();
    select(&mut state, "feature-a", &[]);

    state.clear_selection();
    assert!(selected(&state).is_empty());
    assert_eq!(state.cursor_rect(), None, "nothing to keep in view");

    state.move_cursor(true);
    assert_eq!(selected(&state), ["On a"]);
}

#[test]
fn the_window_is_told_where_the_selection_was_last_put() {
    let mut state = state();
    select(&mut state, "Loose", &["On a"]);

    let rect = state.cursor_rect().unwrap();

    let on_a = row(&state, "On a");
    let piece = state.board().pieces.iter().find(|p| p.row == Some(on_a));
    assert_eq!(rect, piece.unwrap().rect);
}

/// As in the TUI: the arrow keys go on from where they were.
#[test]
fn the_arrow_keys_go_on_from_the_piece_that_hid_the_one_they_were_on() {
    let mut state = state();
    select(&mut state, "First on b", &[]);

    state.row_toggled(row(&state, "feature-b"));
    state.move_cursor(true);

    assert_eq!(selected(&state), ["feature-a"]);
}

#[test]
fn closing_a_card_puts_the_arrow_keys_on_it_from_its_file() {
    let mut state = state();
    state.row_toggled(row(&state, "Second on b"));
    select(&mut state, "y.rs", &[]);

    state.row_toggled(row(&state, "Second on b"));
    state.move_cursor(true);

    assert_eq!(selected(&state), ["First on b"]);
}

#[test]
fn an_arrow_key_at_either_end_selects_the_piece_it_is_on_alone() {
    let mut state = state();
    select(&mut state, "On a", &["Loose"]);

    state.move_cursor(false);
    assert_eq!(selected(&state), ["Loose"]);

    select(&mut state, "upstream", &[]);
    state.clear_selection();
    state.move_cursor(true);
    assert_eq!(selected(&state), ["upstream"]);
}
