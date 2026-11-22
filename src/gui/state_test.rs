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
