// ── Test Helpers ───────────────────────────────────────────────────────

use git2::{Repository, Signature};
use std::fs;
use std::path::Path;

/// Create a signature for test commits.
fn sig() -> Signature<'static> {
    Signature::now("Test", "test@test.com").unwrap()
}

/// Create a commit with a file (so working tree changes can be tested).
fn make_commit_with_file(repo: &Repository, message: &str, filename: &str) -> git2::Oid {
    let path = repo.workdir().unwrap().join(filename);
    fs::write(&path, message).unwrap();

    let mut index = repo.index().unwrap();
    index.add_path(Path::new(filename)).unwrap();
    index.write().unwrap();

    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = sig();

    if let Ok(head) = repo.head() {
        let parent = repo.find_commit(head.target().unwrap()).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
            .unwrap()
    } else {
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
            .unwrap()
    }
}

/// Create a simple test repo (for testing commit operations).
fn setup_simple_repo() -> (Repository, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();

    // Create an initial commit
    {
        let sig = sig();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }

    (repo, dir)
}

// ── Integration tests ──────────────────────────────────────────────────
// These tests require full git command execution and call the actual reword
// functions. They're marked with  and can be run with:
//   cargo test -- --ignored
//
// Note: Some tests may fail due to platform-specific issues with the PowerShell
// sequence editor command in reword.rs. This is a known issue being tracked.

#[test]
fn reword_commit_with_message() {
    // Test: Reword a non-HEAD commit's message using -m flag
    // Expected: The targeted commit's message changes, all descendant commits
    // are rewritten with new hashes, but their messages remain unchanged

    let (repo, _dir) = setup_simple_repo();

    // Create a few commits
    let c1_oid = make_commit_with_file(&repo, "First commit", "file1.txt");
    make_commit_with_file(&repo, "Second commit", "file2.txt");
    let c3_oid = make_commit_with_file(&repo, "Third commit", "file3.txt");

    // Reword the first (oldest) commit
    let result = super::reword_commit(&repo, &c1_oid.to_string(), Some("Updated first commit".to_string()));

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword commit: {:?}", result);

    // Verify the commit message changed
    // The original c1_oid has been rewritten, so we need to find the new commit
    // by walking back from HEAD
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let parent1 = head.parent(0).unwrap();
    let parent2 = parent1.parent(0).unwrap();

    // The oldest commit (originally c1) should have the new message
    assert_eq!(parent2.message().unwrap().trim(), "Updated first commit");

    // Other commits should have same messages but different hashes (because parent changed)
    assert_eq!(parent1.message().unwrap().trim(), "Second commit");
    assert_eq!(head.message().unwrap().trim(), "Third commit");

    // Verify hashes changed due to rewrite
    assert_ne!(parent2.id(), c1_oid, "First commit hash should have changed");
    assert_ne!(head.id(), c3_oid, "Third commit hash should have changed");

    // Verify HEAD is still on the same branch
    assert!(repo.head().unwrap().is_branch());
}

#[test]
fn reword_commit_without_message() {
    // Test: Reword a commit without -m flag should open editor
    // Expected: Git editor is invoked and the new message from the editor is applied

    let (repo, _dir) = setup_simple_repo();
    let c1_oid = make_commit_with_file(&repo, "First commit", "file1.txt");
    make_commit_with_file(&repo, "Second commit", "file2.txt");

    // Set up a fake editor that replaces the message
    let editor_script = if cfg!(windows) {
        // Use a PowerShell script for Windows with unique filename
        let script_dir = std::env::temp_dir();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let script_path = script_dir.join(format!("test_editor_reword_{}.ps1", timestamp));
        std::fs::write(&script_path, "param($file)\nSet-Content -Path $file -Value 'Reworded by editor'\n").unwrap();
        format!("powershell -ExecutionPolicy Bypass -File \"{}\"", script_path.display())
    } else {
        "sh -c 'echo \"Reworded by editor\" > \"$1\"' --".to_string()
    };

    // SAFETY: This is a test environment and we're setting a git-specific env var
    // that won't affect other tests or the system
    unsafe {
        std::env::set_var("GIT_EDITOR", &editor_script);
    }

    let result = super::reword_commit(&repo, &c1_oid.to_string(), None);

    if result.is_err() {
        eprintln!("Note: This test may fail due to platform-specific editor or PowerShell issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword commit: {:?}", result);

    // Verify the commit message was changed by the "editor"
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let parent1 = head.parent(0).unwrap();

    assert_eq!(parent1.message().unwrap().trim(), "Reworded by editor");
}

#[test]

fn reword_root_commit() {
    // Test: Reword the repository's first (root) commit
    // Expected: Uses git rebase --root flag, commit message changes,
    // hash changes, but it remains a root commit (no parents)

    let (repo, _dir) = setup_simple_repo();

    // Get the root commit (the initial commit)
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let root_oid = head.id();
    assert_eq!(head.parent_count(), 0, "Should be a root commit");

    // Reword the root commit
    let result = super::reword_commit(&repo, &root_oid.to_string(), Some("Updated initial commit".to_string()));

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword root commit: {:?}", result);

    // Verify the commit message changed
    let new_head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(new_head.message().unwrap().trim(), "Updated initial commit");
    assert_eq!(new_head.parent_count(), 0, "Should still be a root commit");

    // Hash should have changed
    assert_ne!(new_head.id(), root_oid, "Root commit hash should have changed");
}

#[test]

fn reword_root_commit_with_descendants() {
    // Test: Reword root commit when there are commits built on top of it
    // Expected: Root commit message changes, all descendant commits are
    // rewritten with new hashes but same messages

    let (repo, _dir) = setup_simple_repo();

    // Get the root commit and add more commits on top
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let root_oid = head.id();

    make_commit_with_file(&repo, "Second commit", "file2.txt");
    make_commit_with_file(&repo, "Third commit", "file3.txt");

    // Reword the root commit
    let result = super::reword_commit(&repo, &root_oid.to_string(), Some("Updated root".to_string()));

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword root commit with descendants: {:?}", result);

    // Verify all commits were rewritten
    let new_head = repo.head().unwrap().peel_to_commit().unwrap();
    let parent1 = new_head.parent(0).unwrap();
    let parent2 = parent1.parent(0).unwrap();

    // The root should have the new message
    assert_eq!(parent2.message().unwrap().trim(), "Updated root");
    assert_eq!(parent2.parent_count(), 0, "Should still be a root commit");

    // Other commits should retain their messages
    assert_eq!(parent1.message().unwrap().trim(), "Second commit");
    assert_eq!(new_head.message().unwrap().trim(), "Third commit");
}

#[test]

fn reword_commit_with_working_tree_changes() {
    // Test: Reword with uncommitted changes in working tree
    // Expected: --autostash flag preserves working tree changes,
    // reword succeeds, and changes are restored after

    let (repo, _dir) = setup_simple_repo();

    let c1_oid = make_commit_with_file(&repo, "First commit", "file1.txt");
    make_commit_with_file(&repo, "Second commit", "file2.txt");

    // Make a working tree change
    let workdir = repo.workdir().unwrap();
    fs::write(workdir.join("file2.txt"), "modified content").unwrap();

    // Reword should handle working tree changes (via --autostash)
    let result = super::reword_commit(&repo, &c1_oid.to_string(), Some("Updated first".to_string()));

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword with working tree changes: {:?}", result);

    // Verify the working tree change is still there
    let content = fs::read_to_string(workdir.join("file2.txt")).unwrap();
    assert_eq!(content, "modified content", "Working tree changes should be preserved");
}

#[test]

fn reword_branch_by_name() {
    // Test: Rename a branch using git branch -m
    // Expected: Old branch name disappears, new branch name exists,
    // branch still points to same commit

    let (repo, _dir) = setup_simple_repo();

    // Create a branch
    let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("feature-old", &head_commit, false).unwrap();

    // Rename the branch using reword_branch
    let result = super::reword_branch(&repo, "feature-old", "feature-new");
    assert!(result.is_ok(), "Failed to rename branch: {:?}", result);

    // Verify old branch doesn't exist
    assert!(repo.find_branch("feature-old", git2::BranchType::Local).is_err(),
            "Old branch should not exist after rename");

    // Verify new branch exists and points to same commit
    let new_branch = repo.find_branch("feature-new", git2::BranchType::Local).unwrap();
    assert_eq!(new_branch.get().target().unwrap(), head_commit.id(),
               "New branch should point to same commit");
}

#[test]

fn reword_current_branch() {
    // Test: Rename the currently checked out branch
    // Expected: Branch rename succeeds, HEAD still tracks the renamed branch

    let (repo, _dir) = setup_simple_repo();

    // The default branch is "main" or "master" - rename it
    let current_branch_name = repo.head().unwrap().shorthand().unwrap().to_string();

    let result = super::reword_branch(&repo, &current_branch_name, "renamed-main");
    assert!(result.is_ok(), "Failed to rename current branch: {:?}", result);

    // Verify HEAD is still on the renamed branch
    let new_head = repo.head().unwrap();
    assert!(new_head.is_branch(), "HEAD should still be on a branch");
    assert_eq!(new_head.shorthand().unwrap(), "renamed-main",
               "HEAD should track renamed branch");
}

#[test]

fn reword_commit_with_partial_hash() {
    // Test: Reword using a partial (7-character) commit hash
    // Expected: Git resolves the partial hash and rewording succeeds

    let (repo, _dir) = setup_simple_repo();

    let c1_oid = make_commit_with_file(&repo, "First commit", "file1.txt");
    make_commit_with_file(&repo, "Second commit", "file2.txt");

    // Use partial hash (first 7 characters)
    let partial_hash = &c1_oid.to_string()[..7];
    let result = super::reword_commit(&repo, partial_hash, Some("Updated via partial hash".to_string()));

    if result.is_err() {
        eprintln!("Note: This test may fail on Windows due to PowerShell sequence editor issues");
        eprintln!("Error: {:?}", result);
    }
    assert!(result.is_ok(), "Failed to reword commit with partial hash: {:?}", result);

    // Verify the commit message changed
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let parent = head.parent(0).unwrap();
    assert_eq!(parent.message().unwrap().trim(), "Updated via partial hash");
}

#[test]

fn reword_nonexistent_commit_fails() {
    // Test: Attempt to reword a commit that doesn't exist
    // Expected: Error during git rebase (invalid revision)

    let (repo, _dir) = setup_simple_repo();

    // Try to reword a commit that doesn't exist
    let result = super::reword_commit(&repo, "0000000000000000000000000000000000000000", Some("New message".to_string()));

    assert!(result.is_err(), "Should fail on nonexistent commit");
}

#[test]

fn reword_nonexistent_branch_fails() {
    // Test: Attempt to rename a branch that doesn't exist
    // Expected: Error from git branch -m

    let (repo, _dir) = setup_simple_repo();

    // Try to rename a branch that doesn't exist
    let result = super::reword_branch(&repo, "nonexistent-branch", "new-name");

    assert!(result.is_err(), "Should fail on nonexistent branch");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Failed to rename branch"),
            "Error should mention branch rename failure");
}
