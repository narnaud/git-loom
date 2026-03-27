use crate::core::test_helpers::TestRepo;

// ── HEAD split tests ──────────────────────────────────────────────────

#[test]
fn split_head_commit() {
    // Split HEAD into two commits: one with file1.txt, one with file2.txt
    let test_repo = TestRepo::new();
    test_repo.commit("Add files", "file1.txt");

    // Create a commit that touches two files
    let target_oid = test_repo.commit_multi(
        &[("file_a.txt", "content a"), ("file_b.txt", "content b")],
        "Two files commit",
    );

    let result = super::split_commit_with_selection(
        &test_repo.repo,
        &target_oid.to_string(),
        vec!["file_a.txt".to_string()],
        "First part".to_string(),
    );

    assert!(result.is_ok(), "split_head_commit failed: {:?}", result);

    // HEAD should be the second commit (original message)
    assert_eq!(test_repo.get_message(0), "Two files commit");
    // HEAD~1 should be the first commit (new message)
    assert_eq!(test_repo.get_message(1), "First part");

    // Verify files are in the right commits
    assert_eq!(
        test_repo.commit_file_paths(test_repo.get_oid(0)),
        vec!["file_b.txt"]
    );
    assert_eq!(
        test_repo.commit_file_paths(test_repo.get_oid(1)),
        vec!["file_a.txt"]
    );
}

// ── Non-HEAD split tests ──────────────────────────────────────────────

#[test]
fn split_non_head_commit() {
    // Split a commit that is not HEAD — requires rebase
    let test_repo = TestRepo::new_with_remote();

    // Create a commit with two files
    let target_oid = test_repo.commit_multi(
        &[("file_a.txt", "content a"), ("file_b.txt", "content b")],
        "Two files commit",
    );

    // Add another commit on top
    test_repo.commit("Later commit", "later.txt");

    let result = super::split_commit_with_selection(
        &test_repo.repo,
        &target_oid.to_string(),
        vec!["file_a.txt".to_string()],
        "First part".to_string(),
    );

    assert!(result.is_ok(), "split_non_head_commit failed: {:?}", result);

    // HEAD should still be the later commit
    assert_eq!(test_repo.get_message(0), "Later commit");
    // HEAD~1 should be the second part (original message)
    assert_eq!(test_repo.get_message(1), "Two files commit");
    // HEAD~2 should be the first part (new message)
    assert_eq!(test_repo.get_message(2), "First part");
}

// ── Validation error tests ───────────────────────────────────────────

#[test]
fn split_single_file_fails() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit("Single file", "only.txt");

    let result = super::split_commit_with_selection(
        &test_repo.repo,
        &c1_oid.to_string(),
        vec!["only.txt".to_string()],
        "Should fail".to_string(),
    );

    assert!(result.is_err(), "Should fail on single-file commit");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("only one file"),
        "Error should mention single file: {}",
        err
    );
}

#[test]
fn split_merge_commit_fails() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit("First", "file1.txt");

    // Create a branch with a different commit
    let default_branch = test_repo.current_branch_name();
    test_repo.create_branch("side");
    test_repo.switch_branch("side");
    let c2_oid = test_repo.commit("Side", "side.txt");

    // Switch back and create a merge
    test_repo.switch_branch(&default_branch);
    let merge_oid = test_repo.commit_merge("Merge", c1_oid, c2_oid);

    let result = super::split_commit_with_selection(
        &test_repo.repo,
        &merge_oid.to_string(),
        vec!["file1.txt".to_string()],
        "Should fail".to_string(),
    );

    assert!(result.is_err(), "Should fail on merge commit");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("merge commit"),
        "Error should mention merge commit: {}",
        err
    );
}

// ── Preservation tests ───────────────────────────────────────────────

#[test]
fn split_preserves_other_commits() {
    // Commits before and after the split target should be unchanged
    let test_repo = TestRepo::new_with_remote();

    let c1_oid = test_repo.commit("Before", "before.txt");

    // Create a commit with two files
    let split_oid = test_repo.commit_multi(
        &[("file_a.txt", "content a"), ("file_b.txt", "content b")],
        "Split me",
    );

    test_repo.commit("After", "after.txt");

    let result = super::split_commit_with_selection(
        &test_repo.repo,
        &split_oid.to_string(),
        vec!["file_a.txt".to_string()],
        "First part".to_string(),
    );

    assert!(
        result.is_ok(),
        "split_preserves_other_commits failed: {:?}",
        result
    );

    // Verify surrounding commits are preserved
    assert_eq!(test_repo.get_message(0), "After");
    assert_eq!(test_repo.get_message(1), "Split me");
    assert_eq!(test_repo.get_message(2), "First part");
    assert_eq!(test_repo.get_message(3), "Before");

    // Before commit is an ancestor of the rebase range and should be unchanged
    assert_eq!(test_repo.get_oid(3), c1_oid);
}

#[test]
fn split_with_woven_branches() {
    // Verify that split preserves merge topology of woven branches
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at merge-base with a two-file commit
    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");

    let split_oid = test_repo.commit_multi(
        &[("fa1.txt", "content a1"), ("fa2.txt", "content a2")],
        "Feature A files",
    );

    // Switch back to integration, add a commit, then weave
    test_repo.switch_branch("integration");
    test_repo.commit("Int commit", "int.txt");
    test_repo.merge_no_ff("feature-a");

    let result = super::split_commit_with_selection(
        &test_repo.repo,
        &split_oid.to_string(),
        vec!["fa1.txt".to_string()],
        "Feature A part 1".to_string(),
    );

    assert!(
        result.is_ok(),
        "split_with_woven_branches failed: {:?}",
        result
    );

    // Verify feature-a branch still exists
    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a branch should still exist"
    );

    // Verify the split created two commits on the branch
    // The HEAD should still be on integration with merge topology preserved
    let head = test_repo.head_commit();
    assert!(
        head.parent_count() > 1,
        "HEAD should still be a merge commit"
    );
}
