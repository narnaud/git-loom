use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use super::*;
use crate::core::repo::{CommitInfo, FileChange, RemoteStatus, RepoInfo};
use crate::core::shortid::IdAllocator;
use crate::core::test_helpers::{TestRepo, base_info, branch, commit};
use crate::core::ui::{self, Answer, Cancelled, PromptKind};
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
    State::new(snapshot_in(info, cwd_prefix), None, 1, 4)
}

fn snapshot_in(info: RepoInfo, cwd_prefix: &str) -> Snapshot {
    let ids = IdAllocator::new(info.collect_entities());
    let nowhere = PathBuf::from("gui-test-fixture-has-no-workdir");
    Snapshot {
        workdir: nowhere.clone(),
        git_dir: nowhere,
        cwd_prefix: cwd_prefix.to_string(),
        info,
        ids,
    }
}

pub(crate) fn state() -> State {
    state_of(info())
}

/// Index of the row that reads `text`: a path, a branch name, a message.
pub(crate) fn row(state: &State, text: &str) -> usize {
    let found = state.rows.iter().position(|r| match &r.kind {
        RowKind::CommitFile { path, .. } | RowKind::WorkingFile { path, .. } => path == text,
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

/// What `row` reads, as [`row`] finds it.
fn reads(row: &Row) -> String {
    match &row.kind {
        RowKind::CommitFile { path, .. } | RowKind::WorkingFile { path, .. } => path.clone(),
        RowKind::BranchName { name, .. } => name.clone(),
        RowKind::Commit { message, .. } => message.clone(),
        RowKind::Upstream { .. } => "upstream".to_string(),
        RowKind::LocalChanges { .. } => "local changes".to_string(),
        _ => "not shown".to_string(),
    }
}

/// What the selected rows read, in tree order.
pub(crate) fn selected(state: &State) -> Vec<String> {
    let rows = state.rows.iter().filter(|row| state.is_selected(row));
    rows.map(reads).collect()
}

/// What the rows the local changes pane lists read.
fn listed(state: &State) -> Vec<String> {
    state.listed().map(|i| reads(&state.rows[i])).collect()
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
    let local = HashSet::from([tree::LOCAL_CHANGES_KEY.to_string()]);
    assert_eq!(state.expanded, local, "the local changes are always open");
    assert!(state.collapsed.is_empty());
}

#[test]
fn the_local_changes_pane_lists_what_stands_for_them_all_then_each_file() {
    let state = state();

    assert_eq!(listed(&state), ["local changes", "a.rs", "b.rs", "new.rs"]);
}

#[test]
fn the_local_changes_are_not_listed_while_an_operation_is_paused() {
    let (repo, _) = repo_with_a_paused_drop();
    let mut state = repo.in_dir(State::load).unwrap();

    assert!(listed(&state).is_empty());

    state.row_pressed(row(&state, "local changes"), false, false);
    state.move_cursor(true);
    assert!(selected(&state).is_empty());
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
fn a_local_change_is_selected_as_a_piece_is_and_keeps_to_its_class() {
    let mut state = state();

    select(&mut state, "a.rs", &["new.rs", "local changes", "Loose"]);
    assert_eq!(selected(&state), ["a.rs", "new.rs"]);

    select(&mut state, "local changes", &["b.rs"]);
    assert_eq!(
        selected(&state),
        ["local changes"],
        "which stands for them all"
    );
}

#[test]
fn shift_click_from_a_local_file_onto_the_board_keeps_to_the_local_files() {
    let mut state = state();
    select(&mut state, "a.rs", &[]);

    state.row_pressed(row(&state, "Loose"), false, true);

    assert_eq!(selected(&state), ["a.rs", "b.rs", "new.rs"]);
}

#[test]
fn a_press_on_a_row_neither_pane_shows_selects_nothing() {
    let mut state = state();
    select(&mut state, "On a", &[]);
    let spacer = state
        .rows
        .iter()
        .position(|r| matches!(r.kind, RowKind::Spacer(_)));

    state.row_pressed(spacer.unwrap(), false, false);
    state.row_pressed(state.rows.len(), false, false);

    assert_eq!(selected(&state), ["On a"]);
}

#[test]
fn the_arrow_keys_step_through_the_local_changes_then_the_board_and_stop_at_its_ends() {
    let mut state = state();
    let mut walk = Vec::new();

    for _ in 0..12 {
        state.move_cursor(true);
        walk.extend(selected(&state));
    }

    assert_eq!(
        walk,
        [
            "local changes",
            "a.rs",
            "b.rs",
            "new.rs",
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
    for _ in 0..12 {
        state.move_cursor(false);
    }
    assert_eq!(selected(&state), ["local changes"]);
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
fn left_on_a_local_file_goes_up_to_the_local_changes_which_stay_listed() {
    let mut state = state();
    select(&mut state, "new.rs", &[]);

    state.collapse_cursor();
    assert_eq!(selected(&state), ["local changes"]);

    state.collapse_cursor();
    state.expand_cursor();
    assert_eq!(selected(&state), ["local changes"]);
    assert_eq!(listed(&state).len(), 4);
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

#[test]
fn the_window_is_told_which_local_file_the_selection_was_last_put_on() {
    let mut state = state();

    select(&mut state, "a.rs", &["new.rs"]);
    assert_eq!(state.cursor_change(), Some(2));
    assert_eq!(state.cursor_rect(), None, "which is not on the board");

    state.clear_selection();
    assert_eq!(state.cursor_change(), None, "nothing to keep in view");

    select(&mut state, "local changes", &[]);
    assert_eq!(state.cursor_change(), None, "which is always in view");

    select(&mut state, "On a", &[]);
    assert_eq!(state.cursor_change(), None);
    assert!(state.cursor_rect().is_some());
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
    select(&mut state, "local changes", &[]);
    state.clear_selection();

    state.move_cursor(false);
    assert_eq!(selected(&state), ["local changes"]);

    select(&mut state, "upstream", &[]);
    state.clear_selection();
    state.move_cursor(true);
    assert_eq!(selected(&state), ["upstream"]);
}

/// What the inspector names, or nothing.
fn inspected_title(state: &mut State) -> Option<String> {
    state.inspected().map(|shown| shown.title)
}

#[test]
fn the_inspector_shows_nothing_with_nothing_selected() {
    let mut state = state();

    assert!(state.inspected().is_none());

    select(&mut state, "On a", &[]);
    state.clear_selection();
    assert!(state.inspected().is_none(), "nor what was selected before");
}

#[test]
fn the_inspector_names_each_kind_of_piece() {
    let mut state = state();
    state.row_toggled(row(&state, "Second on b"));
    let commit = state.rows[row(&state, "Second on b")].sid.clone();
    let mut title = |text: &str| {
        select(&mut state, text, &[]);
        inspected_title(&mut state).unwrap()
    };

    assert_eq!(title("local changes"), "Local changes");
    assert_eq!(title("b.rs"), "b.rs");
    assert_eq!(title("feature-a"), "Branch feature-a");
    assert_eq!(title("Second on b"), format!("{commit}  Second on b"));
    assert_eq!(title("y.rs"), format!("y.rs  in {commit}"));
    assert_eq!(title("upstream"), "Upstream origin/main");
}

/// As the board and the local changes write the path of that file (Spec 001).
#[test]
fn the_inspector_names_a_file_from_the_directory_the_window_started_in() {
    let mut state = state_in(info(), "docs");
    state.row_toggled(row(&state, "On a"));
    select(&mut state, "z.rs", &[]);

    let title = inspected_title(&mut state).unwrap();

    assert!(title.starts_with("../z.rs  in "), "{title}");
    select(&mut state, "a.rs", &[]);
    assert_eq!(inspected_title(&mut state).unwrap(), "../a.rs");
}

#[test]
fn the_inspector_shows_the_diff_of_a_local_file_and_of_all_of_them() {
    let repo = TestRepo::new_with_remote();
    repo.commit("Loose on the line", "loose.txt");
    repo.write_file("loose.txt", "new line\n");
    let mut state = repo.in_dir(State::load).unwrap();

    select(&mut state, "loose.txt", &[]);
    let file = state.inspected().unwrap().diff.to_string();
    select(&mut state, "local changes", &[]);
    let all = state.inspected().unwrap().diff.to_string();

    assert!(file.contains("+new line"), "{file}");
    assert!(all.contains("+new line"), "{all}");
}

#[test]
fn the_inspector_shows_the_piece_the_selection_was_last_put_on() {
    let mut state = state();

    select(&mut state, "First on b", &["On a"]);

    assert!(inspected_title(&mut state).unwrap().ends_with("On a"));
}

/// The one it was last put on can be taken out of the selection again.
#[test]
fn the_inspector_falls_back_on_the_first_selected_piece() {
    let mut state = state();

    select(&mut state, "Second on b", &["First on b", "On a", "On a"]);

    assert!(
        inspected_title(&mut state)
            .unwrap()
            .ends_with("Second on b")
    );
}

#[test]
fn the_inspector_keeps_to_what_is_selected_when_a_branch_closes_over_the_rest() {
    let mut state = state();
    select(&mut state, "On a", &["First on b"]);

    state.row_toggled(row(&state, "feature-b"));

    assert!(inspected_title(&mut state).unwrap().ends_with("On a"));
}

#[test]
fn the_inspector_says_what_the_upstream_holds() {
    let mut state = state();
    select(&mut state, "upstream", &[]);

    let shown = state.inspected().unwrap();

    assert_eq!(
        shown.diff,
        "upstream: origin/main\ncommon base: aaa0000 Initial commit\n0 new commit(s) on the remote"
    );
}

/// The window asks at each sync: a resize must not start git again.
#[test]
fn the_diff_of_a_piece_is_read_from_git_once() {
    let repo = TestRepo::new_with_remote();
    repo.commit("Loose on the line", "loose.txt");
    let mut state = repo.in_dir(State::load).unwrap();
    select(&mut state, "Loose on the line", &[]);

    let first = state.inspected().unwrap().diff.to_string();
    assert!(first.starts_with("commit "), "{first}");
    assert!(first.contains("+++ b/loose.txt"), "{first}");

    state.snapshot.workdir = PathBuf::from("gui-test-fixture-has-no-workdir");
    assert_eq!(state.inspected().unwrap().diff, first);
}

#[test]
fn each_inspected_piece_is_told_from_the_others_by_its_key() {
    let mut state = state();
    select(&mut state, "On a", &[]);
    let commit = state.inspected().unwrap().key.to_string();

    select(&mut state, "feature-a", &[]);
    let branch = state.inspected().unwrap().key.to_string();

    assert_ne!(commit, branch);
    select(&mut state, "On a", &[]);
    assert_eq!(state.inspected().unwrap().key, commit);
}

#[test]
fn the_width_of_a_tab_is_the_repositorys() {
    let repo = TestRepo::new_with_remote();
    repo.set_config("loom.tuiTabWidth", "2");

    let state = repo.in_dir(State::load).unwrap();

    assert_eq!(state.tab_width(), 2);
}

// ── Commands ──

/// Take in what the worker sent until `done` holds.
pub(crate) fn tick_until(state: &mut State, done: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(state) {
        assert!(Instant::now() < deadline, "timed out");
        state.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_status_bar_names_the_command_running_and_the_step_it_is_at() {
    let mut state = state();
    let (go, wait) = channel::<()>();
    state.worker.run("loom update".to_string(), move || {
        ui::spinner(Some("Fetching"));
        let _ = wait.recv();
        Err(Cancelled.into())
    });

    tick_until(&mut state, |state| {
        state.status().1 == "loom update — Fetching"
    });
    assert!(state.busy());
    assert!(!state.may_close(), "its git would be killed mid-way");
    assert_eq!(
        state.status(),
        (
            Level::Warn,
            "A command is running: wait for it to end".to_string()
        )
    );
    go.send(()).unwrap();
    tick_until(&mut state, |state| !state.busy());

    assert_eq!(state.status(), (Level::Plain, "Cancelled".to_string()));
    assert!(state.may_close());
}

#[test]
fn what_the_command_running_asks_is_asked_over_the_window() {
    let mut state = state();
    let (seen, answers) = channel();
    state.worker.run("loom drop a".to_string(), move || {
        let answer = ui::prompt(PromptKind::Confirm, "Drop it?", None)?;
        seen.send(answer).unwrap();
        Err(Cancelled.into())
    });

    tick_until(&mut state, |state| state.form().is_some());
    let form = state.form().unwrap();
    assert_eq!(
        (form.title.as_str(), form.body.as_str()),
        ("loom drop a", "Drop it?")
    );
    state.form_accepted("", None, 0);

    assert_eq!(answers.recv().unwrap(), Answer::Bool(true));
    tick_until(&mut state, |state| !state.busy());
}

#[test]
fn a_command_that_went_through_is_summed_up_by_its_last_success() {
    let mut state = state();
    state.worker.log.push(LogEntry {
        command: "loom drop a b".to_string(),
        lines: vec![
            (Level::Success, "Dropped `a`".to_string()),
            (Level::Plain, "a note".to_string()),
            (Level::Success, "Dropped `b`\nwith a hint".to_string()),
        ],
    });

    state.report(Ended::Done);

    assert_eq!(
        state.status(),
        (
            Level::Success,
            "Dropped `b` (and 1 more in the log)".to_string()
        )
    );
    assert!(state.form().is_none());
}

#[test]
fn a_command_that_failed_says_why_over_the_window_unless_the_log_shows_it() {
    let mut state = state();
    let error = "Push refused\nIf `a` has diverged on the remote, pick Force push in its menu";
    state.worker.log.push(LogEntry {
        command: "loom push a".to_string(),
        lines: vec![(Level::Error, error.to_string())],
    });

    state.report(Ended::Failed(error.to_string()));

    assert_eq!(state.status(), (Level::Error, "Push refused".to_string()));
    let form = state.form().unwrap();
    assert_eq!(
        (form.title.as_str(), form.body.as_str()),
        ("loom push a", "Push refused")
    );
    state.form_accepted("", None, 0);
    state.open_log();
    state.report(Ended::Failed(error.to_string()));
    assert!(state.form().is_none());
}

#[test]
fn a_command_that_ended_shows_the_repository_as_it_left_it() {
    let repo = TestRepo::new_with_remote();
    repo.commit("Loose on the line", "loose.txt");
    let mut state = repo.in_dir(State::load).unwrap();
    select(&mut state, "Loose on the line", &[]);
    state.worker.run("loom commit".to_string(), || Ok(()));
    repo.write_file("made.txt", "by the command\n");

    repo.in_dir(|| tick_until(&mut state, |state| !state.busy()));

    assert_eq!(listed(&state), ["local changes", "made.txt"]);
    assert_eq!(selected(&state), ["Loose on the line"]);
    assert_eq!(state.status(), (Level::Success, "Done".to_string()));
}

#[test]
fn a_selection_goes_to_the_cursor_row_after_a_command_else_to_its_place() {
    let mut state = state();
    select(&mut state, "feature-a", &[]);
    state.apply(snapshot_in(info(), ""));
    assert_eq!(selected(&state), ["feature-a"], "the same row");

    select(&mut state, "On a", &[]);
    let mut reworded = info();
    reworded.commits[2] = commit(0xA2, "On a, reworded", Some(0xAA));
    reworded.branches[1] = branch("feature-a", 0xA2, Some(RemoteStatus::Synced));
    state.apply(snapshot_in(reworded, ""));
    assert_eq!(selected(&state), ["On a, reworded"], "what is in its place");

    select(&mut state, "a.rs", &["b.rs"]);
    let mut committed = info();
    committed
        .working_changes
        .retain(|change| change.path == "new.rs");
    state.apply(snapshot_in(committed, ""));
    assert_eq!(selected(&state), ["new.rs"], "the nearest above its place");
}

#[test]
fn nothing_is_selected_after_a_command_run_with_nothing_selected() {
    let mut state = state();
    select(&mut state, "feature-a", &[]);
    state.clear_selection();

    state.apply(snapshot_in(info(), ""));

    assert!(selected(&state).is_empty());
}

#[test]
fn the_log_opens_over_the_window_and_closes_again() {
    let mut state = state();
    assert!(!state.log_open());

    state.open_log();
    assert!(state.log_open());
    state.close_log();

    assert!(!state.log_open());
}

#[test]
fn a_branch_a_command_created_or_renamed_is_selected_once_it_is_read() {
    let mut state = state();
    state.next_cursor = Some(tree::branch_key("feature-a"));

    state.apply(snapshot_in(info(), ""));

    assert_eq!(selected(&state), ["feature-a"]);
    assert!(state.next_cursor.is_none());
}

#[test]
fn each_read_of_the_repository_is_counted() {
    let mut state = state();
    let before = state.reads();

    state.apply(snapshot_in(info(), ""));

    assert_eq!(state.reads(), before + 1);
}
