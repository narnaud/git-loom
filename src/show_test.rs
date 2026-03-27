use crate::core::test_helpers::TestRepo;

/// Disable the pager so `git show` doesn't block or pollute test output.
fn no_pager() {
    // SAFETY: tests are serialized via `in_dir`'s global mutex, so no concurrent
    // env mutation occurs.
    unsafe { std::env::set_var("GIT_PAGER", "cat") };
}

#[test]
fn show_commit_by_hash() {
    no_pager();
    let test_repo = TestRepo::new();
    let oid = test_repo.commit("Test commit", "file.txt");

    let result = test_repo.in_dir(|| super::run(oid.to_string()));
    assert!(
        result.is_ok(),
        "show should succeed for a valid commit hash"
    );
}

#[test]
fn show_branch_tip() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("On main", "file.txt");

    let head = test_repo.repo.head().unwrap();
    let branch_name = head.shorthand().unwrap().to_string();

    let result = test_repo.in_dir(|| super::run(branch_name.clone()));
    assert!(result.is_ok(), "show should succeed for a branch name");
}

#[test]
fn show_invalid_target_fails() {
    let test_repo = TestRepo::new();

    let result = test_repo.in_dir(|| super::run("nonexistent_target_xyz".to_string()));
    assert!(result.is_err(), "show should fail for invalid target");
}
