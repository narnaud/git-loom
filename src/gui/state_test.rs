use std::path::PathBuf;

use super::*;
use crate::core::repo::{CommitInfo, FileChange, RemoteStatus, RepoInfo};
use crate::core::shortid::IdAllocator;
use crate::core::test_helpers::{TestRepo, base_info, branch, commit};
use crate::session::paused::test_support::repo_with_a_paused_drop;
use crate::session::tree::RowKind;

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
    let ids = IdAllocator::new(info.collect_entities());
    let nowhere = PathBuf::from("gui-test-fixture-has-no-workdir");
    let snapshot = Snapshot {
        workdir: nowhere.clone(),
        git_dir: nowhere,
        cwd_prefix: String::new(),
        info,
        ids,
    };
    State::new(snapshot, None)
}

pub(crate) fn state() -> State {
    state_of(info())
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
