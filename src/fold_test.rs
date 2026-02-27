use crate::git;
use crate::git_commands::{self, git_branch, git_commit, git_merge};
use crate::test_helpers::TestRepo;

// ── Case 1: File(s) + Commit (Amend) ────────────────────────────────────

#[test]
fn fold_file_into_head() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Modify a file without committing
    test_repo.write_file("file1.txt", "modified content");

    let head_oid = test_repo.head_oid();

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &head_oid.to_string(),
    );

    assert!(
        result.is_ok(),
        "fold_files_into_commit failed: {:?}",
        result
    );

    // HEAD should have been amended (same message, different hash)
    assert_eq!(test_repo.get_message(0), "Second commit");
    assert_ne!(test_repo.head_oid(), head_oid, "Hash should have changed");

    // The file content should be in the commit now
    assert_eq!(test_repo.read_file("file1.txt"), "modified content");
}

#[test]
fn fold_multiple_files_into_head() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");

    // Modify multiple files
    test_repo.write_file("file1.txt", "modified 1");
    test_repo.write_file("new_file.txt", "new content");

    let head_oid = test_repo.head_oid();

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string(), "new_file.txt".to_string()],
        &head_oid.to_string(),
    );

    assert!(result.is_ok(), "fold failed: {:?}", result);
    assert_eq!(test_repo.get_message(0), "First commit");
    assert_eq!(test_repo.read_file("file1.txt"), "modified 1");
    assert_eq!(test_repo.read_file("new_file.txt"), "new content");
}

#[test]
fn fold_file_into_non_head_commit() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Modify file1.txt (which was introduced in first commit)
    test_repo.write_file("file1.txt", "amended content");

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &c1_oid.to_string(),
    );

    assert!(
        result.is_ok(),
        "fold_files_into_commit failed: {:?}",
        result
    );

    // Messages should be preserved
    assert_eq!(test_repo.get_message(0), "Second commit");
    assert_eq!(test_repo.get_message(1), "First commit");

    // The first commit's hash should have changed
    assert_ne!(test_repo.get_oid(1), c1_oid);
}

#[test]
fn fold_file_no_changes_fails() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");

    let head_oid = test_repo.head_oid();

    // file1.txt has no uncommitted changes
    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &head_oid.to_string(),
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no changes"));
}

#[test]
fn fold_file_into_non_head_with_other_changes_autostashed() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Modify two files but only fold one
    test_repo.write_file("file1.txt", "change 1");
    test_repo.write_file("file2.txt", "change 2");

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &c1_oid.to_string(),
    );

    assert!(
        result.is_ok(),
        "fold should succeed with autostash: {:?}",
        result
    );

    // Other dirty file should be preserved after autostash
    assert_eq!(test_repo.read_file("file2.txt"), "change 2");
}

/// Bug: folding a file into a woven branch commit (non-HEAD) where both the
/// commit and working-tree modify the same file would leave unmerged paths.
/// The autostash would pop stale changes that conflict with rewritten history.
#[test]
fn fold_file_into_woven_branch_commit() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature branch with a commit that modifies feature1
    git_branch::create(workdir.as_path(), "feat1", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feat1");
    test_repo.write_file("feature1", "initial feature content");
    git_commit::stage_files(workdir.as_path(), &["feature1"]).unwrap();
    git_commit::commit(workdir.as_path(), "Feature 1").unwrap();
    let feat1_oid = test_repo.head_oid();

    // Merge feat1 into integration branch
    test_repo.switch_branch("integration");
    git_merge::merge_no_ff(workdir.as_path(), "feat1").unwrap();

    // Modify feature1 in the working tree (same file as the commit)
    test_repo.write_file("feature1", "updated feature content");

    // Fold the working-tree changes into the feat1 commit
    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["feature1".to_string()],
        &feat1_oid.to_string(),
    );

    assert!(
        result.is_ok(),
        "fold_files_into_commit (woven branch) failed: {:?}",
        result
    );

    // The feat1 commit should have been rewritten
    let feat1_new_tip = test_repo.get_branch_target("feat1");
    assert_ne!(feat1_new_tip, feat1_oid, "feat1 should have been rewritten");

    // The file should have the updated content (now in the commit)
    assert_eq!(test_repo.read_file("feature1"), "updated feature content");

    // There should be no unmerged paths
    let status_output =
        git_commands::run_git_stdout(workdir.as_path(), &["status", "--porcelain"]).unwrap();
    assert!(
        !status_output.contains("UU") && !status_output.contains("AA"),
        "Working tree should have no merge conflicts, but status shows:\n{}",
        status_output
    );
}

// ── Case 2: Commit + Commit (Fixup) ─────────────────────────────────────

#[test]
fn fold_commit_into_earlier_commit() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("Original feature", "feature.txt");
    let c2_oid = test_repo.commit("Fix typo in feature", "feature.txt");

    let result =
        super::fold_commit_into_commit(&test_repo.repo, &c2_oid.to_string(), &c1_oid.to_string());

    assert!(
        result.is_ok(),
        "fold_commit_into_commit failed: {:?}",
        result
    );

    // Only one commit should remain (plus the initial commit)
    assert_eq!(test_repo.get_message(0), "Original feature");

    // The source commit should be gone (HEAD is now the target commit)
    // Hash should be different (rewritten)
    assert_ne!(test_repo.head_oid(), c1_oid);
    assert_ne!(test_repo.head_oid(), c2_oid);
}

#[test]
fn fold_commit_into_commit_preserves_other_commits() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("First", "file1.txt");
    test_repo.commit("Second", "file2.txt");
    let c3_oid = test_repo.commit("Fix for first", "file1.txt");

    // Fold c3 into c1 (c3 is the fixup that should be part of c1)
    let result =
        super::fold_commit_into_commit(&test_repo.repo, &c3_oid.to_string(), &c1_oid.to_string());

    assert!(result.is_ok(), "fold failed: {:?}", result);

    // Should have 2 commits now (initial + First + Second; "Fix for first" absorbed)
    assert_eq!(test_repo.get_message(0), "Second");
    assert_eq!(test_repo.get_message(1), "First");
}

#[test]
fn fold_commit_same_commit_fails() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit("First", "file1.txt");

    let result =
        super::fold_commit_into_commit(&test_repo.repo, &c1_oid.to_string(), &c1_oid.to_string());

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("same commit"));
}

#[test]
fn fold_commit_wrong_direction_fails() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit("First", "file1.txt");
    let c2_oid = test_repo.commit("Second", "file2.txt");

    // Try to fold the older commit into the newer one (wrong direction)
    let result =
        super::fold_commit_into_commit(&test_repo.repo, &c1_oid.to_string(), &c2_oid.to_string());

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("newer than target")
    );
}

#[test]
fn fold_commit_dirty_working_tree_autostashed() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("First", "file1.txt");
    let c2_oid = test_repo.commit("Second", "file2.txt");

    // Dirty the working tree
    test_repo.write_file("file1.txt", "dirty");

    let result =
        super::fold_commit_into_commit(&test_repo.repo, &c2_oid.to_string(), &c1_oid.to_string());

    assert!(
        result.is_ok(),
        "fold should succeed with autostash: {:?}",
        result
    );

    // Dirty changes should be preserved after autostash
    assert_eq!(test_repo.read_file("file1.txt"), "dirty");
}

// ── Case 3: Commit + Branch (Move) ──────────────────────────────────────

#[test]
fn fold_commit_to_branch() {
    // Set up an integration branch with two woven feature branches:
    //   origin/main → A1 (feature-a)
    //              ↘              ↘
    //               B1 --------→ merge → C1 (loose, to be moved)
    // After fold C1 to feature-a:
    //   origin/main → A1 → C1 (feature-a)
    //              ↘              ↘
    //               B1 --------→ merge
    let test_repo = TestRepo::new_with_remote();

    // Create commits for feature-a
    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();

    // Create feature-a branch and weave it
    let workdir = test_repo.workdir();
    git_branch::create(workdir.as_path(), "feature-a", &a1_oid.to_string()).unwrap();

    let base_oid = test_repo.find_remote_branch_target("origin/main");
    // Add a commit on integration line before merge
    test_repo.commit("B1", "b1.txt");

    // Manually set up merge topology:
    // Rebase B1 onto merge-base, then merge feature-a
    git_commands::git_rebase::rebase_onto(
        workdir.as_path(),
        &base_oid.to_string(),
        &a1_oid.to_string(),
    )
    .unwrap();
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();

    // Now add a loose commit on the integration line
    test_repo.commit("C1", "c1.txt");
    let c1_oid = test_repo.head_oid();

    // Move C1 to feature-a
    let result = super::fold_commit_to_branch(&test_repo.repo, &c1_oid.to_string(), "feature-a");

    assert!(result.is_ok(), "fold_commit_to_branch failed: {:?}", result);

    // C1 should now be on feature-a's branch (feature-a tip should have C1's message)
    let feature_a_oid = test_repo.get_branch_target("feature-a");
    let feature_a_commit = test_repo.find_commit(feature_a_oid);
    assert_eq!(
        feature_a_commit.summary().unwrap_or(""),
        "C1",
        "C1 should now be at the tip of feature-a"
    );
}

#[test]
fn fold_commit_to_branch_dirty_autostashed() {
    // Set up an integration branch with a woven feature branch and a loose commit
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();

    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();

    git_branch::create(workdir.as_path(), "feature-a", &a1_oid.to_string()).unwrap();

    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.commit("B1", "b1.txt");

    git_commands::git_rebase::rebase_onto(
        workdir.as_path(),
        &base_oid.to_string(),
        &a1_oid.to_string(),
    )
    .unwrap();
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();

    let loose_oid = test_repo.commit("Loose", "loose.txt");

    // Dirty the working tree
    test_repo.write_file("a1.txt", "dirty");

    let result = super::fold_commit_to_branch(&test_repo.repo, &loose_oid.to_string(), "feature-a");

    assert!(
        result.is_ok(),
        "fold should succeed with autostash: {:?}",
        result
    );

    // Dirty changes should be preserved after autostash
    assert_eq!(test_repo.read_file("a1.txt"), "dirty");
}

#[test]
fn fold_commit_to_colocated_branch_only_affects_target() {
    // Reproduce: two co-located woven branches (feat2 and feat3 sharing the same
    // merge commit), plus a third branch (test) with commits.
    // Moving a commit from 'test' to 'feat3' should put it only on feat3,
    // NOT on feat2.
    //
    // Before:
    //   ╭─ [feat3]
    //   ├─ [feat2]
    //   ●  Feat2
    //   ╯
    //   ╭─ [test]
    //   ●  Feat3
    //   ●  Feat1
    //   ╯
    //
    // After fold Feat3 → feat3:
    //   ╭─ [feat3]
    //   ●  Feat3    ← only on feat3
    //   ├─ [feat2]
    //   ●  Feat2
    //   ╯
    //   ╭─ [test]
    //   ●  Feat1
    //   ╯
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Build the 'test' branch with two commits: Feat1 and Feat3
    git_branch::create(workdir.as_path(), "test", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("test");
    test_repo.commit("Feat1", "feat1.txt");
    test_repo.commit("Feat3", "feat3.txt");
    let feat3_oid = test_repo.head_oid();
    test_repo.switch_branch("integration");

    // Weave the 'test' branch
    git_merge::merge_no_ff(workdir.as_path(), "test").unwrap();

    // Build the 'feat2' branch with one commit: Feat2
    git_branch::create(workdir.as_path(), "feat2", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feat2");
    test_repo.commit("Feat2", "feat2.txt");
    test_repo.switch_branch("integration");

    // Create feat3 as co-located with feat2 (same tip)
    let feat2_tip = test_repo.get_branch_target("feat2");
    git_branch::create(workdir.as_path(), "feat3", &feat2_tip.to_string()).unwrap();

    // Weave feat2 (which also brings in feat3 since they're co-located)
    git_merge::merge_no_ff(workdir.as_path(), "feat2").unwrap();

    // Now move the Feat3 commit from 'test' branch to 'feat3' branch
    let result = super::fold_commit_to_branch(&test_repo.repo, &feat3_oid.to_string(), "feat3");

    assert!(result.is_ok(), "fold_commit_to_branch failed: {:?}", result);

    // feat3 should have Feat3 at its tip (above feat2)
    let feat3_tip = test_repo.get_branch_target("feat3");
    let feat3_commit = test_repo.find_commit(feat3_tip);
    assert_eq!(
        feat3_commit.summary().unwrap_or(""),
        "Feat3",
        "feat3 tip should be Feat3"
    );

    // feat2 should still have Feat2 at its tip (NOT Feat3)
    let feat2_tip = test_repo.get_branch_target("feat2");
    let feat2_commit = test_repo.find_commit(feat2_tip);
    assert_eq!(
        feat2_commit.summary().unwrap_or(""),
        "Feat2",
        "feat2 tip should still be Feat2, not Feat3"
    );

    // feat3 should be stacked on feat2: feat3's parent should be feat2's tip
    assert_eq!(
        feat3_commit.parent_id(0).unwrap(),
        feat2_tip,
        "feat3 should be stacked on feat2"
    );
}

// ── Type dispatch / classify tests ───────────────────────────────────────

#[test]
fn classify_files_into_commit() {
    let sources = vec![git::Target::File("f1.txt".into())];
    let target = git::Target::Commit("abc123".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
}

#[test]
fn classify_commit_into_commit() {
    let sources = vec![git::Target::Commit("abc123".into())];
    let target = git::Target::Commit("def456".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
}

#[test]
fn classify_commit_into_branch() {
    let sources = vec![git::Target::Commit("abc123".into())];
    let target = git::Target::Branch("feature-a".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
}

#[test]
fn classify_branch_source_rejected() {
    let sources = vec![git::Target::Branch("feature-a".into())];
    let target = git::Target::Commit("abc123".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Cannot fold a branch")
    );
}

#[test]
fn classify_files_into_branch_rejected() {
    let sources = vec![git::Target::File("f1.txt".into())];
    let target = git::Target::Branch("feature-a".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Cannot fold files into a branch")
    );
}

#[test]
fn classify_mixed_sources_rejected() {
    let sources = vec![
        git::Target::File("f1.txt".into()),
        git::Target::Commit("abc123".into()),
    ];
    let target = git::Target::Commit("def456".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Cannot mix"));
}

#[test]
fn classify_multiple_commit_sources_rejected() {
    let sources = vec![
        git::Target::Commit("abc123".into()),
        git::Target::Commit("def456".into()),
    ];
    let target = git::Target::Commit("ghi789".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Only one commit"));
}

// ── Case 4: Commit + Unstaged (Uncommit) ─────────────────────────────────

#[test]
fn fold_commit_to_unstaged_head() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    let head_oid = test_repo.head_oid();

    let result = super::fold_commit_to_unstaged(&test_repo.repo, &head_oid.to_string());

    assert!(
        result.is_ok(),
        "fold_commit_to_unstaged failed: {:?}",
        result
    );

    // HEAD should now be "First commit"
    assert_eq!(test_repo.get_message(0), "First commit");

    // file2.txt should exist in working directory as unstaged change
    assert_eq!(test_repo.read_file("file2.txt"), "Second commit");

    // The old HEAD should be gone
    assert_ne!(test_repo.head_oid(), head_oid);
}

#[test]
fn fold_commit_to_unstaged_non_head() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    let result = super::fold_commit_to_unstaged(&test_repo.repo, &c1_oid.to_string());

    assert!(
        result.is_ok(),
        "fold_commit_to_unstaged (non-HEAD) failed: {:?}",
        result
    );

    // Only "Second commit" should remain
    assert_eq!(test_repo.get_message(0), "Second commit");

    // file1.txt should be in the working directory as unstaged
    assert_eq!(test_repo.read_file("file1.txt"), "First commit");
}

#[test]
fn fold_commit_to_unstaged_dirty_autostashed() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Dirty the working tree with an unrelated change
    test_repo.write_file("file1.txt", "dirty");

    let head_oid = test_repo.head_oid();

    let result = super::fold_commit_to_unstaged(&test_repo.repo, &head_oid.to_string());

    assert!(
        result.is_ok(),
        "fold should succeed with dirty tree: {:?}",
        result
    );

    // HEAD should now be "First commit"
    assert_eq!(test_repo.get_message(0), "First commit");

    // Existing dirty changes should be preserved
    assert_eq!(test_repo.read_file("file1.txt"), "dirty");

    // Uncommitted changes should appear
    assert_eq!(test_repo.read_file("file2.txt"), "Second commit");
}

#[test]
fn classify_commit_into_unstaged() {
    let sources = vec![git::Target::Commit("abc123".into())];
    let target = git::Target::Unstaged;
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap(),
        super::FoldOp::CommitToUnstaged { .. }
    ));
}

#[test]
fn classify_files_into_unstaged_rejected() {
    let sources = vec![git::Target::File("f1.txt".into())];
    let target = git::Target::Unstaged;
    let result = super::classify(&sources, &target);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Cannot fold files into unstaged")
    );
}

#[test]
fn classify_unstaged_source_rejected() {
    let sources = vec![git::Target::Unstaged];
    let target = git::Target::Commit("abc123".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Cannot fold unstaged changes")
    );
}

// ── Case 5: CommitFile + Unstaged (Uncommit file) ─────────────────────────

#[test]
fn fold_commit_file_to_unstaged_head() {
    let test_repo = TestRepo::new();
    let workdir = test_repo.workdir();

    // Commit has two files (use CLI for consistent index)
    test_repo.write_file("file1.txt", "content1");
    test_repo.write_file("file2.txt", "content2");
    git_commit::stage_files(workdir.as_path(), &["file1.txt", "file2.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Two files").unwrap();

    let head_oid = test_repo.head_oid();

    let result =
        super::fold_commit_file_to_unstaged(&test_repo.repo, &head_oid.to_string(), "file1.txt");

    assert!(
        result.is_ok(),
        "fold_commit_file_to_unstaged failed: {:?}",
        result
    );

    // The commit should still exist but only contain file2.txt
    assert_eq!(test_repo.get_message(0), "Two files");

    // file1.txt should be in working directory (unstaged)
    assert_eq!(test_repo.read_file("file1.txt"), "content1");

    // file2.txt should still be committed
    assert_eq!(test_repo.read_file("file2.txt"), "content2");
}

#[test]
fn fold_commit_file_to_unstaged_non_head() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();

    // First commit has two files (use CLI for staging to avoid libgit2 index mismatch)
    test_repo.write_file("file1.txt", "content1");
    test_repo.write_file("file2.txt", "content2");
    git_commit::stage_files(workdir.as_path(), &["file1.txt", "file2.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Two files").unwrap();
    let c1_oid = test_repo.head_oid();

    // Second commit (also via CLI to keep index consistent)
    test_repo.write_file("file3.txt", "content3");
    git_commit::stage_files(workdir.as_path(), &["file3.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Second commit").unwrap();

    let result =
        super::fold_commit_file_to_unstaged(&test_repo.repo, &c1_oid.to_string(), "file1.txt");

    assert!(
        result.is_ok(),
        "fold_commit_file_to_unstaged (non-HEAD) failed: {:?}",
        result
    );

    // Both commits should still exist
    assert_eq!(test_repo.get_message(0), "Second commit");
    assert_eq!(test_repo.get_message(1), "Two files");

    // file1.txt should be in working directory as unstaged changes
    assert_eq!(test_repo.read_file("file1.txt"), "content1");

    // file2.txt should still be committed
    assert_eq!(test_repo.read_file("file2.txt"), "content2");
}

#[test]
fn fold_commit_file_to_unstaged_no_changes_fails() {
    let test_repo = TestRepo::new();
    test_repo.commit("A commit", "file1.txt");
    let head_oid = test_repo.head_oid();

    // Try to uncommit a file that doesn't exist in the commit
    let result = super::fold_commit_file_to_unstaged(
        &test_repo.repo,
        &head_oid.to_string(),
        "nonexistent.txt",
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no changes"));
}

// ── Case 6: CommitFile + Commit (Move file between commits) ──────────────

#[test]
fn fold_commit_file_to_commit() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();

    // First commit has two files (use CLI for staging)
    test_repo.write_file("file1.txt", "content1");
    test_repo.write_file("file2.txt", "content2");
    git_commit::stage_files(workdir.as_path(), &["file1.txt", "file2.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Source commit").unwrap();
    let source_oid = test_repo.head_oid();

    // Second commit (target, also via CLI)
    test_repo.write_file("file3.txt", "content3");
    git_commit::stage_files(workdir.as_path(), &["file3.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Target commit").unwrap();
    let target_oid = test_repo.head_oid();

    let result = super::fold_commit_file_to_commit(
        &test_repo.repo,
        &source_oid.to_string(),
        "file1.txt",
        &target_oid.to_string(),
    );

    assert!(
        result.is_ok(),
        "fold_commit_file_to_commit failed: {:?}",
        result
    );

    // Both commits should still exist
    assert_eq!(test_repo.get_message(0), "Target commit");
    assert_eq!(test_repo.get_message(1), "Source commit");

    // file1.txt should still exist in the repo (now in target commit)
    assert_eq!(test_repo.read_file("file1.txt"), "content1");
    // file2.txt should still be in the source commit
    assert_eq!(test_repo.read_file("file2.txt"), "content2");
}

#[test]
fn fold_commit_file_to_commit_same_commit_fails() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit("A commit", "file1.txt");

    let result = super::fold_commit_file_to_commit(
        &test_repo.repo,
        &c1_oid.to_string(),
        "file1.txt",
        &c1_oid.to_string(),
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("same commit"));
}

/// Bug: moving a file from a newer commit to an older commit should
/// remove the file from source and add it to target. Previously, the
/// reverse fixup was incorrectly applied to the source, creating a
/// backwards change instead of a no-op.
#[test]
fn fold_commit_file_to_older_commit() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();

    // C1 (older, target): adds file_a.txt
    test_repo.write_file("file_a.txt", "aaa");
    git_commit::stage_files(workdir.as_path(), &["file_a.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Add file_a").unwrap();
    let c1_oid = test_repo.head_oid();

    // C2 (newer, source): adds file_b.txt and modifies file_a.txt
    test_repo.write_file("file_a.txt", "aaa modified");
    test_repo.write_file("file_b.txt", "bbb");
    git_commit::stage_files(workdir.as_path(), &["file_a.txt", "file_b.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Add file_b and modify file_a").unwrap();
    let c2_oid = test_repo.head_oid();

    // Move file_a.txt changes from C2 (newer) to C1 (older)
    let result = super::fold_commit_file_to_commit(
        &test_repo.repo,
        &c2_oid.to_string(),
        "file_a.txt",
        &c1_oid.to_string(),
    );

    assert!(
        result.is_ok(),
        "fold_commit_file_to_commit (newer→older) failed: {:?}",
        result
    );

    // Both commits should still exist
    assert_eq!(test_repo.get_message(0), "Add file_b and modify file_a");
    assert_eq!(test_repo.get_message(1), "Add file_a");

    // C1 should now include the file_a modification
    // Final state should have file_a.txt as "aaa modified"
    assert_eq!(test_repo.read_file("file_a.txt"), "aaa modified");

    // file_b.txt should still be in C2
    assert_eq!(test_repo.read_file("file_b.txt"), "bbb");

    // Key assertion: C2's diff should NOT contain a reverse change to file_a.txt.
    // Verify by checking that C2's diff only touches file_b.txt.
    let c2_diff =
        git_commands::diff_commit(workdir.as_path(), &test_repo.head_oid().to_string()).unwrap();
    assert!(
        !c2_diff.contains("file_a.txt"),
        "C2 should no longer have any changes to file_a.txt, but diff contains:\n{}",
        c2_diff
    );
}

/// Bug: fixupping a commit in a stacked branch should preserve the
/// middle branch's ref. Previously, `update-ref` was emitted before
/// the fixup in the todo, causing the branch to point to a replaced commit.
#[test]
fn fold_commit_file_to_unstaged_stacked_branch() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Build feature-a on its own branch
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-a");

    test_repo.write_file("fa1.txt", "feature-a file 1");
    test_repo.write_file("fa2.txt", "feature-a file 2");
    git_commit::stage_files(workdir.as_path(), &["fa1.txt", "fa2.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "A1: two files").unwrap();

    // Build feature-b stacked on feature-a
    git_branch::create(
        workdir.as_path(),
        "feature-b",
        &test_repo.head_oid().to_string(),
    )
    .unwrap();
    test_repo.switch_branch("feature-b");

    test_repo.write_file("fb1.txt", "feature-b file 1");
    git_commit::stage_files(workdir.as_path(), &["fb1.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "B1: one file").unwrap();

    // Switch to integration and merge feature-b (includes A1 and B1)
    test_repo.switch_branch("integration");
    git_merge::merge_no_ff(workdir.as_path(), "feature-b").unwrap();

    // Verify setup: both branches exist, working tree has all files
    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a should exist before fold"
    );
    assert!(
        test_repo.branch_exists("feature-b"),
        "feature-b should exist before fold"
    );
    assert_eq!(test_repo.read_file("fa1.txt"), "feature-a file 1");

    // Get feature-a's tip (A1)
    let fa_tip = test_repo.get_branch_target("feature-a");

    // Uncommit fa1.txt from the A1 commit
    let result =
        super::fold_commit_file_to_unstaged(&test_repo.repo, &fa_tip.to_string(), "fa1.txt");

    assert!(
        result.is_ok(),
        "fold_commit_file_to_unstaged (stacked branch) failed: {:?}",
        result
    );

    // Key assertion: feature-a branch must still exist
    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a branch should still exist after fold"
    );

    // feature-b should also still exist
    assert!(
        test_repo.branch_exists("feature-b"),
        "feature-b branch should still exist after fold"
    );

    // fa1.txt should be in the working directory (unstaged)
    assert_eq!(test_repo.read_file("fa1.txt"), "feature-a file 1");

    // fa2.txt should still be committed
    assert_eq!(test_repo.read_file("fa2.txt"), "feature-a file 2");

    // fb1.txt should still be committed
    assert_eq!(test_repo.read_file("fb1.txt"), "feature-b file 1");
}

/// Bug: moving a file between commits in a stacked branch should
/// preserve all branch refs. Tests moving from a newer branch commit
/// to an older branch commit (which triggers the direction bug).
#[test]
fn fold_commit_file_to_commit_stacked_branch() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Build feature-a branch
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-a");

    test_repo.write_file("fa1.txt", "feature-a file 1");
    git_commit::stage_files(workdir.as_path(), &["fa1.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "A1").unwrap();
    let a1_oid = test_repo.head_oid();

    // Build feature-b stacked on feature-a
    git_branch::create(workdir.as_path(), "feature-b", &a1_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-b");

    test_repo.write_file("fb1.txt", "feature-b file 1");
    test_repo.write_file("fb2.txt", "feature-b file 2");
    git_commit::stage_files(workdir.as_path(), &["fb1.txt", "fb2.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "B1").unwrap();
    let b1_oid = test_repo.head_oid();

    // Merge into integration
    test_repo.switch_branch("integration");
    git_merge::merge_no_ff(workdir.as_path(), "feature-b").unwrap();

    // Move fb1.txt from B1 (newer) to A1 (older)
    let result = super::fold_commit_file_to_commit(
        &test_repo.repo,
        &b1_oid.to_string(),
        "fb1.txt",
        &a1_oid.to_string(),
    );

    assert!(
        result.is_ok(),
        "fold_commit_file_to_commit (stacked branch) failed: {:?}",
        result
    );

    // Both branches should still exist
    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a should still exist after fold"
    );
    assert!(
        test_repo.branch_exists("feature-b"),
        "feature-b should still exist after fold"
    );

    // fb1.txt should have moved to feature-a (A1)
    assert_eq!(test_repo.read_file("fb1.txt"), "feature-b file 1");

    // B1 should no longer have fb1.txt changes
    let b_tip = test_repo.get_branch_target("feature-b");
    let b_diff = git_commands::diff_commit(workdir.as_path(), &b_tip.to_string()).unwrap();
    assert!(
        !b_diff.contains("fb1.txt"),
        "B1 should no longer have fb1.txt changes, but diff contains:\n{}",
        b_diff
    );
    assert!(
        b_diff.contains("fb2.txt"),
        "B1 should still have fb2.txt changes"
    );

    // Key assertion for Bug 1: feature-a's ref should point to the correct
    // commit (the one with fb1.txt included), not the pre-fixup version.
    let a_tip = test_repo.get_branch_target("feature-a");
    let a_diff = git_commands::diff_commit(workdir.as_path(), &a_tip.to_string()).unwrap();
    assert!(
        a_diff.contains("fb1.txt"),
        "feature-a (A1) should now include fb1.txt, but diff is:\n{}",
        a_diff
    );
    assert!(
        a_diff.contains("fa1.txt"),
        "feature-a (A1) should still include fa1.txt"
    );
}

/// Bug: moving a file between commits in a multi-branch woven topology
/// should preserve all branch refs and not produce rebase conflicts.
/// Matches the real-world scenario: foo2 has one commit, foo3 is stacked
/// on foo2 with another commit. Both are woven via a single merge of foo3.
/// Moving a file from foo3's commit (newer) to foo2's commit (older)
/// exercises both the two-phase rebase and the update-ref deferral.
#[test]
fn fold_commit_file_to_commit_woven_branches() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Build foo1 branch with one commit
    git_branch::create(workdir.as_path(), "foo1", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("foo1");
    test_repo.write_file("feature1", "feat 1");
    git_commit::stage_files(workdir.as_path(), &["feature1"]).unwrap();
    git_commit::commit(workdir.as_path(), "Feature 1").unwrap();
    test_repo.switch_branch("integration");
    git_merge::merge_no_ff(workdir.as_path(), "foo1").unwrap();

    // Build foo2 branch with one commit (on base, not on integration)
    git_branch::create(workdir.as_path(), "foo2", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("foo2");
    test_repo.write_file("feature2", "feat 2");
    git_commit::stage_files(workdir.as_path(), &["feature2"]).unwrap();
    git_commit::commit(workdir.as_path(), "Feature 2").unwrap();
    let foo2_tip = test_repo.head_oid();

    // Build foo3 stacked on foo2 with one commit that modifies feature2 and adds feature7
    git_branch::create(workdir.as_path(), "foo3", &foo2_tip.to_string()).unwrap();
    test_repo.switch_branch("foo3");
    test_repo.write_file("feature2", "feat 2 updated");
    test_repo.write_file("feature7", "feat 7");
    git_commit::stage_files(workdir.as_path(), &["feature2", "feature7"]).unwrap();
    git_commit::commit(workdir.as_path(), "Feature 2 fixup").unwrap();
    let foo3_tip = test_repo.head_oid();

    // Only merge foo3 into integration (brings in both foo2 and foo3 commits)
    test_repo.switch_branch("integration");
    git_merge::merge_no_ff(workdir.as_path(), "foo3").unwrap();

    // Now move 'feature7' from foo3 tip (newer) to foo2 tip (older)
    let result = super::fold_commit_file_to_commit(
        &test_repo.repo,
        &foo3_tip.to_string(),
        "feature7",
        &foo2_tip.to_string(),
    );

    assert!(
        result.is_ok(),
        "fold_commit_file_to_commit (woven branches) failed: {:?}",
        result
    );

    // All branches should still exist
    assert!(test_repo.branch_exists("foo1"), "foo1 should still exist");
    assert!(test_repo.branch_exists("foo2"), "foo2 should still exist");
    assert!(test_repo.branch_exists("foo3"), "foo3 should still exist");

    // feature7 should be accessible (moved to foo2's commit)
    assert_eq!(test_repo.read_file("feature7"), "feat 7");

    // foo2's commit should now include feature7
    let foo2_new_tip = test_repo.get_branch_target("foo2");
    let foo2_diff =
        git_commands::diff_commit(workdir.as_path(), &foo2_new_tip.to_string()).unwrap();
    assert!(
        foo2_diff.contains("feature7"),
        "foo2 should now include feature7, but diff is:\n{}",
        foo2_diff
    );

    // foo3's commit should no longer include feature7
    let foo3_new_tip = test_repo.get_branch_target("foo3");
    let foo3_diff =
        git_commands::diff_commit(workdir.as_path(), &foo3_new_tip.to_string()).unwrap();
    assert!(
        !foo3_diff.contains("feature7"),
        "foo3 should no longer include feature7, but diff contains:\n{}",
        foo3_diff
    );
    // foo3 should still have its other change (feature2 modification)
    assert!(
        foo3_diff.contains("feature2"),
        "foo3 should still have feature2 changes"
    );
}

// ── CommitFile classify tests ────────────────────────────────────────────

#[test]
fn classify_commit_file_into_unstaged() {
    let sources = vec![git::Target::CommitFile {
        commit: "abc123".into(),
        path: "file.txt".into(),
    }];
    let target = git::Target::Unstaged;
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap(),
        super::FoldOp::CommitFileToUnstaged { .. }
    ));
}

#[test]
fn classify_commit_file_into_commit() {
    let sources = vec![git::Target::CommitFile {
        commit: "abc123".into(),
        path: "file.txt".into(),
    }];
    let target = git::Target::Commit("def456".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap(),
        super::FoldOp::CommitFileToCommit { .. }
    ));
}

#[test]
fn classify_commit_file_into_branch_rejected() {
    let sources = vec![git::Target::CommitFile {
        commit: "abc123".into(),
        path: "file.txt".into(),
    }];
    let target = git::Target::Branch("feature-a".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Cannot fold a commit file into a branch")
    );
}

#[test]
fn classify_commit_file_target_rejected() {
    let sources = vec![git::Target::Commit("abc123".into())];
    let target = git::Target::CommitFile {
        commit: "def456".into(),
        path: "file.txt".into(),
    };
    let result = super::classify(&sources, &target);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("not a commit file")
    );
}

// ── Resolve fold arg tests ───────────────────────────────────────────────

#[test]
fn resolve_fold_arg_filesystem_path() {
    let test_repo = TestRepo::new();
    test_repo.commit("commit", "file1.txt");

    // Modify a file — should resolve as Target::File via filesystem fallback
    test_repo.write_file("file1.txt", "changed");

    let result = super::resolve_fold_arg(&test_repo.repo, "file1.txt");
    assert!(result.is_ok(), "resolve failed: {:?}", result);
    assert!(matches!(result.unwrap(), git::Target::File(_)));
}

#[test]
fn resolve_fold_arg_commit_hash() {
    let test_repo = TestRepo::new();
    let c1_oid = test_repo.commit("commit", "file1.txt");

    let result = super::resolve_fold_arg(&test_repo.repo, &c1_oid.to_string());
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), git::Target::Commit(_)));
}

#[test]
fn resolve_fold_arg_branch_name() {
    let test_repo = TestRepo::new();
    test_repo.create_branch("feature-a");

    let result = super::resolve_fold_arg(&test_repo.repo, "feature-a");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), git::Target::Branch(_)));
}

#[test]
fn resolve_fold_arg_head() {
    let test_repo = TestRepo::new();
    test_repo.commit("commit", "file1.txt");

    let result = super::resolve_fold_arg(&test_repo.repo, "HEAD");
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), git::Target::Commit(_)));
}
