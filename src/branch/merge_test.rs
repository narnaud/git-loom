use crate::git;
use crate::test_helpers::TestRepo;

/// Merging a non-woven local branch weaves it into integration.
#[test]
fn merge_weaves_existing_branch() {
    let test_repo = TestRepo::new_with_remote();

    // Create commits on integration
    test_repo.commit("B1", "b1.txt");

    // Create feature-a at merge-base with its own commits
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    // Add a commit to feature-a
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    // Merge feature-a into integration
    test_repo
        .in_dir(|| super::merge::run(Some("feature-a".to_string()), false))
        .unwrap();

    // HEAD should be a merge commit
    assert_eq!(
        test_repo.head_commit().parent_count(),
        2,
        "HEAD should be a merge commit after merging a branch"
    );

    // feature-a should now be in the woven branches
    let info = git::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        branch_names.contains(&"feature-a"),
        "feature-a should be in integration branches after merge, got: {:?}",
        branch_names
    );
}

/// Merging an already-woven branch should error.
#[test]
fn merge_already_woven_errors() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    let a2_oid = test_repo.head_oid();

    // Create and weave feature-a via branch::new
    test_repo
        .in_dir(|| crate::branch::new::run(Some("feature-a".to_string()), Some(a2_oid.to_string())))
        .unwrap();

    // Try to merge it again — should error
    let result = test_repo.in_dir(|| super::merge::run(Some("feature-a".to_string()), false));
    assert!(result.is_err(), "merging already-woven branch should error");
}

/// Merging a nonexistent branch should error.
#[test]
fn merge_nonexistent_branch_errors() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");

    let result =
        test_repo.in_dir(|| super::merge::run(Some("nonexistent-branch".to_string()), false));
    assert!(result.is_err(), "merging nonexistent branch should error");
}
