// ── Test Helpers ───────────────────────────────────────────────────────

use crate::test_helpers::TestRepo;

// ── Integration tests ──────────────────────────────────────────────────
// These tests require full git command execution and call the actual reword
// functions.

#[test]
fn reword_commit_with_message() {
    // Test: Reword a non-HEAD commit's message using -m flag
    // Expected: The targeted commit's message changes, all descendant commits
    // are rewritten with new hashes, but their messages remain unchanged

    let test_repo = TestRepo::new();

    // Create a few commits
    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");
    let c3_oid = test_repo.commit("Third commit", "file3.txt");

    // Reword the first (oldest) commit
    let result = super::reword_commit(
        &test_repo.repo,
        &c1_oid.to_string(),
        Some("Updated first commit".to_string()),
    );

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword commit: {:?}", result);

    // Verify the commit message changed
    // The original c1_oid has been rewritten, so we need to find the new commit
    // by walking back from HEAD
    assert_eq!(test_repo.get_message(2), "Updated first commit");

    // Other commits should have same messages but different hashes (because parent changed)
    assert_eq!(test_repo.get_message(1), "Second commit");
    assert_eq!(test_repo.get_message(0), "Third commit");

    // Verify hashes changed due to rewrite
    assert_ne!(
        test_repo.get_oid(2),
        c1_oid,
        "First commit hash should have changed"
    );
    assert_ne!(
        test_repo.get_oid(0),
        c3_oid,
        "Third commit hash should have changed"
    );

    // Verify HEAD is still on the same branch
    assert!(test_repo.is_on_branch());
}

#[test]
fn reword_commit_without_message() {
    // Test: Reword a commit without -m flag should open editor
    // Expected: Git editor is invoked and the new message from the editor is applied

    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Set up a fake editor that replaces the message
    test_repo.set_fake_editor("Reworded by editor");

    let result = super::reword_commit(&test_repo.repo, &c1_oid.to_string(), None);

    if result.is_err() {
        eprintln!("Note: This test may fail due to platform-specific editor or PowerShell issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword commit: {:?}", result);

    // Verify the commit message was changed by the "editor"
    assert_eq!(test_repo.get_message(1), "Reworded by editor");
}

#[test]
fn reword_root_commit() {
    // Test: Reword the repository's first (root) commit
    // Expected: Uses git rebase --root flag, commit message changes,
    // hash changes, but it remains a root commit (no parents)

    let test_repo = TestRepo::new();

    // Get the root commit (the initial commit)
    let root_commit = test_repo.get_commit(0);
    let root_oid = root_commit.id();
    assert_eq!(root_commit.parent_count(), 0, "Should be a root commit");

    // Reword the root commit
    let result = super::reword_commit(
        &test_repo.repo,
        &root_oid.to_string(),
        Some("Updated initial commit".to_string()),
    );

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword root commit: {:?}", result);

    // Verify the commit message changed
    assert_eq!(test_repo.get_message(0), "Updated initial commit");
    assert_eq!(
        test_repo.get_commit(0).parent_count(),
        0,
        "Should still be a root commit"
    );

    // Hash should have changed
    assert_ne!(
        test_repo.get_oid(0),
        root_oid,
        "Root commit hash should have changed"
    );
}

#[test]
fn reword_root_commit_with_descendants() {
    // Test: Reword root commit when there are commits built on top of it
    // Expected: Root commit message changes, all descendant commits are
    // rewritten with new hashes but same messages

    let test_repo = TestRepo::new();

    // Get the root commit and add more commits on top
    let root_oid = test_repo.get_oid(0);

    test_repo.commit("Second commit", "file2.txt");
    test_repo.commit("Third commit", "file3.txt");

    // Reword the root commit
    let result = super::reword_commit(
        &test_repo.repo,
        &root_oid.to_string(),
        Some("Updated root".to_string()),
    );

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(
        result.is_ok(),
        "Failed to reword root commit with descendants: {:?}",
        result
    );

    // Verify all commits were rewritten
    assert_eq!(test_repo.get_message(2), "Updated root");
    assert_eq!(
        test_repo.get_commit(2).parent_count(),
        0,
        "Should still be a root commit"
    );

    // Other commits should retain their messages
    assert_eq!(test_repo.get_message(1), "Second commit");
    assert_eq!(test_repo.get_message(0), "Third commit");
}

#[test]
fn reword_commit_with_working_tree_changes() {
    // Test: Reword with uncommitted changes in working tree
    // Expected: --autostash flag preserves working tree changes,
    // reword succeeds, and changes are restored after

    let test_repo = TestRepo::new();

    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Make a working tree change
    test_repo.write_file("file2.txt", "modified content");

    // Reword should handle working tree changes (via --autostash)
    let result = super::reword_commit(
        &test_repo.repo,
        &c1_oid.to_string(),
        Some("Updated first".to_string()),
    );

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(
        result.is_ok(),
        "Failed to reword with working tree changes: {:?}",
        result
    );

    // Verify the working tree change is still there
    assert_eq!(
        test_repo.read_file("file2.txt"),
        "modified content",
        "Working tree changes should be preserved"
    );
}

#[test]
fn reword_branch_by_name() {
    // Test: Rename a branch using git branch -m
    // Expected: Old branch name disappears, new branch name exists,
    // branch still points to same commit

    let test_repo = TestRepo::new();

    // Create a branch
    test_repo.create_branch("feature-old");

    // Rename the branch using reword_branch
    let result = super::reword_branch(&test_repo.repo, "feature-old", "feature-new");
    assert!(result.is_ok(), "Failed to rename branch: {:?}", result);

    // Verify old branch doesn't exist
    assert!(
        test_repo
            .repo
            .find_branch("feature-old", git2::BranchType::Local)
            .is_err(),
        "Old branch should not exist after rename"
    );

    // Verify new branch exists and points to same commit
    let new_branch = test_repo
        .repo
        .find_branch("feature-new", git2::BranchType::Local)
        .unwrap();
    assert_eq!(
        new_branch.get().target().unwrap(),
        test_repo.get_oid(0),
        "New branch should point to same commit"
    );
}

#[test]
fn reword_current_branch() {
    // Test: Rename the currently checked out branch
    // Expected: Branch rename succeeds, HEAD still tracks the renamed branch

    let test_repo = TestRepo::new();

    // The default branch is "main" or "master" - rename it
    let current_branch_name = test_repo.current_branch_name();

    let result = super::reword_branch(&test_repo.repo, &current_branch_name, "renamed-main");
    assert!(
        result.is_ok(),
        "Failed to rename current branch: {:?}",
        result
    );

    // Verify HEAD is still on the renamed branch
    assert!(test_repo.is_on_branch(), "HEAD should still be on a branch");
    assert_eq!(
        test_repo.current_branch_name(),
        "renamed-main",
        "HEAD should track renamed branch"
    );
}

#[test]
fn reword_commit_with_partial_hash() {
    // Test: Reword using a partial (7-character) commit hash
    // Expected: Git resolves the partial hash and rewording succeeds

    let test_repo = TestRepo::new();

    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Use partial hash (first 7 characters)
    let partial_hash = &c1_oid.to_string()[..7];
    let result = super::reword_commit(
        &test_repo.repo,
        partial_hash,
        Some("Updated via partial hash".to_string()),
    );

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(
        result.is_ok(),
        "Failed to reword commit with partial hash: {:?}",
        result
    );

    // Verify the commit message changed
    assert_eq!(test_repo.get_message(1), "Updated via partial hash");
}

#[test]
fn reword_nonexistent_commit_fails() {
    // Test: Attempt to reword a commit that doesn't exist
    // Expected: Error during git rebase (invalid revision)

    let test_repo = TestRepo::new();

    // Try to reword a commit that doesn't exist
    let result = super::reword_commit(
        &test_repo.repo,
        "0000000000000000000000000000000000000000",
        Some("New message".to_string()),
    );

    assert!(result.is_err(), "Should fail on nonexistent commit");
}

#[test]
fn reword_nonexistent_branch_fails() {
    // Test: Attempt to rename a branch that doesn't exist
    // Expected: Error from git branch -m

    let test_repo = TestRepo::new();

    // Try to rename a branch that doesn't exist
    let result = super::reword_branch(&test_repo.repo, "nonexistent-branch", "new-name");

    assert!(result.is_err(), "Should fail on nonexistent branch");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Failed to rename branch"),
        "Error should mention branch rename failure"
    );
}

#[test]
fn reword_branch_by_full_name_via_run() {
    // Test: Use reword::run with a full branch name and -m flag
    // Expected: Branch is renamed (not commit at branch tip)

    let test_repo = TestRepo::new();

    // Create a branch
    test_repo.create_branch("feature-original");

    // Change directory to the repo for the run command
    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(test_repo.repo.workdir().unwrap()).unwrap();

    // Rename using full branch name
    let result = super::run(
        "feature-original".to_string(),
        Some("feature-renamed".to_string()),
    );

    // Restore directory
    std::env::set_current_dir(original_dir).unwrap();

    assert!(
        result.is_ok(),
        "Failed to rename branch via run: {:?}",
        result
    );

    // Verify old branch doesn't exist
    assert!(
        test_repo
            .repo
            .find_branch("feature-original", git2::BranchType::Local)
            .is_err(),
        "Old branch should not exist after rename"
    );

    // Verify new branch exists
    assert!(
        test_repo
            .repo
            .find_branch("feature-renamed", git2::BranchType::Local)
            .is_ok(),
        "New branch should exist after rename"
    );
}
