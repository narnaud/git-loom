use crate::git;
use crate::git_commands::{git_branch, git_merge};
use crate::test_helpers::TestRepo;

// ── HEAD split tests ──────────────────────────────────────────────────

#[test]
fn split_head_commit() {
    // Split HEAD into two commits: one with file1.txt, one with file2.txt
    let test_repo = TestRepo::new();
    test_repo.commit("Add files", "file1.txt");

    // Create a commit that touches two files
    test_repo.write_file("file_a.txt", "content a");
    test_repo.write_file("file_b.txt", "content b");
    {
        let mut index = test_repo.repo.index().unwrap();
        index.add_path(std::path::Path::new("file_a.txt")).unwrap();
        index.add_path(std::path::Path::new("file_b.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = test_repo.repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let parent = test_repo.head_commit();
        test_repo
            .repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                "Two files commit",
                &tree,
                &[&parent],
            )
            .unwrap();
    }

    let target_oid = test_repo.head_oid();

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
    let head_files = git::commit_file_paths(&test_repo.repo, test_repo.get_oid(0)).unwrap();
    assert_eq!(head_files, vec!["file_b.txt"]);

    let first_files = git::commit_file_paths(&test_repo.repo, test_repo.get_oid(1)).unwrap();
    assert_eq!(first_files, vec!["file_a.txt"]);
}

// ── Non-HEAD split tests ──────────────────────────────────────────────

#[test]
fn split_non_head_commit() {
    // Split a commit that is not HEAD — requires rebase
    let test_repo = TestRepo::new_with_remote();

    // Create a commit with two files
    test_repo.write_file("file_a.txt", "content a");
    test_repo.write_file("file_b.txt", "content b");
    {
        let mut index = test_repo.repo.index().unwrap();
        index.add_path(std::path::Path::new("file_a.txt")).unwrap();
        index.add_path(std::path::Path::new("file_b.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = test_repo.repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let parent = test_repo.head_commit();
        test_repo
            .repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                "Two files commit",
                &tree,
                &[&parent],
            )
            .unwrap();
    }
    let target_oid = test_repo.head_oid();

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
    test_repo.create_branch("side");
    test_repo.switch_branch("side");
    let c2_oid = test_repo.commit("Side", "side.txt");

    // Switch back and create a merge
    test_repo.switch_branch(&test_repo.current_branch_name().replace("side", "main"));
    // Back to main which may be named "main" or "master"
    // Try using commit_merge directly
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
    test_repo.write_file("file_a.txt", "content a");
    test_repo.write_file("file_b.txt", "content b");
    {
        let mut index = test_repo.repo.index().unwrap();
        index.add_path(std::path::Path::new("file_a.txt")).unwrap();
        index.add_path(std::path::Path::new("file_b.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = test_repo.repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let parent = test_repo.head_commit();
        test_repo
            .repo
            .commit(Some("HEAD"), &sig, &sig, "Split me", &tree, &[&parent])
            .unwrap();
    }
    let split_oid = test_repo.head_oid();

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
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at merge-base with a two-file commit
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-a");

    test_repo.write_file("fa1.txt", "content a1");
    test_repo.write_file("fa2.txt", "content a2");
    {
        let mut index = test_repo.repo.index().unwrap();
        index.add_path(std::path::Path::new("fa1.txt")).unwrap();
        index.add_path(std::path::Path::new("fa2.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = test_repo.repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let parent = test_repo.head_commit();
        test_repo
            .repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                "Feature A files",
                &tree,
                &[&parent],
            )
            .unwrap();
    }
    let split_oid = test_repo.head_oid();

    // Switch back to integration, add a commit, then weave
    test_repo.switch_branch("integration");
    test_repo.commit("Int commit", "int.txt");
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();

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
