use crate::git;
use crate::test_helpers::TestRepo;
use crate::weave::Weave;

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
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature branch with a commit that modifies feature1
    test_repo.create_branch_at("feat1", &base_oid.to_string());
    test_repo.switch_branch("feat1");
    test_repo.write_file("feature1", "initial feature content");
    test_repo.stage_files(&["feature1"]);
    test_repo.commit_staged("Feature 1");
    let feat1_oid = test_repo.head_oid();

    // Merge feat1 into integration branch
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feat1");

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
    let status_output = test_repo.status_porcelain();
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
    test_repo.create_branch_at("feature-a", &a1_oid.to_string());

    let base_oid = test_repo.find_remote_branch_target("origin/main");
    // Add a commit on integration line before merge
    test_repo.commit("B1", "b1.txt");

    // Manually set up merge topology:
    // Rebase B1 onto merge-base, then merge feature-a
    test_repo.rebase_onto(&base_oid.to_string(), &a1_oid.to_string());
    test_repo.merge_no_ff("feature-a");

    // Now add a loose commit on the integration line
    test_repo.commit("C1", "c1.txt");
    let c1_oid = test_repo.head_oid();

    // Move C1 to feature-a
    let result = super::fold_commit_to_branch(&test_repo.repo, &c1_oid.to_string(), "feature-a");

    assert!(result.is_ok(), "fold_commit_to_branch failed: {:?}", result);

    // C1 should now be on feature-a's branch (feature-a tip should have C1's message)
    assert_eq!(
        test_repo.branch_commit_summary("feature-a"),
        "C1",
        "C1 should now be at the tip of feature-a"
    );
}

#[test]
fn fold_commit_to_branch_via_short_ids() {
    // Regression: `run()` was missing TargetKind::Branch when resolving the
    // target, so `fold <commit-sid> <branch-sid>` would fail with
    // "'xx' did not resolve to a commit or commit file or file or unstaged changes".
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at("feature-a", &a1_oid.to_string());

    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.commit("B1", "b1.txt");
    test_repo.rebase_onto(&base_oid.to_string(), &a1_oid.to_string());
    test_repo.merge_no_ff("feature-a");

    test_repo.commit("C1", "c1.txt");
    let c1_oid = test_repo.head_oid();

    // Get short IDs for the commit and branch
    let (commit_sid, branch_sid) = test_repo.in_dir(|| {
        let info = crate::git::gather_repo_info(&test_repo.repo, false, 1).unwrap();
        let alloc = crate::shortid::IdAllocator::new(info.collect_entities());
        (
            alloc.get_commit(c1_oid).to_string(),
            alloc.get_branch("feature-a").to_string(),
        )
    });

    let result =
        test_repo.in_dir(|| super::run(false, vec![commit_sid.clone(), branch_sid.clone()]));

    assert!(result.is_ok(), "fold via short IDs failed: {:?}", result);
    assert_eq!(
        test_repo.branch_commit_summary("feature-a"),
        "C1",
        "C1 should now be at the tip of feature-a"
    );
}

#[test]
fn fold_commit_to_branch_dirty_autostashed() {
    // Set up an integration branch with a woven feature branch and a loose commit
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();

    test_repo.create_branch_at("feature-a", &a1_oid.to_string());

    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.commit("B1", "b1.txt");

    test_repo.rebase_onto(&base_oid.to_string(), &a1_oid.to_string());
    test_repo.merge_no_ff("feature-a");

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
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Build the 'test' branch with two commits: Feat1 and Feat3
    test_repo.create_branch_at("test", &base_oid.to_string());
    test_repo.switch_branch("test");
    test_repo.commit("Feat1", "feat1.txt");
    test_repo.commit("Feat3", "feat3.txt");
    let feat3_oid = test_repo.head_oid();
    test_repo.switch_branch("integration");

    // Weave the 'test' branch
    test_repo.merge_no_ff("test");

    // Build the 'feat2' branch with one commit: Feat2
    test_repo.create_branch_at("feat2", &base_oid.to_string());
    test_repo.switch_branch("feat2");
    test_repo.commit("Feat2", "feat2.txt");
    test_repo.switch_branch("integration");

    // Create feat3 as co-located with feat2 (same tip)
    let feat2_tip = test_repo.get_branch_target("feat2");
    test_repo.create_branch_at("feat3", &feat2_tip.to_string());

    // Weave feat2 (which also brings in feat3 since they're co-located)
    test_repo.merge_no_ff("feat2");

    // Now move the Feat3 commit from 'test' branch to 'feat3' branch
    let result = super::fold_commit_to_branch(&test_repo.repo, &feat3_oid.to_string(), "feat3");

    assert!(result.is_ok(), "fold_commit_to_branch failed: {:?}", result);

    // feat3 should have Feat3 at its tip (above feat2)
    assert_eq!(
        test_repo.branch_commit_summary("feat3"),
        "Feat3",
        "feat3 tip should be Feat3"
    );

    // feat2 should still have Feat2 at its tip (NOT Feat3)
    assert_eq!(
        test_repo.branch_commit_summary("feat2"),
        "Feat2",
        "feat2 tip should still be Feat2, not Feat3"
    );

    // feat3 should be stacked on feat2: feat3's parent should be feat2's tip
    let feat3_commit = test_repo.find_commit(test_repo.get_branch_target("feat3"));
    assert_eq!(
        feat3_commit.parent_id(0).unwrap(),
        test_repo.get_branch_target("feat2"),
        "feat3 should be stacked on feat2"
    );

    // The outermost merge commit (HEAD) should reference feat3, not feat2
    let head = test_repo.head_commit();
    assert!(
        head.summary().unwrap_or("").contains("feat3"),
        "HEAD merge message should reference 'feat3', got: {:?}",
        head.summary()
    );
}

#[test]
fn fold_commit_to_empty_branch() {
    // Reproduce: a branch at the merge-base (no commits, no merge in the
    // integration line) and another branch with commits. Moving a commit to
    // the empty branch should create a section+merge and update the ref.
    //
    // This is the real-world scenario: create feat-a with a commit, move it
    // away (leaving feat-a at base with no merge), then move another commit
    // back to feat-a.
    //
    // Before:
    //   ╭─ [feature-b]
    //   ●  B1
    //   ●  A1
    //   ╯
    //   ● [feature-a]   ← at base, no merge in topology
    //
    // After fold A1 → feature-a:
    //   ╭─ [feature-a]
    //   ●  A1
    //   ╯
    //   ╭─ [feature-b]
    //   ●  B1
    //   ╯
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at base (empty branch, not woven)
    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    // Create feature-b with two commits
    test_repo.create_branch_at("feature-b", &base_oid.to_string());
    test_repo.switch_branch("feature-b");
    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();
    test_repo.commit("B1", "b1.txt");
    test_repo.switch_branch("integration");

    // Weave feature-b into the integration line
    test_repo.merge_no_ff("feature-b");

    // Verify feature-a has no section in the graph (it's at base, not woven)
    let graph = Weave::from_repo(&test_repo.repo).unwrap();
    assert!(
        !graph.branch_sections.iter().any(|s| s.label == "feature-a"),
        "feature-a should NOT have a section before the fold"
    );

    // Move A1 from feature-b to feature-a
    let result = super::fold_commit_to_branch(&test_repo.repo, &a1_oid.to_string(), "feature-a");
    assert!(result.is_ok(), "fold_commit_to_branch failed: {:?}", result);

    // feature-a should now point to a commit with message "A1"
    assert_eq!(
        test_repo.branch_commit_summary("feature-a"),
        "A1",
        "feature-a tip should be A1, but branch was not updated (still at base)"
    );

    // feature-a should NOT still be at the base
    assert_ne!(
        test_repo.get_branch_target("feature-a"),
        base_oid,
        "feature-a should have moved from the base commit"
    );
}

#[test]
fn fold_commit_to_existing_out_of_scope_branch_fails() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create an out-of-scope branch with its own commit (diverged from base)
    test_repo.create_branch_at("out-of-scope", &base_oid.to_string());
    test_repo.switch_branch("out-of-scope");
    test_repo.commit("Out of scope work", "oos.txt");
    test_repo.switch_branch("integration");

    // Create a woven branch with a commit
    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

    // Try to move A1 to the out-of-scope branch — should fail
    let result = super::fold_commit_to_branch(&test_repo.repo, &a1_oid.to_string(), "out-of-scope");
    assert!(result.is_err(), "should reject out-of-scope branch");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not part of the current integration scope"),
        "error should mention scope, got: {}",
        err
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
fn fold_unstaged_into_commit() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Modify files without staging
    test_repo.write_file("file1.txt", "modified 1");
    test_repo.write_file("file2.txt", "modified 2");

    let head_oid = test_repo.head_oid();

    // fold zz HEAD — should amend all changed files into HEAD
    let result = test_repo.in_dir(|| super::run(false, vec!["zz".into(), "HEAD".into()]));
    assert!(result.is_ok(), "fold zz HEAD failed: {:?}", result);

    assert_ne!(test_repo.head_oid(), head_oid, "Hash should have changed");
    assert_eq!(test_repo.read_file("file1.txt"), "modified 1");
    assert_eq!(test_repo.read_file("file2.txt"), "modified 2");
}

#[test]
fn fold_unstaged_clean_tree_fails() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");

    let result = test_repo.in_dir(|| super::run(false, vec!["zz".into(), "HEAD".into()]));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("working tree is clean"),
        "Expected clean-tree error"
    );
}

// ── Case 5: CommitFile + Unstaged (Uncommit file) ─────────────────────────

#[test]
fn fold_commit_file_to_unstaged_head() {
    let test_repo = TestRepo::new();

    // Commit has two files (use CLI for consistent index)
    test_repo.write_file("file1.txt", "content1");
    test_repo.write_file("file2.txt", "content2");
    test_repo.stage_files(&["file1.txt", "file2.txt"]);
    test_repo.commit_staged("Two files");

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

    // First commit has two files (use CLI for staging to avoid libgit2 index mismatch)
    test_repo.write_file("file1.txt", "content1");
    test_repo.write_file("file2.txt", "content2");
    test_repo.stage_files(&["file1.txt", "file2.txt"]);
    test_repo.commit_staged("Two files");
    let c1_oid = test_repo.head_oid();

    // Second commit (also via CLI to keep index consistent)
    test_repo.write_file("file3.txt", "content3");
    test_repo.stage_files(&["file3.txt"]);
    test_repo.commit_staged("Second commit");

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

    // First commit has two files (use CLI for staging)
    test_repo.write_file("file1.txt", "content1");
    test_repo.write_file("file2.txt", "content2");
    test_repo.stage_files(&["file1.txt", "file2.txt"]);
    test_repo.commit_staged("Source commit");
    let source_oid = test_repo.head_oid();

    // Second commit (target, also via CLI)
    test_repo.write_file("file3.txt", "content3");
    test_repo.stage_files(&["file3.txt"]);
    test_repo.commit_staged("Target commit");
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

    // C1 (older, target): adds file_a.txt
    test_repo.write_file("file_a.txt", "aaa");
    test_repo.stage_files(&["file_a.txt"]);
    test_repo.commit_staged("Add file_a");
    let c1_oid = test_repo.head_oid();

    // C2 (newer, source): adds file_b.txt and modifies file_a.txt
    test_repo.write_file("file_a.txt", "aaa modified");
    test_repo.write_file("file_b.txt", "bbb");
    test_repo.stage_files(&["file_a.txt", "file_b.txt"]);
    test_repo.commit_staged("Add file_b and modify file_a");
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
    let c2_diff = test_repo.diff_commit(&test_repo.head_oid().to_string());
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
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Build feature-a on its own branch
    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");

    test_repo.write_file("fa1.txt", "feature-a file 1");
    test_repo.write_file("fa2.txt", "feature-a file 2");
    test_repo.stage_files(&["fa1.txt", "fa2.txt"]);
    test_repo.commit_staged("A1: two files");

    // Build feature-b stacked on feature-a
    test_repo.create_branch_at("feature-b", &test_repo.head_oid().to_string());
    test_repo.switch_branch("feature-b");

    test_repo.write_file("fb1.txt", "feature-b file 1");
    test_repo.stage_files(&["fb1.txt"]);
    test_repo.commit_staged("B1: one file");

    // Switch to integration and merge feature-b (includes A1 and B1)
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-b");

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
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Build feature-a branch
    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");

    test_repo.write_file("fa1.txt", "feature-a file 1");
    test_repo.stage_files(&["fa1.txt"]);
    test_repo.commit_staged("A1");
    let a1_oid = test_repo.head_oid();

    // Build feature-b stacked on feature-a
    test_repo.create_branch_at("feature-b", &a1_oid.to_string());
    test_repo.switch_branch("feature-b");

    test_repo.write_file("fb1.txt", "feature-b file 1");
    test_repo.write_file("fb2.txt", "feature-b file 2");
    test_repo.stage_files(&["fb1.txt", "fb2.txt"]);
    test_repo.commit_staged("B1");
    let b1_oid = test_repo.head_oid();

    // Merge into integration
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-b");

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
    let b_diff = test_repo.diff_commit(&b_tip.to_string());
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
    let a_diff = test_repo.diff_commit(&a_tip.to_string());
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
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Build foo1 branch with one commit
    test_repo.create_branch_at("foo1", &base_oid.to_string());
    test_repo.switch_branch("foo1");
    test_repo.write_file("feature1", "feat 1");
    test_repo.stage_files(&["feature1"]);
    test_repo.commit_staged("Feature 1");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("foo1");

    // Build foo2 branch with one commit (on base, not on integration)
    test_repo.create_branch_at("foo2", &base_oid.to_string());
    test_repo.switch_branch("foo2");
    test_repo.write_file("feature2", "feat 2");
    test_repo.stage_files(&["feature2"]);
    test_repo.commit_staged("Feature 2");
    let foo2_tip = test_repo.head_oid();

    // Build foo3 stacked on foo2 with one commit that modifies feature2 and adds feature7
    test_repo.create_branch_at("foo3", &foo2_tip.to_string());
    test_repo.switch_branch("foo3");
    test_repo.write_file("feature2", "feat 2 updated");
    test_repo.write_file("feature7", "feat 7");
    test_repo.stage_files(&["feature2", "feature7"]);
    test_repo.commit_staged("Feature 2 fixup");
    let foo3_tip = test_repo.head_oid();

    // Only merge foo3 into integration (brings in both foo2 and foo3 commits)
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("foo3");

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
    let foo2_diff = test_repo.diff_commit(&foo2_new_tip.to_string());
    assert!(
        foo2_diff.contains("feature7"),
        "foo2 should now include feature7, but diff is:\n{}",
        foo2_diff
    );

    // foo3's commit should no longer include feature7
    let foo3_new_tip = test_repo.get_branch_target("foo3");
    let foo3_diff = test_repo.diff_commit(&foo3_new_tip.to_string());
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

    let result = test_repo
        .in_dir(|| git::resolve_arg(&test_repo.repo, "file1.txt", &[git::TargetKind::File]));
    assert!(result.is_ok(), "resolve failed: {:?}", result);
    assert!(matches!(result.unwrap(), git::Target::File(_)));
}

#[test]
fn resolve_fold_arg_commit_hash() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit_empty("commit");

    let result = test_repo.in_dir(|| {
        git::resolve_arg(
            &test_repo.repo,
            &c1_oid.to_string(),
            &[git::TargetKind::Commit],
        )
    });
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), git::Target::Commit(_)));
}

#[test]
fn resolve_fold_arg_branch_name() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", a1_oid);

    let result = test_repo
        .in_dir(|| git::resolve_arg(&test_repo.repo, "feature-a", &[git::TargetKind::Branch]));
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), git::Target::Branch(_)));
}

#[test]
fn resolve_fold_arg_head() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("commit");

    let result =
        test_repo.in_dir(|| git::resolve_arg(&test_repo.repo, "HEAD", &[git::TargetKind::Commit]));
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), git::Target::Commit(_)));
}

// ── fold --create ─────────────────────────────────────────────────────────

#[test]
fn fold_create_moves_commit_on_branch_to_new_branch() {
    // Set up an integration branch with a woven feature branch.
    // Move a commit from the feature branch into a brand new branch.
    //
    // Before:
    //   ╭─ [feature-a]
    //   ●  A1  ← move this to new-branch
    //   ╯
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    let a1_oid = test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

    let result = super::run_create(
        &test_repo.repo,
        &[a1_oid.to_string(), "new-branch".to_string()],
    );
    assert!(result.is_ok(), "fold --create failed: {:?}", result);

    // new-branch should exist and point to A1
    assert_eq!(
        test_repo.branch_commit_summary("new-branch"),
        "A1",
        "new-branch should have A1 at its tip"
    );
}

#[test]
fn fold_create_warns_and_moves_to_existing_branch() {
    // When --create is used but the branch already exists, warn and move the commit.
    // Uses a non-woven feature-a at base and a loose commit on integration.
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // feature-a exists at base (not woven, no section in the graph)
    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    // Add a loose commit on integration
    let loose_oid = test_repo.commit("Loose", "loose.txt");

    // feature-a already exists — should warn and move the commit anyway
    let result = super::run_create(
        &test_repo.repo,
        &[loose_oid.to_string(), "feature-a".to_string()],
    );
    assert!(
        result.is_ok(),
        "fold --create with existing branch should succeed: {:?}",
        result
    );
    assert_eq!(
        test_repo.branch_commit_summary("feature-a"),
        "Loose",
        "Loose commit should have moved to feature-a"
    );
}

#[test]
fn fold_create_rejects_non_commit_source() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.create_branch("feature-a");

    let result = super::run_create(
        &test_repo.repo,
        &["feature-a".to_string(), "new-branch".to_string()],
    );
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("commit"),
        "should error when source is a branch, got: {err}"
    );
}

// ── Single-arg: Staged files into commit ──────────────────────────────

#[test]
fn fold_staged_into_head() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Modify and stage a file
    test_repo.write_file("file1.txt", "staged content");
    test_repo.stage_files(&["file1.txt"]);

    let head_oid = test_repo.head_oid();

    let result = super::run_staged(&test_repo.repo, &head_oid.to_string());
    assert!(result.is_ok(), "run_staged failed: {:?}", result);

    // HEAD should have been amended
    assert_eq!(test_repo.get_message(0), "Second commit");
    assert_ne!(test_repo.head_oid(), head_oid);
    assert_eq!(test_repo.read_file("file1.txt"), "staged content");
}

#[test]
fn fold_staged_nothing_staged_fails() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");

    let head_oid = test_repo.head_oid();

    let result = super::run_staged(&test_repo.repo, &head_oid.to_string());
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Nothing to commit"),
        "should error when nothing is staged"
    );
}

#[test]
fn fold_staged_only_uses_staged_not_unstaged() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    // Stage one file, leave another unstaged
    test_repo.write_file("file1.txt", "staged content");
    test_repo.write_file("file2.txt", "unstaged content");
    test_repo.stage_files(&["file1.txt"]);

    let head_oid = test_repo.head_oid();

    let result = super::run_staged(&test_repo.repo, &head_oid.to_string());
    assert!(result.is_ok(), "run_staged failed: {:?}", result);

    // Only file1.txt should be in the commit; file2.txt should remain as unstaged
    assert_eq!(test_repo.read_file("file1.txt"), "staged content");
    // file2.txt should still have working tree changes (not committed)
    assert_eq!(test_repo.read_file("file2.txt"), "unstaged content");
}

#[test]
fn fold_staged_non_commit_target_fails() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("First commit", "file1.txt");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", a1_oid);

    test_repo.write_file("file1.txt", "staged content");
    test_repo.stage_files(&["file1.txt"]);

    // Passing a branch name when only Commit is accepted should fail
    let result = test_repo.in_dir(|| super::run_staged(&test_repo.repo, "feature-a"));
    assert!(result.is_err(), "should have failed");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("commit"),
        "should error when target is not a commit, got: {err_msg}"
    );
}

// ── Abort preserves working state ────────────────────────────────────────

/// Regression: loom abort after a fold conflict must preserve staged changes
/// on other files, unstaged changes, and new untracked files.
///
/// Conflict setup: Commit A creates `shared.txt`; Commit B modifies it.
/// Folding the working-tree version of `shared.txt` into A rewrites A's
/// content; when B is replayed it expects A's original content → conflict.
#[test]
fn fold_abort_preserves_working_state() {
    let test_repo = TestRepo::new_with_remote();

    let a_oid = test_repo.commit("version-a", "shared.txt");
    test_repo.write_file("shared.txt", "version-b");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit B");

    // Write the content we want to fold into A.
    // When B is replayed after modified-A it expects "version-a" → conflict.
    test_repo.write_file("shared.txt", "version-folded");

    // Bystander state — fold.rs saves other staged files via saved_staged_patch.
    test_repo.write_file("other-staged.txt", "staged-content");
    test_repo.stage_files(&["other-staged.txt"]);
    test_repo.write_file("other-unstaged.txt", "unstaged-content");
    test_repo.write_file("new-file.txt", "new-content");

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["shared.txt".to_string()],
        &a_oid.to_string(),
    );
    assert!(
        result.is_ok(),
        "fold should pause on conflict: {:?}",
        result
    );

    let state_path = test_repo.repo.path().join("loom").join("state.json");
    assert!(
        state_path.exists(),
        "loom state must exist when fold is paused on conflict"
    );

    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();
    crate::transaction::abort_cmd(&workdir, &git_dir).unwrap();

    assert_eq!(test_repo.read_file("other-staged.txt"), "staged-content");
    assert_eq!(
        test_repo.read_file("other-unstaged.txt"),
        "unstaged-content"
    );
    assert!(
        workdir.join("new-file.txt").exists(),
        "new untracked file must survive abort"
    );
    assert_eq!(test_repo.read_file("new-file.txt"), "new-content");
}
