use crate::core::test_helpers::TestRepo;

fn no_pager() {
    // SAFETY: tests are serialized via `in_dir`'s global mutex.
    unsafe { std::env::set_var("GIT_PAGER", "cat") };
}

#[test]
fn diff_no_args() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("Initial commit", "file.txt");

    let result = test_repo.in_dir(|| super::run(vec![]));
    assert!(result.is_ok(), "diff with no args should succeed");
}

#[test]
fn diff_commit_by_hash() {
    no_pager();
    let test_repo = TestRepo::new();
    let oid = test_repo.commit("Test commit", "file.txt");

    let result = test_repo.in_dir(|| super::run(vec![oid.to_string()]));
    assert!(result.is_ok(), "diff with commit hash should succeed");
}

#[test]
fn diff_commit_range() {
    no_pager();
    let test_repo = TestRepo::new();
    let oid1 = test_repo.commit("First commit", "file1.txt");
    let oid2 = test_repo.commit("Second commit", "file2.txt");

    let range = format!("{}..{}", oid1, oid2);
    let result = test_repo.in_dir(|| super::run(vec![range]));
    assert!(result.is_ok(), "diff with commit range should succeed");
}

#[test]
fn diff_invalid_target_fails() {
    let test_repo = TestRepo::new();

    let result = test_repo.in_dir(|| super::run(vec!["nonexistent_xyz".to_string()]));
    assert!(result.is_err(), "diff with invalid target should fail");
}
