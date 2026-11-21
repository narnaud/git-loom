use super::*;
use crate::core::test_helpers::TestRepo;
use crate::session::paused::test_support::repo_with_a_paused_drop;

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
