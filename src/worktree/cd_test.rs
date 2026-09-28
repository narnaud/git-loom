use crate::core::test_helpers::TestRepo;

#[test]
fn cd_resolves_a_worktree_and_the_main_one() {
    let test_repo = TestRepo::new_with_remote();
    let path = test_repo.add_worktree("work-foo", &["-b", "integration-foo", "origin/main"]);

    test_repo
        .in_dir(|| super::run(Some("foo".to_string())))
        .unwrap();
    test_repo
        .in_dir_path(&path, || super::run(Some("work".to_string())))
        .unwrap();
    test_repo.in_dir_path(&path, || super::run(None)).unwrap();

    let err = test_repo
        .in_dir(|| super::run(Some("nope".to_string())))
        .unwrap_err();
    assert!(err.to_string().contains("not found"), "got: {err}");
}

#[test]
fn cd_without_argument_in_the_main_worktree_needs_another_one() {
    let test_repo = TestRepo::new_with_remote();

    let err = test_repo.in_dir(|| super::run(None)).unwrap_err();

    assert!(err.to_string().contains("No other worktree"), "got: {err}");
}
