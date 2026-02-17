use crate::test_helpers::TestRepo;

#[test]
fn update_pulls_upstream_changes() {
    let test_repo = TestRepo::new_with_remote();

    // Add commits to the remote
    let remote_oid = test_repo.add_remote_commits(&["Remote commit 1"]);

    // Before update, integration should be behind
    let before_oid = test_repo.head_oid();
    assert_ne!(before_oid, remote_oid);

    let result = test_repo.in_dir(|| super::run());
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // After update, integration should point at the remote commit
    let after_oid = test_repo.head_oid();
    assert_eq!(
        after_oid, remote_oid,
        "HEAD should point at the remote commit after pull"
    );
}

#[test]
fn update_works_when_already_up_to_date() {
    let test_repo = TestRepo::new_with_remote();

    let result = test_repo.in_dir(|| super::run());
    assert!(result.is_ok(), "update failed: {:?}", result.err());
}

#[test]
fn update_fails_on_detached_head() {
    let test_repo = TestRepo::new_with_remote();
    let oid = test_repo.head_oid();
    test_repo.set_detached_head(oid);

    let result = test_repo.in_dir(|| super::run());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("detached"),
        "Expected detached HEAD error, got: {}",
        err
    );
}

#[test]
fn update_fails_without_upstream() {
    let test_repo = TestRepo::new();
    // new() creates a repo without remote/upstream

    let result = test_repo.in_dir(|| super::run());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("upstream") || err.contains("init"),
        "Expected upstream error, got: {}",
        err
    );
}

#[test]
fn update_rebases_local_commits_on_top_of_upstream() {
    let test_repo = TestRepo::new_with_remote();

    // Add a local commit on the integration branch
    test_repo.commit("Local work", "local.txt");

    // Add commits to the remote
    test_repo.add_remote_commits(&["Remote commit"]);

    let result = test_repo.in_dir(|| super::run());
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // Local commit should still be on top
    assert_eq!(test_repo.get_message(0), "Local work");
}
