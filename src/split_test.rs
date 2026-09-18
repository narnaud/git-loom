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

#[test]
fn split_head_commit_with_absolute_path() {
    let test_repo = TestRepo::new();
    test_repo.commit("Add files", "file1.txt");

    let target_oid = test_repo.commit_multi(
        &[("file_a.txt", "content a"), ("file_b.txt", "content b")],
        "Two files commit",
    );

    let abs = test_repo
        .workdir()
        .join("file_a.txt")
        .to_string_lossy()
        .into_owned();

    let result = test_repo.in_dir(|| {
        super::split_commit_with_selection(
            &test_repo.repo,
            &target_oid.to_string(),
            vec![abs.clone()],
            "First part".to_string(),
        )
    });

    assert!(
        result.is_ok(),
        "split with absolute path failed: {result:?}"
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

/// Splitting off the deletion half of a commit: the split stages a path that
/// is gone from the working tree.
#[test]
fn split_head_commit_with_a_deletion() {
    let test_repo = TestRepo::new();
    test_repo.commit("Add file1", "file1.txt");

    // A commit that deletes file1.txt and adds file2.txt.
    std::fs::remove_file(test_repo.workdir().join("file1.txt")).unwrap();
    test_repo.write_file("file2.txt", "content b");
    test_repo.stage_files(&["file1.txt", "file2.txt"]);
    test_repo.commit_staged("Delete one, add another");

    let target_oid = test_repo.head_oid();
    let result = super::split_commit_with_selection(
        &test_repo.repo,
        &target_oid.to_string(),
        vec!["file1.txt".to_string()],
        "Delete file1".to_string(),
    );

    assert!(result.is_ok(), "split of a deletion failed: {:?}", result);
    assert_eq!(test_repo.get_message(1), "Delete file1");
    assert_eq!(test_repo.get_message(0), "Delete one, add another");
    assert!(!test_repo.commit_has_file(test_repo.get_oid(1), "file1.txt"));
    assert_eq!(
        test_repo.commit_file_paths(test_repo.get_oid(0)),
        vec!["file2.txt"]
    );
    test_repo.assert_working_tree_clean();
}

/// A submodule has to be split by the commit's own diff: `git add` would stage
/// whatever its checkout currently holds. Here the checkout deliberately holds
/// the *pre-image*, so staging by path would drop the bump from history.
#[test]
fn split_by_hunks_takes_a_submodule_from_the_commit() {
    use crate::tui::hunk_selector::{FileEntry, HunkEntry, HunkOrigin};

    let test_repo = TestRepo::new();
    let (first, second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");

    test_repo.checkout_submodule("Data", second);
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["Data", "other.txt"]);
    test_repo.commit_staged("Bump and add");

    // An uncommitted downgrade sitting in the checkout.
    test_repo.checkout_submodule("Data", first);

    let entry = |path: &str, selected: bool, binary: bool| FileEntry {
        path: path.to_string(),
        hunks: vec![HunkEntry {
            hunk: crate::core::diff::DiffHunk {
                text: String::from("(submodule)"),
                modified_lines: vec![],
            },
            selected,
            origin: HunkOrigin::Commit,
        }],
        index_status: 'M',
        worktree_status: ' ',
        binary,
    };
    let selections = [entry("Data", true, true), entry("other.txt", false, false)];

    let workdir = test_repo.workdir();
    let (hash1, _hash2) =
        super::perform_head_split_by_hunks(&workdir, &selections, Some("first"), "second").unwrap();

    let split_off = git2::Oid::from_str(&hash1).unwrap();
    assert_eq!(test_repo.submodule_oid(split_off, "Data"), second);
    assert_eq!(test_repo.get_message(1), "first");
    assert_eq!(test_repo.get_message(0), "second");
}

#[test]
fn split_refuses_when_the_replay_is_dropped() {
    // Same dropped-commit stop as reword, but split's `reset --mixed HEAD~1`
    // would destroy the commit below the target instead of amending it.
    let (t, target) = crate::core::test_helpers::repo_with_dropped_replay();
    let head_before = t.head_oid();
    let base_before = t.find_remote_branch_target("origin/main");

    let err = super::split_commit_with_selection(
        &t.repo,
        &target.to_string(),
        vec!["one.txt".to_string()],
        "First part".to_string(),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(t.head_oid(), head_before, "{err}");
    assert_eq!(
        t.find_remote_branch_target("origin/main"),
        base_before,
        "{err}"
    );
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()), "{err}");
}
