use crate::git;
use crate::test_helpers::TestRepo;

/// Unmerging a woven branch removes it from integration but keeps the ref.
#[test]
fn unmerge_removes_branch_from_integration() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    let a2_oid = test_repo.head_oid();

    // Create and weave feature-a
    test_repo
        .in_dir(|| crate::branch::new::run(Some("feature-a".to_string()), Some(a2_oid.to_string())))
        .unwrap();

    // Verify it's woven (HEAD is a merge commit)
    assert_eq!(test_repo.head_commit().parent_count(), 2);

    // Unmerge it
    test_repo
        .in_dir(|| super::unmerge::run(Some("feature-a".to_string())))
        .unwrap();

    // Branch ref should still exist
    assert!(test_repo.branch_exists("feature-a"));

    // feature-a should not be in the woven branches list
    let info = git::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        !branch_names.contains(&"feature-a"),
        "feature-a should not be in integration branches after unmerge, got: {:?}",
        branch_names
    );
}

/// Unmerging a branch that isn't woven should error.
#[test]
fn unmerge_non_woven_branch_errors() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");

    // Create a branch at merge-base (not woven, just a ref)
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    let result = test_repo.in_dir(|| super::unmerge::run(Some("feature-a".to_string())));

    assert!(result.is_err());
}
