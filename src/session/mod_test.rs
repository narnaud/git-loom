use super::*;
use crate::core::test_helpers::TestRepo;
use crate::core::transaction::Resumable;
use crate::session::paused::test_support::repo_with_a_paused_drop;

#[test]
fn a_session_opens_on_the_integration_branch() {
    let repo = TestRepo::new_with_remote();

    let opened = repo.in_dir(open).unwrap();

    assert!(opened.paused.is_none());
    assert_eq!(opened.snapshot.info.branch_name, "integration");
    assert_eq!(opened.snapshot.git_dir, repo.repo.path());
    assert_eq!(opened.stamp, repo_stamp(repo.repo.path()));
}

/// HEAD is detached while an operation is paused, so there is no integration
/// branch to load: the session opens on the operation instead of failing.
#[test]
fn a_session_opens_on_a_paused_operation() {
    let (repo, _) = repo_with_a_paused_drop();

    let opened = repo.in_dir(open).unwrap();

    let paused = opened.paused.expect("the paused operation");
    assert_eq!(paused.command, Some(Resumable::Drop));
    assert_eq!(opened.snapshot.info.branch_name, "", "a placeholder");
}

/// An unreadable state reads as no loom command further down: a front-end
/// would show the rebase as git's own, whose abort skips loom's rollback.
#[test]
fn an_unreadable_loom_state_opens_nothing() {
    let (repo, _) = repo_with_a_paused_drop();
    let state = repo.repo.path().join("loom").join("state.json");
    assert!(state.exists());
    std::fs::write(&state, "not json").unwrap();

    let error = repo.in_dir(open).err().expect("no session").to_string();

    assert!(error.contains("corrupted or invalid"), "{error}");
}

#[test]
fn outside_a_repository_there_is_nothing_to_open() {
    let nowhere = tempfile::tempdir().unwrap();
    let repo = TestRepo::new_with_remote();

    assert!(repo.in_dir_path(nowhere.path(), open).is_err());
}
