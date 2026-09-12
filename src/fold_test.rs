use crate::core::repo;
use crate::core::test_helpers::TestRepo;
use crate::core::weave::Weave;

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
        false,
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
        false,
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
        false,
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
        false,
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
        false,
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
        false,
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

// ── Case 1b: fold -p (skip_staging=true) — hunk-level precision ─────────

/// Read the content of a file from a specific commit tree.
fn read_file_from_commit(repo: &git2::Repository, commit_oid: git2::Oid, path: &str) -> String {
    let commit = repo.find_commit(commit_oid).unwrap();
    let tree = commit.tree().unwrap();
    let entry = tree.get_path(std::path::Path::new(path)).unwrap();
    let blob = repo.find_blob(entry.id()).unwrap();
    std::str::from_utf8(blob.content()).unwrap().to_string()
}

/// Regression test: `fold -p <commit>` must fold only staged hunks, not entire files.
///
/// Before the fix, `fold_files_into_commit` called `git stage_files` which
/// re-staged the whole file, overwriting the hunk-level staging set by
/// `apply_selections`.  With `skip_staging = true` the precise staging must be
/// preserved.
#[test]
fn fold_patch_only_staged_hunk_is_folded_into_head() {
    let test_repo = TestRepo::new();

    // Initial file with enough gap between sections to produce two hunks.
    let initial = "line 1\nline 2\nline 3\nline 4\nline 5\n\
                   line 6\nline 7\nline 8\nline 9\nline 10\n\
                   line 11\nline 12\nline 13\nline 14\nline 15\n";
    test_repo.write_file("file.txt", initial);
    test_repo.stage_files(&["file.txt"]);
    test_repo.commit_staged("initial");

    // Modify two distant regions — produces two separate hunks.
    let modified = "line 1\nMODIFIED TOP\nline 3\nline 4\nline 5\n\
                    line 6\nline 7\nline 8\nline 9\nline 10\n\
                    line 11\nline 12\nline 13\nMODIFIED BOTTOM\nline 15\n";
    test_repo.write_file("file.txt", modified);

    // Stage only the first hunk (top change) by applying a patch to the index.
    let first_hunk_patch = "--- a/file.txt\n+++ b/file.txt\n\
                             @@ -1,5 +1,5 @@\n line 1\n-line 2\n+MODIFIED TOP\n \
                             line 3\n line 4\n line 5\n";
    let workdir = test_repo.workdir();
    crate::git::apply_cached_patch(workdir.as_path(), first_hunk_patch).unwrap();

    let head_oid = test_repo.head_oid();
    let staged = crate::core::repo::get_staged_files(&test_repo.repo).unwrap();
    assert_eq!(staged, vec!["file.txt"]);

    let result =
        super::fold_files_into_commit(&test_repo.repo, &staged, &head_oid.to_string(), true);
    assert!(
        result.is_ok(),
        "fold_files_into_commit failed: {:?}",
        result
    );

    // The committed file must contain only the first hunk — not MODIFIED BOTTOM.
    let committed = read_file_from_commit(&test_repo.repo, test_repo.head_oid(), "file.txt");
    assert!(
        committed.contains("MODIFIED TOP"),
        "committed content should contain MODIFIED TOP"
    );
    assert!(
        !committed.contains("MODIFIED BOTTOM"),
        "committed content must NOT contain MODIFIED BOTTOM (whole-file bug)"
    );

    // The second change must still be in the working tree (not lost).
    let worktree = test_repo.read_file("file.txt");
    assert!(
        worktree.contains("MODIFIED BOTTOM"),
        "working tree should still have MODIFIED BOTTOM"
    );
}

/// Same regression but targeting a non-HEAD commit.
#[test]
fn fold_patch_only_staged_hunk_is_folded_into_non_head() {
    let test_repo = TestRepo::new_with_remote();

    let initial = "line 1\nline 2\nline 3\nline 4\nline 5\n\
                   line 6\nline 7\nline 8\nline 9\nline 10\n\
                   line 11\nline 12\nline 13\nline 14\nline 15\n";
    test_repo.write_file("file.txt", initial);
    test_repo.stage_files(&["file.txt"]);
    test_repo.commit_staged("target commit");
    let target_oid = test_repo.head_oid();

    // Add a second commit on top using CLI helpers to avoid index sync issues.
    test_repo.write_file("other.txt", "other content");
    test_repo.stage_files(&["other.txt"]);
    test_repo.commit_staged("second commit");

    // Modify two distant regions.
    let modified = "line 1\nMODIFIED TOP\nline 3\nline 4\nline 5\n\
                    line 6\nline 7\nline 8\nline 9\nline 10\n\
                    line 11\nline 12\nline 13\nMODIFIED BOTTOM\nline 15\n";
    test_repo.write_file("file.txt", modified);

    // Stage only the first hunk.
    let first_hunk_patch = "--- a/file.txt\n+++ b/file.txt\n\
                             @@ -1,5 +1,5 @@\n line 1\n-line 2\n+MODIFIED TOP\n \
                             line 3\n line 4\n line 5\n";
    let workdir = test_repo.workdir();
    crate::git::apply_cached_patch(workdir.as_path(), first_hunk_patch).unwrap();

    let staged = crate::core::repo::get_staged_files(&test_repo.repo).unwrap();
    let result =
        super::fold_files_into_commit(&test_repo.repo, &staged, &target_oid.to_string(), true);
    assert!(
        result.is_ok(),
        "fold_files_into_commit failed: {:?}",
        result
    );

    // Find the new OID of the rewritten target commit (it's now 1 step back).
    let new_target_oid = test_repo.get_oid(1);
    let committed = read_file_from_commit(&test_repo.repo, new_target_oid, "file.txt");
    assert!(
        committed.contains("MODIFIED TOP"),
        "committed content should contain MODIFIED TOP"
    );
    assert!(
        !committed.contains("MODIFIED BOTTOM"),
        "committed content must NOT contain MODIFIED BOTTOM (whole-file bug)"
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
        let info = crate::core::repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
        let alloc = crate::core::shortid::IdAllocator::new(info.collect_entities());
        (
            alloc.get_commit(c1_oid).to_string(),
            alloc.get_branch("feature-a").to_string(),
        )
    });

    let result = test_repo.in_dir(|| {
        super::run(
            false,
            false,
            vec![commit_sid.clone(), branch_sid.clone()],
            &crate::core::graph::Theme::dark(),
        )
    });

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
        head.summary()
            .ok()
            .flatten()
            .unwrap_or("")
            .contains("feat3"),
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
    let sources = vec![repo::Target::File("f1.txt".into())];
    let target = repo::Target::Commit("abc123".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
}

#[test]
fn classify_commit_into_commit() {
    let sources = vec![repo::Target::Commit("abc123".into())];
    let target = repo::Target::Commit("def456".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
}

#[test]
fn classify_commit_into_branch() {
    let sources = vec![repo::Target::Commit("abc123".into())];
    let target = repo::Target::Branch("feature-a".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
}

#[test]
fn classify_several_commits_into_branch() {
    let sources = vec![
        repo::Target::Commit("abc123".into()),
        repo::Target::Commit("def456".into()),
    ];
    let target = repo::Target::Branch("feature-a".into());
    match super::classify(&sources, &target) {
        Ok(super::FoldOp::CommitsToBranch { commits, branch }) => {
            assert_eq!(commits.len(), 2);
            assert_eq!(branch, "feature-a");
        }
        other => panic!("expected CommitsToBranch, got {other:?}"),
    }
}

#[test]
fn classify_branch_source_rejected() {
    let sources = vec![repo::Target::Branch("feature-a".into())];
    let target = repo::Target::Commit("abc123".into());
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
    let sources = vec![repo::Target::File("f1.txt".into())];
    let target = repo::Target::Branch("feature-a".into());
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
        repo::Target::File("f1.txt".into()),
        repo::Target::Commit("abc123".into()),
    ];
    let target = repo::Target::Commit("def456".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Cannot mix"));
}

#[test]
fn classify_multiple_commit_sources_rejected() {
    let sources = vec![
        repo::Target::Commit("abc123".into()),
        repo::Target::Commit("def456".into()),
    ];
    let target = repo::Target::Commit("ghi789".into());
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

/// A commit's diff is taken against its own parent but applied on top of the
/// later commits, so a neighbor's edit inside a hunk's context defeats a plain
/// `git apply`. The three-way fallback merges it anyway.
#[test]
fn fold_commit_to_unstaged_when_a_later_commit_edited_nearby_lines() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_multi(&[("f.txt", "1\n2\n3\n4\n5\n6\n7\n")], "Base");
    let target = test_repo.commit_multi(&[("f.txt", "1\n2\n3\nFOUR\n5\n6\n7\n")], "Change 4");
    test_repo.commit_multi(&[("f.txt", "1\n2\n3\nFOUR\n5\nSIX\n7\n")], "Change 6");

    let result = super::fold_commit_to_unstaged(&test_repo.repo, &target.to_string());

    assert!(
        result.is_ok(),
        "fold_commit_to_unstaged failed: {:?}",
        result
    );
    assert_eq!(test_repo.get_message(0), "Change 6");
    assert_eq!(test_repo.read_file("f.txt"), "1\n2\n3\nFOUR\n5\nSIX\n7\n");
    // Porcelain, not `diff HEAD`: the index column is what says the merged
    // change came back unstaged, which is the whole point of `zz`.
    assert_eq!(test_repo.status_porcelain(), " M f.txt\n");
}

/// When the uncommitted diff cannot be merged back, the rollback restores the
/// history — and the uncommitted changes the rebase's autostash had put back.
#[test]
fn fold_commit_to_unstaged_rollback_keeps_uncommitted_changes() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_multi(&[("f.txt", "1\n2\n3\n4\n5\n6\n7\n")], "Base");
    let target = test_repo.commit_multi(&[("f.txt", "1\n2\n3\nFOUR\n5\n6\n7\n")], "Change 4");
    test_repo.commit("Other", "other.txt");
    // The rename replays fine without "Change 4", but leaves its diff with
    // nowhere to apply.
    crate::git::run_git(&test_repo.workdir(), &["mv", "f.txt", "g.txt"]).unwrap();
    test_repo.commit_staged("Rename");

    let head_before = test_repo.head_oid();
    test_repo.write_file("other.txt", "uncommitted work");

    let err = super::fold_commit_to_unstaged(&test_repo.repo, &target.to_string())
        .expect_err("the diff must not merge back");
    assert!(
        err.to_string().contains("rolled back"),
        "unexpected error: {err}"
    );

    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "history must be restored"
    );
    assert_eq!(test_repo.read_file("other.txt"), "uncommitted work");
    assert_eq!(test_repo.read_file("g.txt"), "1\n2\n3\nFOUR\n5\n6\n7\n");
    assert_eq!(test_repo.status_porcelain(), " M other.txt\n");
}

/// Uncommitting the only commit of a woven branch keeps the branch, parked
/// at its base, so the reworked change can be committed to it again. Same
/// for an inner (stacked) branch and for a branch with its own section.
#[test]
fn fold_commit_to_unstaged_parks_sole_branch_at_base() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("inner", &base_oid.to_string());
    test_repo.switch_branch("inner");
    let i1_oid = test_repo.commit("I1", "i1.txt");
    test_repo.create_branch_at("inner-too", &i1_oid.to_string());

    test_repo.create_branch_at("outer", &i1_oid.to_string());
    test_repo.switch_branch("outer");
    test_repo.commit("O1", "o1.txt");

    test_repo.create_branch_at("solo", &base_oid.to_string());
    test_repo.switch_branch("solo");
    let s1_oid = test_repo.commit("S1", "s1.txt");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("outer");
    test_repo.merge_no_ff("solo");

    // Inner branches: parked at the base of outer's section
    super::fold_commit_to_unstaged(&test_repo.repo, &i1_oid.to_string())
        .expect("uncommitting the sole commit of two inner branches");
    for name in ["inner", "inner-too"] {
        assert_eq!(test_repo.get_branch_target(name), base_oid, "{name}");
    }
    let outer = test_repo.get_branch_target("outer");
    let o1 = test_repo.repo.find_commit(outer).unwrap();
    assert_eq!(o1.summary().unwrap().unwrap(), "O1");
    assert_eq!(o1.parent_id(0).unwrap(), base_oid, "I1 is gone from outer");
    assert!(test_repo.status_porcelain().contains("i1.txt"));

    // Section branch: its merge goes, the branch stays at the base
    let head_before = test_repo.head_oid();
    super::fold_commit_to_unstaged(&test_repo.repo, &s1_oid.to_string())
        .expect("uncommitting the sole commit of a woven branch");
    assert_eq!(test_repo.get_branch_target("solo"), base_oid);
    assert_ne!(test_repo.head_oid(), head_before);
    let head = test_repo.repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2, "only the merge of outer remains");
    assert_eq!(
        head.parent_id(1).unwrap(),
        test_repo.get_branch_target("outer")
    );
    assert!(test_repo.status_porcelain().contains("s1.txt"));
}

/// Moving the only commit of an inner branch to another branch leaves the
/// inner branch behind, parked at its base, rather than dragging it into
/// the target branch.
#[test]
fn fold_commit_to_branch_leaves_inner_branch_behind() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("inner", &base_oid.to_string());
    test_repo.switch_branch("inner");
    let i1_oid = test_repo.commit("I1", "i1.txt");

    test_repo.create_branch_at("outer", &i1_oid.to_string());
    test_repo.switch_branch("outer");
    test_repo.commit("O1", "o1.txt");

    test_repo.create_branch_at("other", &base_oid.to_string());
    test_repo.switch_branch("other");
    test_repo.commit("X1", "x1.txt");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("outer");
    test_repo.merge_no_ff("other");

    let (_, parked) =
        super::move_commits_to_branch(&test_repo.repo, &[i1_oid.to_string()], "other")
            .expect("moving the sole commit of an inner branch");

    assert_eq!(parked, vec!["inner".to_string()]);
    assert_eq!(test_repo.get_branch_target("inner"), base_oid);
    let repo = &test_repo.repo;
    let other = repo
        .find_commit(test_repo.get_branch_target("other"))
        .unwrap();
    assert_eq!(other.summary().unwrap().unwrap(), "I1");
    assert_eq!(
        repo.find_commit(other.parent_id(0).unwrap())
            .unwrap()
            .summary()
            .unwrap()
            .unwrap(),
        "X1"
    );
    let outer = repo
        .find_commit(test_repo.get_branch_target("outer"))
        .unwrap();
    assert_eq!(outer.summary().unwrap().unwrap(), "O1");
    assert_eq!(outer.parent_id(0).unwrap(), base_oid);
}

/// Moving a commit that is not the inner branch's only one leaves that branch
/// at the commit before, rather than parking it at the base.
#[test]
fn fold_commit_to_branch_leaves_inner_branch_at_the_commit_before() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("inner", &base_oid.to_string());
    test_repo.switch_branch("inner");
    let i1_oid = test_repo.commit("I1", "i1.txt");
    let i2_oid = test_repo.commit("I2", "i2.txt");

    test_repo.create_branch_at("outer", &i2_oid.to_string());
    test_repo.switch_branch("outer");
    test_repo.commit("O1", "o1.txt");

    test_repo.create_branch_at("other", &base_oid.to_string());
    test_repo.switch_branch("other");
    test_repo.commit("X1", "x1.txt");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("outer");
    test_repo.merge_no_ff("other");

    let (_, parked) =
        super::move_commits_to_branch(&test_repo.repo, &[i2_oid.to_string()], "other")
            .expect("moving a commit out of an inner branch");

    assert!(parked.is_empty(), "inner still has a commit: {parked:?}");
    assert_eq!(
        test_repo.branch_commit_summary("inner"),
        "I1",
        "inner must end at the commit before I2"
    );
    assert_eq!(test_repo.get_branch_target("inner"), i1_oid);
    assert_eq!(test_repo.branch_commit_summary("other"), "I2");
}

/// The target is a branch stacked inside another: the commit becomes its
/// tip and the branch above is replayed on top of it.
#[test]
fn fold_commit_to_inner_branch() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("inner", &base_oid.to_string());
    test_repo.switch_branch("inner");
    let i1_oid = test_repo.commit("I1", "i1.txt");

    test_repo.create_branch_at("outer", &i1_oid.to_string());
    test_repo.switch_branch("outer");
    test_repo.commit("O1", "o1.txt");

    test_repo.create_branch_at("other", &base_oid.to_string());
    test_repo.switch_branch("other");
    let x1_oid = test_repo.commit("X1", "x1.txt");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("outer");
    test_repo.merge_no_ff("other");

    let (_, parked) =
        super::move_commits_to_branch(&test_repo.repo, &[x1_oid.to_string()], "inner")
            .expect("moving a commit onto an inner branch");

    assert_eq!(parked, vec!["other".to_string()]);
    assert_eq!(test_repo.get_branch_target("other"), base_oid);

    let repo = &test_repo.repo;
    let inner = repo
        .find_commit(test_repo.get_branch_target("inner"))
        .unwrap();
    assert_eq!(inner.summary().unwrap().unwrap(), "X1");
    assert_eq!(
        inner.parent_id(0).unwrap(),
        i1_oid,
        "X1 sits right after I1"
    );
    let outer = repo
        .find_commit(test_repo.get_branch_target("outer"))
        .unwrap();
    assert_eq!(outer.summary().unwrap().unwrap(), "O1");
    assert_eq!(
        outer.parent_id(0).unwrap(),
        inner.id(),
        "outer is replayed on top of the moved commit"
    );
    assert!(test_repo.commit_has_file(outer.id(), "x1.txt"));
}

#[test]
fn classify_commit_into_unstaged() {
    let sources = vec![repo::Target::Commit("abc123".into())];
    let target = repo::Target::Unstaged;
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap(),
        super::FoldOp::CommitToUnstaged { .. }
    ));
}

#[test]
fn classify_files_into_unstaged_rejected() {
    let sources = vec![repo::Target::File("f1.txt".into())];
    let target = repo::Target::Unstaged;
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
    let result = test_repo.in_dir(|| {
        super::run(
            false,
            false,
            vec!["zz".into(), "HEAD".into()],
            &crate::core::graph::Theme::dark(),
        )
    });
    assert!(result.is_ok(), "fold zz HEAD failed: {:?}", result);

    assert_ne!(test_repo.head_oid(), head_oid, "Hash should have changed");
    assert_eq!(test_repo.read_file("file1.txt"), "modified 1");
    assert_eq!(test_repo.read_file("file2.txt"), "modified 2");
}

#[test]
fn fold_unstaged_clean_tree_fails() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");

    let result = test_repo.in_dir(|| {
        super::run(
            false,
            false,
            vec!["zz".into(), "HEAD".into()],
            &crate::core::graph::Theme::dark(),
        )
    });
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

/// A change that is staged and then undone in the working tree is in
/// `git diff --cached` and nowhere else, so the rollback has to carry the index
/// separately from the files.
#[test]
fn fold_commit_to_unstaged_rollback_keeps_a_staged_only_change() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_multi(&[("f.txt", "1\n2\n3\n4\n5\n6\n7\n")], "Base");
    let target = test_repo.commit_multi(&[("f.txt", "1\n2\n3\nFOUR\n5\n6\n7\n")], "Change 4");
    test_repo.commit("Other", "other.txt");
    crate::git::run_git(&test_repo.workdir(), &["mv", "f.txt", "g.txt"]).unwrap();
    test_repo.commit_staged("Rename");

    let head_before = test_repo.head_oid();
    // Stage a change, then put the working tree back: only the index knows.
    test_repo.write_file("other.txt", "staged only");
    test_repo.stage_files(&["other.txt"]);
    test_repo.write_file("other.txt", "Other");

    super::fold_commit_to_unstaged(&test_repo.repo, &target.to_string())
        .expect_err("the diff must not merge back");

    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "history must be restored"
    );
    // `MM`: the index carries the change, the working tree is back at HEAD —
    // exactly how it stood before the fold.
    assert_eq!(test_repo.status_porcelain(), "MM other.txt\n");
    assert_eq!(test_repo.read_file("other.txt"), "Other");
    assert!(
        crate::git::diff_cached(&test_repo.workdir())
            .unwrap()
            .contains("staged only"),
        "the staged content must be the one that was staged"
    );
}

/// A changed binary file has no text diff to replay, so the snapshot the
/// rollback restores from has to carry the file's bytes — `git apply` is
/// all-or-nothing, and one unreplayable file would sink the text changes with it.
#[test]
fn fold_commit_to_unstaged_rollback_keeps_binary_changes() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit_multi(&[("f.txt", "1\n2\n3\n4\n5\n6\n7\n")], "Base");
    let target = test_repo.commit_multi(&[("f.txt", "1\n2\n3\nFOUR\n5\n6\n7\n")], "Change 4");
    std::fs::write(workdir.join("logo.bin"), [0u8, 1, 2, 3]).unwrap();
    test_repo.stage_files(&["logo.bin"]);
    test_repo.commit_staged("Binary");
    crate::git::run_git(&workdir, &["mv", "f.txt", "g.txt"]).unwrap();
    test_repo.commit_staged("Rename");

    let head_before = test_repo.head_oid();
    std::fs::write(workdir.join("logo.bin"), [9u8, 9, 9, 9, 9]).unwrap();
    test_repo.stage_files(&["logo.bin"]);
    test_repo.write_file("text.txt", "untracked-but-tracked-later");

    super::fold_commit_to_unstaged(&test_repo.repo, &target.to_string())
        .expect_err("the diff must not merge back");

    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "history must be restored"
    );
    assert_eq!(
        std::fs::read(workdir.join("logo.bin")).unwrap(),
        [9u8, 9, 9, 9, 9],
        "the binary change must survive the rollback"
    );
    assert_eq!(
        test_repo.read_file("text.txt"),
        "untracked-but-tracked-later",
        "the text change must survive it too"
    );
    assert_eq!(
        test_repo.status_porcelain(),
        "M  logo.bin\n?? text.txt\n",
        "and the staged binary change must still be staged"
    );
}

/// Same rollback as [`fold_commit_to_unstaged_rollback_keeps_uncommitted_changes`],
/// for the single-file form.
#[test]
fn fold_commit_file_to_unstaged_rollback_keeps_uncommitted_changes() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("file1.txt", "v1");
    test_repo.stage_files(&["file1.txt"]);
    test_repo.commit_staged("Base");

    test_repo.write_file("file1.txt", "content1");
    test_repo.write_file("file2.txt", "content2");
    test_repo.stage_files(&["file1.txt", "file2.txt"]);
    test_repo.commit_staged("Two files");
    let c1_oid = test_repo.head_oid();

    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["other.txt"]);
    test_repo.commit_staged("Other");

    // The rename replays fine without file1.txt's changes, but leaves their
    // diff with nowhere to apply.
    crate::git::run_git(&test_repo.workdir(), &["mv", "file1.txt", "renamed.txt"]).unwrap();
    test_repo.commit_staged("Rename");

    let head_before = test_repo.head_oid();
    test_repo.write_file("other.txt", "uncommitted work");

    let err =
        super::fold_commit_file_to_unstaged(&test_repo.repo, &c1_oid.to_string(), "file1.txt")
            .expect_err("the diff must not merge back");
    assert!(
        err.to_string().contains("rolled back"),
        "unexpected error: {err}"
    );

    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "history must be restored"
    );
    assert_eq!(test_repo.read_file("other.txt"), "uncommitted work");
    assert_eq!(test_repo.read_file("renamed.txt"), "content1");
    assert_eq!(test_repo.status_porcelain(), " M other.txt\n");
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

/// Moving a file backwards onto an older commit replays its diff where the
/// commits in between have not happened yet; when that does not apply, phase 2
/// rolls back over a working tree phase 1's rebase has already restored.
#[test]
fn fold_commit_file_to_commit_rollback_keeps_uncommitted_changes() {
    let test_repo = TestRepo::new_with_remote();
    let body = "1\n2\n3\n4\n5\n6\n7\n";
    test_repo.commit_multi(&[("f.txt", body)], "Base");

    let target = test_repo.commit_multi(
        &[("f.txt", body.replace("6\n", "SIX\n").as_str())],
        "Target",
    );
    // Between the two: its edit is context for the source's diff, and is not
    // there yet when that diff is replayed onto the target.
    let middle = body.replace("6\n", "SIX\n").replace("2\n", "TWO\n");
    test_repo.commit_multi(&[("f.txt", &middle)], "Middle");
    let source = test_repo.commit_multi(
        &[
            ("f.txt", middle.replace("4\n", "FOUR\n").as_str()),
            ("other.txt", "other"),
        ],
        "Source",
    );

    let head_before = test_repo.head_oid();
    test_repo.write_file("other.txt", "uncommitted work");

    super::fold_commit_file_to_commit(
        &test_repo.repo,
        &source.to_string(),
        "f.txt",
        &target.to_string(),
    )
    .expect_err("the file's diff must not apply onto the target");

    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "history must be restored"
    );
    assert_eq!(test_repo.read_file("other.txt"), "uncommitted work");
    assert_eq!(test_repo.status_porcelain(), " M other.txt\n");
}

/// Same rollback for the other direction, where a single rebase stops twice and
/// the second stop is the one that fails.
#[test]
fn fold_commit_file_to_commit_forward_rollback_keeps_uncommitted_changes() {
    let test_repo = TestRepo::new_with_remote();
    let body = "1\n2\n3\n4\n5\n6\n7\n";
    test_repo.commit_multi(&[("f.txt", body), ("other.txt", "other")], "Base");

    let source = test_repo.commit_multi(
        &[("f.txt", body.replace("4\n", "FOUR\n").as_str())],
        "Source",
    );
    // Between the two: once the source no longer carries f.txt, this edit is
    // what the file's diff no longer fits around at the target.
    test_repo.commit_multi(
        &[(
            "f.txt",
            body.replace("4\n", "FOUR\n")
                .replace("2\n", "TWO\n")
                .as_str(),
        )],
        "Middle",
    );
    let target = test_repo.commit_multi(&[("g.txt", "unrelated")], "Target");

    let head_before = test_repo.head_oid();
    test_repo.write_file("other.txt", "uncommitted work");

    super::fold_commit_file_to_commit(
        &test_repo.repo,
        &source.to_string(),
        "f.txt",
        &target.to_string(),
    )
    .expect_err("the file's diff must not apply onto the target");

    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "history must be restored"
    );
    assert_eq!(test_repo.read_file("other.txt"), "uncommitted work");
    assert_eq!(test_repo.status_porcelain(), " M other.txt\n");
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
    let sources = vec![repo::Target::CommitFile {
        commit: "abc123".into(),
        path: "file.txt".into(),
    }];
    let target = repo::Target::Unstaged;
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap(),
        super::FoldOp::CommitFileToUnstaged { .. }
    ));
}

#[test]
fn classify_commit_file_into_commit() {
    let sources = vec![repo::Target::CommitFile {
        commit: "abc123".into(),
        path: "file.txt".into(),
    }];
    let target = repo::Target::Commit("def456".into());
    let result = super::classify(&sources, &target);
    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap(),
        super::FoldOp::CommitFileToCommit { .. }
    ));
}

#[test]
fn classify_commit_file_into_branch_rejected() {
    let sources = vec![repo::Target::CommitFile {
        commit: "abc123".into(),
        path: "file.txt".into(),
    }];
    let target = repo::Target::Branch("feature-a".into());
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
    let sources = vec![repo::Target::Commit("abc123".into())];
    let target = repo::Target::CommitFile {
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
        .in_dir(|| repo::resolve_arg(&test_repo.repo, "file1.txt", &[repo::TargetKind::File]));
    assert!(result.is_ok(), "resolve failed: {:?}", result);
    assert!(matches!(result.unwrap(), repo::Target::File(_)));
}

#[test]
fn resolve_fold_arg_commit_hash() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit_empty("commit");

    let result = test_repo.in_dir(|| {
        repo::resolve_arg(
            &test_repo.repo,
            &c1_oid.to_string(),
            &[repo::TargetKind::Commit],
        )
    });
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), repo::Target::Commit(_)));
}

#[test]
fn resolve_fold_arg_branch_name() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", a1_oid);

    let result = test_repo
        .in_dir(|| repo::resolve_arg(&test_repo.repo, "feature-a", &[repo::TargetKind::Branch]));
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), repo::Target::Branch(_)));
}

#[test]
fn resolve_fold_arg_head() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("commit");

    let result = test_repo
        .in_dir(|| repo::resolve_arg(&test_repo.repo, "HEAD", &[repo::TargetKind::Commit]));
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), repo::Target::Commit(_)));
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
fn fold_create_moves_multiple_commits_to_new_branch() {
    // Move two loose integration commits into a brand new branch.
    //
    // Before:
    //   ●  L2  ← move both to new-branch
    //   ●  L1  ←
    //   ╯
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    let l1_oid = test_repo.commit("L1", "l1.txt");
    let l2_oid = test_repo.commit("L2", "l2.txt");

    // Pass them newest-first to verify they are reordered oldest-first.
    let result = super::run_create(
        &test_repo.repo,
        &[
            l2_oid.to_string(),
            l1_oid.to_string(),
            "new-branch".to_string(),
        ],
    );
    assert!(result.is_ok(), "fold --create failed: {:?}", result);

    // new-branch should contain L1 then L2 (oldest-first), with L2 at the tip.
    let tip = test_repo.get_branch_target("new-branch");
    let tip_commit = test_repo.find_commit(tip);
    assert_eq!(
        tip_commit.summary().unwrap().unwrap(),
        "L2",
        "L2 should be at tip"
    );
    let parent = tip_commit.parent(0).unwrap();
    assert_eq!(
        parent.summary().unwrap().unwrap(),
        "L1",
        "L1 should be below L2"
    );
    assert_eq!(
        parent.parent(0).unwrap().id(),
        base_oid,
        "L1 should sit on the merge-base"
    );
}

/// Install a `commit-msg` hook that fails on any message carrying a diff.
fn write_hook_rejecting_diffs(test_repo: &TestRepo) {
    let hook = test_repo.workdir().join(".git/hooks/commit-msg");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        "#!/bin/sh\ngrep -q '^diff --git' \"$1\" && exit 1\nexit 0\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The merge commit `fold --create` weaves in is made with no editor to strip
/// a `commit.verbose` diff back out of the message.
#[test]
fn fold_create_keeps_the_diff_out_of_the_merge_message() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.set_config("commit.verbose", "true");
    write_hook_rejecting_diffs(&test_repo);

    let l1_oid = test_repo.commit("L1", "l1.txt");

    let result = super::run_create(
        &test_repo.repo,
        &[l1_oid.to_string(), "new-branch".to_string()],
    );
    assert!(result.is_ok(), "fold --create failed: {:?}", result);

    // The hook only proves nothing was rejected; check the message itself, so a
    // merge that stops being created cannot pass this test by default.
    let head = test_repo.head_commit();
    assert_eq!(
        head.parent_count(),
        2,
        "fold --create should have woven the branch in with a merge commit"
    );
    let message = head.message().unwrap();
    assert!(
        !message.contains("diff --git"),
        "the merge message must not carry the diff: {:?}",
        message
    );
}

/// Once a woven branch lands upstream as a fast-forward, the merge-base is
/// that branch's own tip — still inside the weave — and the weave base is
/// somewhere else entirely. `-c` has to create the branch at the weave base:
/// anywhere else and `plan_move` refuses the branch `-c` just handed it.
#[test]
fn fold_create_uses_the_weave_base_when_a_branch_landed_upstream() {
    let test_repo = TestRepo::new_with_remote();

    let weave_base = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", weave_base);
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");
    // Staged through git, not git2: the merge ran in a subprocess, so the
    // cached index a git2 commit would write back is stale.
    test_repo.write_file("l1.txt", "l1");
    test_repo.stage_files(&["l1.txt"]);
    test_repo.commit_staged("L1");
    let l1_oid = test_repo.head_oid();

    // feature-a lands upstream as a fast-forward: origin/main is now its tip,
    // so the merge-base sits on the branch side of the merge.
    test_repo.push_branch_to_remote_main("feature-a");
    let info = repo::gather_commit_graph(&test_repo.repo).unwrap();
    assert_ne!(
        info.upstream.merge_base_oid, weave_base,
        "setup should have left the merge-base off the weave base"
    );

    let result = super::run_create(
        &test_repo.repo,
        &[l1_oid.to_string(), "new-branch".to_string()],
    );
    assert!(result.is_ok(), "fold --create failed: {:?}", result);

    let tip = test_repo
        .repo
        .find_branch("new-branch", git2::BranchType::Local)
        .unwrap()
        .get()
        .peel_to_commit()
        .unwrap();
    assert_eq!(repo::commit_subject(&tip), "L1");
    assert_eq!(
        tip.parent(0).unwrap().id(),
        weave_base,
        "the new branch should be built on the weave base"
    );
}

/// A rebase that refuses to start — a branch it would move is checked out in
/// another worktree — must leave nothing of the fold behind: no `fixup!`
/// commit, no staging loom did itself, no temp branch, no state file.
#[test]
fn fold_rolls_back_when_the_rebase_refuses_to_start() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base = test_repo
        .find_remote_branch_target("origin/main")
        .to_string();

    test_repo.create_branch_at("feature", &base);
    test_repo.switch_branch("feature");
    let a_oid = test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature");

    // Holding `feature` in a second worktree is what makes the rebase refuse,
    // and it refuses before starting: the rollback is all there is.
    let wt = workdir.parent().unwrap().join("wt");
    crate::git::run_git(
        &workdir,
        &["worktree", "add", wt.to_str().unwrap(), "feature"],
    )
    .unwrap();

    let head_before = test_repo.head_oid();
    test_repo.write_file("a1.txt", "the change to fold\n");
    test_repo.write_file("other.txt", "staged by the user\n");
    test_repo.stage_files(&["other.txt"]);

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["a1.txt".to_string()],
        &a_oid.to_string(),
        false,
    );

    assert!(
        result.is_err(),
        "the rebase cannot start, so the fold fails"
    );
    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "the `fixup!` commit must be gone"
    );
    let status = test_repo.status_porcelain();
    assert!(
        status.contains(" M a1.txt"),
        "the folded file goes back to modified-but-unstaged, got: {:?}",
        status
    );
    let staged = crate::core::repo::get_staged_files(&test_repo.repo).unwrap();
    assert_eq!(
        staged,
        vec!["other.txt".to_string()],
        "only the user's own staged file may be left staged"
    );
    assert!(
        test_repo
            .repo
            .find_branch(super::TRACK_BRANCH, git2::BranchType::Local)
            .is_err(),
        "the temp branch must not survive"
    );
    assert!(
        !test_repo
            .repo
            .path()
            .join("loom")
            .join("state.json")
            .exists(),
        "no state file may be left behind"
    );
}

/// A target the weave cannot rewrite has to be refused before anything is
/// committed. The fixup path commits first and builds the graph second, so a
/// late refusal left that commit on HEAD and dropped the staging of every other
/// file the user had staged.
#[test]
fn fold_into_an_out_of_scope_commit_leaves_the_repo_alone() {
    let test_repo = TestRepo::new_with_remote();
    // Upstream's own commit: below the weave's base, so not a commit loom can
    // rewrite.
    let out_of_scope = test_repo.find_remote_branch_target("origin/main");

    test_repo.commit("L0", "other.txt");
    test_repo.commit("L1", "l1.txt");
    let head_before = test_repo.head_oid();

    test_repo.write_file("l1.txt", "the change to fold\n");
    test_repo.write_file("other.txt", "staged by the user\n");
    test_repo.stage_files(&["other.txt"]);

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["l1.txt".to_string()],
        &out_of_scope.to_string(),
        false,
    );

    assert!(result.is_err(), "an out-of-scope target must be refused");
    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "a refused fold must leave no commit behind"
    );
    let staged = crate::core::repo::get_staged_files(&test_repo.repo).unwrap();
    assert!(
        staged.contains(&"other.txt".to_string()),
        "the user's own staged file must still be staged, got: {:?}",
        staged
    );
}

/// `-c` creates. A name that is taken is refused, and the existing branch is
/// left exactly where it was.
#[test]
fn fold_create_rejects_an_existing_branch() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    let loose_oid = test_repo.commit("Loose", "loose.txt");

    let result = super::run_create(
        &test_repo.repo,
        &[loose_oid.to_string(), "feature-a".to_string()],
    );

    // Line by line: the hint carries no leading whitespace of its own, which
    // is what a dropped `\n\` continuation would bake into the literal.
    let err = result.unwrap_err().to_string();
    let mut lines = err.lines();
    assert_eq!(lines.next(), Some("Branch `feature-a` already exists"));
    assert_eq!(
        lines.next(),
        Some("Use `loom fold <commit>... feature-a` to move commits onto it")
    );
    assert_eq!(lines.next(), None, "no extra lines: {err}");
    assert_eq!(
        test_repo.get_branch_target("feature-a"),
        base_oid,
        "feature-a must be untouched"
    );
}

/// Ancestry orders only some pairs, so a committer-time tiebreak laid over it
/// is not transitive: two related commits that each tie with an unrelated
/// third are never compared with each other. Every permutation must still put
/// the ancestor first, and give the same answer.
#[test]
fn sort_commits_puts_an_ancestor_first_whatever_the_input_order() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // One fixed second for all three: the tie is what defeats a committer-time
    // tiebreak, and hoping the machine ran fast enough is not a test.
    const TIED: i64 = 1_700_000_000;
    test_repo.create_branch_at("b1", &base_oid.to_string());
    test_repo.switch_branch("b1");
    let x = test_repo.commit_at("X", "x.txt", TIED);
    let z = test_repo.commit_at("Z", "z.txt", TIED);

    // Y forks from the base, so it is unrelated to both X and Z.
    test_repo.create_branch_at("b2", &base_oid.to_string());
    test_repo.switch_branch("b2");
    let y = test_repo.commit_at("Y", "y.txt", TIED);

    let repo = &test_repo.repo;
    let secs = |o: git2::Oid| repo.find_commit(o).unwrap().time().seconds();
    assert_eq!(secs(x), secs(y));
    assert_eq!(secs(y), secs(z));

    let mut outputs = Vec::new();
    for input in [
        [x, y, z],
        [x, z, y],
        [y, x, z],
        [y, z, x],
        [z, x, y],
        [z, y, x],
    ] {
        let hashes: Vec<String> = input.iter().map(|o| o.to_string()).collect();
        let sorted = super::commits_to_move(repo, hashes, base_oid).unwrap();
        assert_eq!(sorted.len(), 3, "no commit may be dropped: {sorted:?}");
        let at = |o: git2::Oid| sorted.iter().position(|h| *h == o.to_string()).unwrap();
        assert!(
            at(x) < at(z),
            "X is Z's ancestor and must come first, got {sorted:?} from {input:?}"
        );
        outputs.push(sorted);
    }

    // History puts no order on unrelated commits, so the answer must not
    // depend on the order they were listed in.
    assert!(
        outputs.windows(2).all(|w| w[0] == w[1]),
        "every permutation must agree: {outputs:?}"
    );
}

/// The walk is bounded to what loom can rewrite, so a commit from below that
/// boundary never turns up in it. It is upstream history that no move could
/// rewrite anyway, so say so by name instead of walking the whole repository
/// to order something that is going to be refused.
#[test]
fn sort_commits_rejects_a_commit_from_below_the_boundary() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    let local = test_repo.commit("L1", "l1.txt");

    let err = super::commits_to_move(
        &test_repo.repo,
        vec![local.to_string(), base_oid.to_string()],
        base_oid,
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("not in the integration scope"),
        "should name the out-of-scope commit: {err}"
    );
    assert!(
        err.contains(&base_oid.to_string()[..7]),
        "should name which commit: {err}"
    );
}

/// Commits from unrelated branches land in committer order, not grouped by
/// the branch they came from: the order the docs promise.
#[test]
fn sort_commits_orders_unrelated_branches_by_time() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("b1", &base_oid.to_string());
    test_repo.switch_branch("b1");
    let a1 = test_repo.commit_at("A1", "a1.txt", 1_700_000_023);
    let a2 = test_repo.commit_at("A2", "a2.txt", 1_700_000_026);

    test_repo.create_branch_at("b2", &base_oid.to_string());
    test_repo.switch_branch("b2");
    let b1 = test_repo.commit_at("B1", "b1.txt", 1_700_000_021);
    let b2 = test_repo.commit_at("B2", "b2.txt", 1_700_000_028);

    let repo = &test_repo.repo;
    let sorted = super::commits_to_move(
        repo,
        vec![
            a1.to_string(),
            a2.to_string(),
            b1.to_string(),
            b2.to_string(),
        ],
        base_oid,
    )
    .unwrap();

    let names: Vec<String> = sorted
        .iter()
        .map(|h| {
            let oid = git2::Oid::from_str(h).unwrap();
            repo.find_commit(oid)
                .unwrap()
                .summary()
                .unwrap()
                .unwrap()
                .to_string()
        })
        .collect();

    // B1 is the oldest of the four and must lead, even though it is on the
    // branch that was written second.
    assert_eq!(names, vec!["B1", "A1", "A2", "B2"]);
}

/// A commit dated older than its parent — normal after a rebase — still comes
/// after it. Ancestry outranks the date.
#[test]
fn sort_commits_keeps_ancestry_over_a_skewed_date() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("b1", &base_oid.to_string());
    test_repo.switch_branch("b1");
    let x = test_repo.commit_at("X", "x.txt", 1_700_000_100);
    let z = test_repo.commit_at("Z", "z.txt", 1_700_000_050);

    let sorted = super::commits_to_move(
        &test_repo.repo,
        vec![z.to_string(), x.to_string()],
        base_oid,
    )
    .unwrap();

    assert_eq!(sorted, vec![x.to_string(), z.to_string()]);
}

/// One commit gets the same answer as several: the guard is not conditional
/// on how many arguments were typed.
#[test]
fn sort_commits_rejects_a_single_out_of_scope_commit() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.commit("L1", "l1.txt");

    let err = super::commits_to_move(&test_repo.repo, vec![base_oid.to_string()], base_oid)
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("not in the integration scope"),
        "one commit must get the same message as several: {err}"
    );
}

/// The same commit named twice is one commit, so the move keeps the resumable
/// single-commit path instead of being treated as a stack.
#[test]
fn sort_commits_drops_duplicates() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    let x = test_repo.commit("X", "x.txt");

    let sorted = super::commits_to_move(
        &test_repo.repo,
        vec![x.to_string(), x.to_string()],
        base_oid,
    )
    .unwrap();

    assert_eq!(sorted, vec![x.to_string()]);
}

/// Several commits go onto an existing branch in one rebase, oldest-first
/// whatever order they were given in.
#[test]
fn fold_moves_several_commits_to_a_branch() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // feature-a gets A1; B1 stays on the integration line below the merge.
    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at("feature-a", &a1_oid.to_string());
    test_repo.commit("B1", "b1.txt");
    test_repo.rebase_onto(&base_oid.to_string(), &a1_oid.to_string());
    test_repo.merge_no_ff("feature-a");

    let m1 = test_repo.commit("M1", "m1.txt");
    let m2 = test_repo.commit("M2", "m2.txt");

    // Newest first on the command line: the move still lands them in order.
    let result = test_repo.in_dir(|| {
        super::run(
            false,
            false,
            vec![m2.to_string(), m1.to_string(), "feature-a".to_string()],
            &crate::core::graph::Theme::dark(),
        )
    });
    assert!(result.is_ok(), "multi-commit move failed: {:?}", result);

    let repo = &test_repo.repo;
    let tip = repo
        .find_commit(test_repo.get_branch_target("feature-a"))
        .unwrap();
    assert_eq!(tip.summary().unwrap().unwrap(), "M2");
    let mid = repo.find_commit(tip.parent_id(0).unwrap()).unwrap();
    assert_eq!(mid.summary().unwrap().unwrap(), "M1");
    assert_eq!(
        repo.find_commit(mid.parent_id(0).unwrap())
            .unwrap()
            .summary()
            .unwrap()
            .unwrap(),
        "A1"
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

    let head_before = test_repo.head_oid();

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
        false,
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
    crate::core::transaction::abort_cmd(&workdir, &git_dir).unwrap();

    // `git rebase --abort` restores HEAD to the `fixup!` commit fold made
    // before the rebase, so the rollback has to reach one step further back.
    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "abort must leave no `fixup!` commit behind"
    );
    assert_eq!(
        test_repo.read_file("shared.txt"),
        "version-folded",
        "the change being folded comes back to the working tree"
    );
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

#[test]
fn fold_unstaged_deletion_into_head() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    std::fs::remove_file(test_repo.workdir().join("file1.txt")).unwrap();

    let head_oid = test_repo.head_oid();
    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &head_oid.to_string(),
        false,
    );

    assert!(result.is_ok(), "fold of a deletion failed: {:?}", result);
    assert_eq!(test_repo.get_message(0), "Second commit");
    assert!(!test_repo.commit_has_file(test_repo.head_oid(), "file1.txt"));
    test_repo.assert_working_tree_clean();
}

#[test]
fn fold_staged_deletion_into_head() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    std::fs::remove_file(test_repo.workdir().join("file1.txt")).unwrap();
    test_repo.stage_files(&["file1.txt"]);

    let head_oid = test_repo.head_oid();
    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &head_oid.to_string(),
        false,
    );

    assert!(
        result.is_ok(),
        "fold of a staged deletion failed: {:?}",
        result
    );
    assert_eq!(test_repo.get_message(0), "Second commit");
    assert!(!test_repo.commit_has_file(test_repo.head_oid(), "file1.txt"));
    test_repo.assert_working_tree_clean();
}

#[test]
fn fold_staged_deletion_into_non_head_commit() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("First commit", "file1.txt");
    let c2_oid = test_repo.commit("Second commit", "file2.txt");
    test_repo.commit("Third commit", "file3.txt");

    std::fs::remove_file(test_repo.workdir().join("file1.txt")).unwrap();
    test_repo.stage_files(&["file1.txt"]);

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &c2_oid.to_string(),
        false,
    );

    assert!(
        result.is_ok(),
        "fold of a staged deletion failed: {:?}",
        result
    );
    assert_eq!(
        test_repo.commit_messages()[..3],
        [
            "Third commit".to_string(),
            "Second commit".to_string(),
            "First commit".to_string()
        ]
    );
    assert!(!test_repo.commit_has_file(test_repo.get_oid(1), "file1.txt"));
    test_repo.assert_working_tree_clean();
}

/// The saved patch is what is left of the user's work when a rollback cannot
/// replay it, so two failures in a row must not land on the same file.
#[test]
fn save_patch_aside_never_writes_over_an_earlier_save() {
    let test_repo = TestRepo::new();
    test_repo.commit("A commit", "file1.txt");
    let workdir = test_repo.workdir();

    let first = super::save_patch_aside(&workdir, "unrestored", "first patch").unwrap();
    let second = super::save_patch_aside(&workdir, "unrestored", "second patch").unwrap();

    assert_eq!(first.file_name().unwrap(), "unrestored-0.patch");
    assert_eq!(second.file_name().unwrap(), "unrestored-1.patch");
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "first patch");
    assert_eq!(std::fs::read_to_string(&second).unwrap(), "second patch");
    assert!(
        first.starts_with(test_repo.repo.path()),
        "saved under the git dir, not next to the user's files: {}",
        first.display()
    );
}

/// When even the reset fails there is nothing safe to replay onto, so both
/// halves of the snapshot are parked on disk instead of applied blind.
#[test]
fn rollback_fold_parks_both_patches_when_the_reset_fails() {
    let test_repo = TestRepo::new();
    test_repo.commit("A commit", "file1.txt");
    let workdir = test_repo.workdir();
    let snapshot = super::WorktreeSnapshot {
        worktree: "worktree half".to_string(),
        staged: "staged half".to_string(),
    };

    super::rollback_fold(
        &workdir,
        "0123456789abcdef0123456789abcdef01234567",
        None,
        &snapshot,
    );

    let loom_dir = crate::git::git_path(&workdir, "loom").unwrap();
    let saved: Vec<String> = std::fs::read_dir(&loom_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert!(
        saved.contains(&"unrestored-0.patch".to_string()),
        "{saved:?}"
    );
    assert!(
        saved.contains(&"unrestored-staged-0.patch".to_string()),
        "{saved:?}"
    );
    assert_eq!(
        std::fs::read_to_string(loom_dir.join("unrestored-0.patch")).unwrap(),
        "worktree half"
    );
    assert_eq!(
        std::fs::read_to_string(loom_dir.join("unrestored-staged-0.patch")).unwrap(),
        "staged half"
    );
}
