// ── Test Helpers ───────────────────────────────────────────────────────

use crate::core::test_helpers::TestRepo;

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
        !test_repo.branch_exists("feature-old"),
        "Old branch should not exist after rename"
    );

    // Verify new branch exists and points to same commit
    assert!(
        test_repo.branch_exists("feature-new"),
        "New branch should exist after rename"
    );
    assert_eq!(
        test_repo.get_branch_target("feature-new"),
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

    // Rename using full branch name
    let result = test_repo.in_dir(|| {
        super::run(
            "feature-original".to_string(),
            Some("feature-renamed".to_string()),
        )
    });

    assert!(
        result.is_ok(),
        "Failed to rename branch via run: {:?}",
        result
    );

    // Verify old branch doesn't exist
    assert!(
        !test_repo.branch_exists("feature-original"),
        "Old branch should not exist after rename"
    );

    // Verify new branch exists
    assert!(
        test_repo.branch_exists("feature-renamed"),
        "New branch should exist after rename"
    );
}

// ── Conflict pause / continue / abort ──────────────────────────────────

/// Build a woven integration branch whose second merge had to be resolved by
/// hand — the shape that makes a reword conflict.
///
/// Both feature branches insert a line into the same empty gap, so merging the
/// second one conflicts. Rewording below the merges forces git to rebuild them,
/// and with no rerere entry the merge conflicts again.
///
/// Returns the OID of `feature-a`'s only commit, the reword target.
fn woven_repo_with_hand_resolved_merge(test_repo: &TestRepo) -> git2::Oid {
    // Keep the machine's own rerere cache out of it: a recorded resolution
    // would be replayed and the reword would not conflict at all.
    test_repo.set_config("rerere.enabled", "false");

    test_repo.write_file("shared.txt", "first\nlast\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Add shared file");
    let shared_base = test_repo.head_oid().to_string();

    test_repo.create_branch_at("feature-a", &shared_base);
    test_repo.switch_branch("feature-a");
    test_repo.write_file("shared.txt", "first\nfrom-a\nlast\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("A1");
    let a1 = test_repo.head_oid();
    test_repo.switch_branch("integration");

    test_repo.create_branch_at("feature-b", &shared_base);
    test_repo.switch_branch("feature-b");
    test_repo.write_file("shared.txt", "first\nfrom-b\nlast\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("B1");
    test_repo.switch_branch("integration");

    test_repo.merge_no_ff("feature-a");

    // The second merge conflicts; resolving it by hand is what leaves a tree no
    // replay can reproduce on its own.
    let workdir = test_repo.workdir();
    let outcome = crate::git::merge_no_ff(&workdir, test_repo.repo.path(), "feature-b").unwrap();
    assert!(
        matches!(outcome, crate::git::MergeOutcome::Conflicted),
        "the second merge should conflict"
    );
    test_repo.write_file("shared.txt", "first\nfrom-a\nfrom-b\nlast\n");
    test_repo.stage_files(&["shared.txt"]);
    crate::git::continue_merge(&workdir, test_repo.repo.path()).unwrap();

    a1
}

fn state_path(test_repo: &TestRepo) -> std::path::PathBuf {
    test_repo.repo.path().join("loom").join("state.json")
}

/// Rewording below a hand-resolved merge makes git rebuild that merge and hit
/// the same conflict. The reword must pause with saved state instead of
/// aborting, so `loom continue` can finish it once the conflict is resolved.
#[test]
fn reword_conflict_pauses_and_continues() {
    let test_repo = TestRepo::new_with_remote();
    let a1 = woven_repo_with_hand_resolved_merge(&test_repo);

    let result = super::reword_commit(
        &test_repo.repo,
        &a1.to_string(),
        Some("A1 reworded".to_string()),
    );
    assert!(
        result.is_ok(),
        "reword should pause, not fail: {:?}",
        result
    );
    assert!(
        state_path(&test_repo).exists(),
        "loom state must exist while the reword is paused"
    );
    assert!(
        crate::git::rebase_is_in_progress(test_repo.repo.path()),
        "the rebase should still be paused for the user"
    );

    // Resolve the replayed merge the same way it was resolved originally.
    test_repo.write_file("shared.txt", "first\nfrom-a\nfrom-b\nlast\n");
    test_repo.stage_files(&["shared.txt"]);

    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();
    crate::core::transaction::continue_cmd(&workdir, &git_dir).unwrap();

    assert!(
        !state_path(&test_repo).exists(),
        "state must be cleared once the reword completes"
    );
    assert!(!crate::git::rebase_is_in_progress(test_repo.repo.path()));
    assert!(
        test_repo
            .commit_messages()
            .contains(&"A1 reworded".to_string()),
        "reworded message should be in history, got: {:?}",
        test_repo.commit_messages()
    );
    assert_eq!(
        test_repo.read_file("shared.txt"),
        "first\nfrom-a\nfrom-b\nlast\n",
        "the resolution should survive"
    );
    assert_eq!(
        test_repo.head_commit().parent_count(),
        2,
        "integration should still be a merge commit"
    );
}

/// `loom abort` after a paused reword must undo the amend along with the
/// rebase, leaving the original message and refs untouched.
#[test]
fn reword_conflict_abort_restores_original_state() {
    let test_repo = TestRepo::new_with_remote();
    let a1 = woven_repo_with_hand_resolved_merge(&test_repo);
    let original_head = test_repo.head_oid();
    let original_feature_a = test_repo.get_branch_target("feature-a");

    let result = super::reword_commit(
        &test_repo.repo,
        &a1.to_string(),
        Some("A1 reworded".to_string()),
    );
    assert!(
        result.is_ok(),
        "reword should pause, not fail: {:?}",
        result
    );

    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();
    crate::core::transaction::abort_cmd(&workdir, &git_dir).unwrap();

    assert!(!state_path(&test_repo).exists(), "state must be cleared");
    assert!(!crate::git::rebase_is_in_progress(test_repo.repo.path()));
    assert_eq!(
        test_repo.head_oid(),
        original_head,
        "abort must restore the original integration tip"
    );
    assert_eq!(
        test_repo.get_branch_target("feature-a"),
        original_feature_a,
        "abort must restore feature-a"
    );
    assert!(
        !test_repo
            .commit_messages()
            .contains(&"A1 reworded".to_string()),
        "the amend must be undone, got: {:?}",
        test_repo.commit_messages()
    );
}

/// A reword that completes normally must not leave state behind for the paused
/// -operation guard to trip over.
#[test]
fn reword_without_conflict_leaves_no_state() {
    let test_repo = TestRepo::new();
    let c1 = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    super::reword_commit(
        &test_repo.repo,
        &c1.to_string(),
        Some("Reworded first".to_string()),
    )
    .unwrap();

    assert!(
        !state_path(&test_repo).exists(),
        "a clean reword must clear its state file"
    );
}
