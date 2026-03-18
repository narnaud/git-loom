use crate::test_helpers::TestRepo;
use crate::weave::Weave;

// ── swap commits ──────────────────────────────────────────────────────────

#[test]
fn swap_commits_on_integration_line() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("First", "first.txt");
    let c2_oid = test_repo.commit("Second", "second.txt");
    test_repo.commit("Third", "third.txt");

    let result = super::swap_two_commits(&test_repo.repo, c1_oid.to_string(), c2_oid.to_string());
    assert!(result.is_ok(), "swap_two_commits failed: {:?}", result);

    // Second was applied before First, so in newest-first order:
    // HEAD=Third, HEAD~1=First, HEAD~2=Second
    assert_eq!(test_repo.get_message(0), "Third");
    assert_eq!(test_repo.get_message(1), "First");
    assert_eq!(test_repo.get_message(2), "Second");
}

#[test]
fn swap_commits_in_branch_section() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    let a1_oid = test_repo.commit("A1", "a1.txt");
    let a2_oid = test_repo.commit("A2", "a2.txt");

    test_repo.switch_branch("integration");
    test_repo.commit("Int", "int.txt");
    test_repo.merge_no_ff("feature-a");

    let result = super::swap_two_commits(&test_repo.repo, a1_oid.to_string(), a2_oid.to_string());
    assert!(
        result.is_ok(),
        "swap_two_commits in branch failed: {:?}",
        result
    );

    // Verify via weave: A2 should now be first (oldest) in the section
    let graph = Weave::from_repo(&test_repo.repo).unwrap();
    assert_eq!(graph.branch_sections[0].commits[0].message, "A2");
    assert_eq!(graph.branch_sections[0].commits[1].message, "A1");
}

#[test]
fn swap_commits_across_sections_errors() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    let a1_oid = test_repo.commit("A1", "a1.txt");

    test_repo.create_branch_at("feature-b", &base_oid.to_string());
    test_repo.switch_branch("feature-b");
    let b1_oid = test_repo.commit("B1", "b1.txt");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");
    test_repo.merge_no_ff("feature-b");

    let result = super::swap_two_commits(&test_repo.repo, a1_oid.to_string(), b1_oid.to_string());
    assert!(
        result.is_err(),
        "should fail for commits in different sections"
    );
    assert!(result.unwrap_err().to_string().contains("different"));
}
