use crate::core::hunk_select::HunkArgs;
use crate::core::repo;
use crate::core::test_helpers::TestRepo;
use crate::core::weave::{Position, Weave};
use crate::tui::hunk_selector::{FileEntry, HunkEntry, HunkOrigin};

// ── Case 1: File(s) + Commit (Amend) ────────────────────────────────────

#[test]
fn fold_file_into_head() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    test_repo.write_file("file1.txt", "modified content");

    let head_oid = test_repo.head_oid();

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &head_oid.to_string(),
        false,
        &[],
    );

    assert!(
        result.is_ok(),
        "fold_files_into_commit failed: {:?}",
        result
    );

    assert_eq!(test_repo.get_message(0), "Second commit");
    assert_ne!(test_repo.head_oid(), head_oid, "Hash should have changed");

    assert_eq!(test_repo.read_file("file1.txt"), "modified content");
}

#[test]
fn fold_multiple_files_into_head() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");

    test_repo.write_file("file1.txt", "modified 1");
    test_repo.write_file("new_file.txt", "new content");

    let head_oid = test_repo.head_oid();

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string(), "new_file.txt".to_string()],
        &head_oid.to_string(),
        false,
        &[],
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

    test_repo.write_file("file1.txt", "amended content");

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &c1_oid.to_string(),
        false,
        &[],
    );

    assert!(
        result.is_ok(),
        "fold_files_into_commit failed: {:?}",
        result
    );

    assert_eq!(test_repo.get_message(0), "Second commit");
    assert_eq!(test_repo.get_message(1), "First commit");

    assert_ne!(test_repo.get_oid(1), c1_oid);
}

#[test]
fn fold_file_no_changes_fails() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");

    let head_oid = test_repo.head_oid();

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &head_oid.to_string(),
        false,
        &[],
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no changes"));
}

#[test]
fn fold_file_into_non_head_with_other_changes_autostashed() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    test_repo.write_file("file1.txt", "change 1");
    test_repo.write_file("file2.txt", "change 2");

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file1.txt".to_string()],
        &c1_oid.to_string(),
        false,
        &[],
    );

    assert!(
        result.is_ok(),
        "fold should succeed with autostash: {:?}",
        result
    );

    assert_eq!(test_repo.read_file("file2.txt"), "change 2");
}

/// Bug: folding a file into a woven branch commit (non-HEAD) where both the
/// commit and working-tree modify the same file would leave unmerged paths.
/// The autostash would pop stale changes that conflict with rewritten history.
#[test]
fn fold_file_into_woven_branch_commit() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feat1", &base_oid.to_string());
    test_repo.switch_branch("feat1");
    test_repo.write_file("feature1", "initial feature content");
    test_repo.stage_files(&["feature1"]);
    test_repo.commit_staged("Feature 1");
    let feat1_oid = test_repo.head_oid();

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feat1");

    test_repo.write_file("feature1", "updated feature content");

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["feature1".to_string()],
        &feat1_oid.to_string(),
        false,
        &[],
    );

    assert!(
        result.is_ok(),
        "fold_files_into_commit (woven branch) failed: {:?}",
        result
    );

    let feat1_new_tip = test_repo.get_branch_target("feat1");
    assert_ne!(feat1_new_tip, feat1_oid, "feat1 should have been rewritten");

    assert_eq!(test_repo.read_file("feature1"), "updated feature content");

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
        super::fold_files_into_commit(&test_repo.repo, &staged, &head_oid.to_string(), true, &[]);
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

    let modified = "line 1\nMODIFIED TOP\nline 3\nline 4\nline 5\n\
                    line 6\nline 7\nline 8\nline 9\nline 10\n\
                    line 11\nline 12\nline 13\nMODIFIED BOTTOM\nline 15\n";
    test_repo.write_file("file.txt", modified);

    let first_hunk_patch = "--- a/file.txt\n+++ b/file.txt\n\
                             @@ -1,5 +1,5 @@\n line 1\n-line 2\n+MODIFIED TOP\n \
                             line 3\n line 4\n line 5\n";
    let workdir = test_repo.workdir();
    crate::git::apply_cached_patch(workdir.as_path(), first_hunk_patch).unwrap();

    let staged = crate::core::repo::get_staged_files(&test_repo.repo).unwrap();
    let result =
        super::fold_files_into_commit(&test_repo.repo, &staged, &target_oid.to_string(), true, &[]);
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

/// Files staged outside the fold are set aside so they cannot join it, and
/// must be staged again afterwards — here across the fixup path, where a whole
/// rebase runs in between.
#[test]
fn fold_into_a_non_head_commit_stages_the_other_files_again() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("file.txt", "target\n");
    test_repo.stage_files(&["file.txt"]);
    test_repo.commit_staged("target commit");
    let target_oid = test_repo.head_oid();

    test_repo.write_file("later.txt", "later\n");
    test_repo.stage_files(&["later.txt"]);
    test_repo.commit_staged("second commit");

    test_repo.write_file("file.txt", "target amended\n");
    test_repo.write_file("kept.txt", "staged, and none of the fold's business\n");
    test_repo.stage_files(&["file.txt", "kept.txt"]);

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file.txt".to_string()],
        &target_oid.to_string(),
        true,
        &[],
    );
    assert!(result.is_ok(), "fold failed: {result:?}");

    let status = test_repo.status_porcelain();
    assert!(status.contains("A  kept.txt"), "{status}");
}

/// The same set-aside work, on the path where the fold fails after the fixup
/// commit exists: nothing durable holds the patch yet, so only the guard can
/// bring it back. A file where `.git/loom` must be a directory is what makes
/// `transaction::save` fail this late.
#[test]
fn a_fold_failing_after_the_fixup_commit_puts_back_the_staging() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("file.txt", "target\n");
    test_repo.stage_files(&["file.txt"]);
    test_repo.commit_staged("target commit");
    let target_oid = test_repo.head_oid();

    test_repo.write_file("later.txt", "later\n");
    test_repo.stage_files(&["later.txt"]);
    test_repo.commit_staged("second commit");

    test_repo.write_file("file.txt", "target amended\n");
    test_repo.write_file("kept.txt", "staged, and none of the fold's business\n");
    test_repo.stage_files(&["file.txt", "kept.txt"]);
    std::fs::write(test_repo.repo.path().join("loom"), "not a directory").unwrap();

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["file.txt".to_string()],
        &target_oid.to_string(),
        true,
        &[],
    );

    assert!(result.is_err(), "the state file cannot be written");
    // The rollback resets over the fixup commit, so only the reflog still shows
    // it. Without this the test passes just as green if the failure ever moves
    // earlier, leaving the window it is named after unguarded.
    let reflog = crate::git::run_git_stdout(&test_repo.workdir(), &["reflog", "--format=%gs"])
        .expect("reflog");
    assert!(
        reflog.contains("fixup! target commit"),
        "the fixup commit must already exist: {reflog}"
    );
    let status = test_repo.status_porcelain();
    assert!(status.contains("A  kept.txt"), "{status}");
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

    assert_eq!(test_repo.get_message(0), "Original feature");

    assert_ne!(test_repo.head_oid(), c1_oid);
    assert_ne!(test_repo.head_oid(), c2_oid);
}

#[test]
fn fold_commit_into_commit_preserves_other_commits() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("First", "file1.txt");
    test_repo.commit("Second", "file2.txt");
    let c3_oid = test_repo.commit("Fix for first", "file1.txt");

    let result =
        super::fold_commit_into_commit(&test_repo.repo, &c3_oid.to_string(), &c1_oid.to_string());

    assert!(result.is_ok(), "fold failed: {:?}", result);

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

    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();

    test_repo.create_branch_at("feature-a", &a1_oid.to_string());

    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.commit("B1", "b1.txt");

    // Manually set up merge topology:
    // Rebase B1 onto merge-base, then merge feature-a
    test_repo.rebase_onto(&base_oid.to_string(), &a1_oid.to_string());
    test_repo.merge_no_ff("feature-a");

    test_repo.commit("C1", "c1.txt");
    let c1_oid = test_repo.head_oid();

    let result = super::fold_commit_to_branch(&test_repo.repo, &c1_oid.to_string(), "feature-a");

    assert!(result.is_ok(), "fold_commit_to_branch failed: {:?}", result);

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
            None,
            HunkArgs::default(),
            vec![commit_sid.clone(), branch_sid.clone()],
            vec![],
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
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();

    test_repo.create_branch_at("feature-a", &a1_oid.to_string());

    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.commit("B1", "b1.txt");

    test_repo.rebase_onto(&base_oid.to_string(), &a1_oid.to_string());
    test_repo.merge_no_ff("feature-a");

    let loose_oid = test_repo.commit("Loose", "loose.txt");

    test_repo.write_file("a1.txt", "dirty");

    let result = super::fold_commit_to_branch(&test_repo.repo, &loose_oid.to_string(), "feature-a");

    assert!(
        result.is_ok(),
        "fold should succeed with autostash: {:?}",
        result
    );

    assert_eq!(test_repo.read_file("a1.txt"), "dirty");
}

#[test]
fn fold_commit_to_colocated_branch_only_affects_target() {
    // Reproduce: two co-located woven branches (feat2 and feat3 sharing the same
    // merge commit), plus a third branch (test) with commits.
    // Moving a commit from 'test' to 'feat3' should put it only on feat3,
    // NOT on feat2.
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

    test_repo.create_branch_at("test", &base_oid.to_string());
    test_repo.switch_branch("test");
    test_repo.commit("Feat1", "feat1.txt");
    test_repo.commit("Feat3", "feat3.txt");
    let feat3_oid = test_repo.head_oid();
    test_repo.switch_branch("integration");

    test_repo.merge_no_ff("test");

    test_repo.create_branch_at("feat2", &base_oid.to_string());
    test_repo.switch_branch("feat2");
    test_repo.commit("Feat2", "feat2.txt");
    test_repo.switch_branch("integration");

    let feat2_tip = test_repo.get_branch_target("feat2");
    test_repo.create_branch_at("feat3", &feat2_tip.to_string());

    test_repo.merge_no_ff("feat2");

    let result = super::fold_commit_to_branch(&test_repo.repo, &feat3_oid.to_string(), "feat3");

    assert!(result.is_ok(), "fold_commit_to_branch failed: {:?}", result);

    assert_eq!(
        test_repo.branch_commit_summary("feat3"),
        "Feat3",
        "feat3 tip should be Feat3"
    );

    assert_eq!(
        test_repo.branch_commit_summary("feat2"),
        "Feat2",
        "feat2 tip should still be Feat2, not Feat3"
    );

    let feat3_commit = test_repo.find_commit(test_repo.get_branch_target("feat3"));
    assert_eq!(
        feat3_commit.parent_id(0).unwrap(),
        test_repo.get_branch_target("feat2"),
        "feat3 should be stacked on feat2"
    );

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
    // This is the real-world scenario: create feat-a with a commit, move it
    // away (leaving feat-a at base with no merge), then move another commit
    // back to feat-a.
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

    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    test_repo.create_branch_at("feature-b", &base_oid.to_string());
    test_repo.switch_branch("feature-b");
    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();
    test_repo.commit("B1", "b1.txt");
    test_repo.switch_branch("integration");

    test_repo.merge_no_ff("feature-b");

    // Verify feature-a has no section in the graph (it's at base, not woven)
    let graph = Weave::from_repo(&test_repo.repo).unwrap();
    assert!(
        !graph.branch_sections.iter().any(|s| s.label == "feature-a"),
        "feature-a should NOT have a section before the fold"
    );

    let result = super::fold_commit_to_branch(&test_repo.repo, &a1_oid.to_string(), "feature-a");
    assert!(result.is_ok(), "fold_commit_to_branch failed: {:?}", result);

    assert_eq!(
        test_repo.branch_commit_summary("feature-a"),
        "A1",
        "feature-a tip should be A1, but branch was not updated (still at base)"
    );

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

    test_repo.create_branch_at("out-of-scope", &base_oid.to_string());
    test_repo.switch_branch("out-of-scope");
    test_repo.commit("Out of scope work", "oos.txt");
    test_repo.switch_branch("integration");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

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

    assert_eq!(test_repo.get_message(0), "First commit");

    assert_eq!(test_repo.read_file("file2.txt"), "Second commit");

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

    assert_eq!(test_repo.get_message(0), "Second commit");

    assert_eq!(test_repo.read_file("file1.txt"), "First commit");
}

#[test]
fn fold_commit_to_unstaged_dirty_autostashed() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");

    test_repo.write_file("file1.txt", "dirty");

    let head_oid = test_repo.head_oid();

    let result = super::fold_commit_to_unstaged(&test_repo.repo, &head_oid.to_string());

    assert!(
        result.is_ok(),
        "fold should succeed with dirty tree: {:?}",
        result
    );

    assert_eq!(test_repo.get_message(0), "First commit");

    assert_eq!(test_repo.read_file("file1.txt"), "dirty");

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

    test_repo.write_file("file1.txt", "modified 1");
    test_repo.write_file("file2.txt", "modified 2");

    let head_oid = test_repo.head_oid();

    let result = test_repo.in_dir(|| {
        super::run(
            false,
            false,
            None,
            HunkArgs::default(),
            vec!["zz".into(), "HEAD".into()],
            vec![],
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
            None,
            HunkArgs::default(),
            vec!["zz".into(), "HEAD".into()],
            vec![],
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

    let result = super::fold_commit_file_to_unstaged(
        &test_repo.repo,
        &head_oid.to_string(),
        "file1.txt",
        &[],
    );

    assert!(
        result.is_ok(),
        "fold_commit_file_to_unstaged failed: {:?}",
        result
    );

    assert_eq!(test_repo.get_message(0), "Two files");

    assert_eq!(test_repo.read_file("file1.txt"), "content1");

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
        super::fold_commit_file_to_unstaged(&test_repo.repo, &c1_oid.to_string(), "file1.txt", &[]);

    assert!(
        result.is_ok(),
        "fold_commit_file_to_unstaged (non-HEAD) failed: {:?}",
        result
    );

    assert_eq!(test_repo.get_message(0), "Second commit");
    assert_eq!(test_repo.get_message(1), "Two files");

    assert_eq!(test_repo.read_file("file1.txt"), "content1");

    assert_eq!(test_repo.read_file("file2.txt"), "content2");
}

/// A submodule bump is a gitlink: `git apply` cannot write one in the working
/// tree and `git add` restages the submodule at its current HEAD, so the bump
/// used to survive the amend and only the commit hash changed.
#[test]
fn fold_commit_file_to_unstaged_submodule() {
    let test_repo = TestRepo::new();
    let (first, second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");

    test_repo.checkout_submodule("Data", second);
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["Data", "other.txt"]);
    test_repo.commit_staged("Bump submodule");
    let head_oid = test_repo.head_oid();

    super::fold_commit_file_to_unstaged(&test_repo.repo, &head_oid.to_string(), "Data", &[])
        .unwrap();

    let new_head = test_repo.head_oid();
    assert_eq!(test_repo.commit_file_paths(new_head), ["other.txt"]);
    assert_eq!(test_repo.submodule_oid(new_head, "Data"), first);
    assert_eq!(test_repo.status_porcelain().trim(), "M Data");
}

#[test]
fn fold_commit_file_to_unstaged_submodule_non_head() {
    let test_repo = TestRepo::new_with_remote();
    let (first, second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");

    test_repo.checkout_submodule("Data", second);
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["Data", "other.txt"]);
    test_repo.commit_staged("Bump submodule");
    let bump_oid = test_repo.head_oid();

    test_repo.write_file("later.txt", "later");
    test_repo.stage_files(&["later.txt"]);
    test_repo.commit_staged("Later");

    super::fold_commit_file_to_unstaged(&test_repo.repo, &bump_oid.to_string(), "Data", &[])
        .unwrap();

    assert_eq!(
        test_repo.commit_file_paths(test_repo.get_oid(1)),
        ["other.txt"]
    );
    assert_eq!(test_repo.submodule_oid(test_repo.head_oid(), "Data"), first);
    assert_eq!(test_repo.status_porcelain().trim(), "M Data");
}

/// Uncommitting the commit that *adds* a submodule reverse-applies a
/// `new file mode 160000` patch, which only `--cached` can turn back into no
/// index entry at all.
#[test]
fn fold_commit_file_to_unstaged_submodule_add() {
    let test_repo = TestRepo::new();
    test_repo.add_submodule("Data");
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["other.txt"]);
    test_repo.commit_staged("Add submodule");
    let head_oid = test_repo.head_oid();

    super::fold_commit_file_to_unstaged(&test_repo.repo, &head_oid.to_string(), "Data", &[])
        .unwrap();

    let new_head = test_repo.head_oid();
    assert!(!test_repo.commit_has_file(new_head, "Data"));
    assert!(test_repo.commit_has_file(new_head, "other.txt"));
    assert_eq!(test_repo.status_porcelain().trim(), "?? Data/");
}

/// A submodule cannot ride in a hunk patch, so a `-p` selection carries it as
/// the commit's own whole-file diff instead. Picking it must move the entry and
/// leave the mode alone; not picking it must leave the entry untouched.
#[test]
fn picked_whole_files_carries_a_submodule() {
    let test_repo = TestRepo::new();
    let (_first, second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");
    test_repo.checkout_submodule("Data", second);
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["Data", "other.txt"]);
    test_repo.commit_staged("Bump submodule");

    let workdir = test_repo.workdir();
    let head = test_repo.head_oid().to_string();
    let entry = |path: &str, selected: bool| FileEntry {
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
        binary: true,
    };

    let picked = [entry("other.txt", true), entry("Data", true)];
    let gitlinks = super::picked_whole_files(&workdir, &head, &picked).unwrap();
    assert_eq!(gitlinks.len(), 1);
    assert_eq!(gitlinks[0].path, "Data");
    assert!(matches!(
        gitlinks[0].kind,
        super::WholeFileKind::Gitlink { removed: false }
    ));
    // The whole-file diff carries the mode a hunk patch cannot.
    assert!(gitlinks[0].diff.contains("160000"));

    let untouched = [entry("other.txt", true), entry("Data", false)];
    assert!(
        super::picked_whole_files(&workdir, &head, &untouched)
            .unwrap()
            .is_empty()
    );
}
/// A submodule that is still checked out cannot show as deleted in the working
/// tree: the restored index entry matches the directory on disk. The removal
/// has to land staged, or it is simply gone.
#[test]
fn fold_commit_file_to_unstaged_submodule_remove_keeps_the_checkout() {
    let test_repo = TestRepo::new();
    let (first, _second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");

    // `--cached` leaves the checkout behind, unlike a plain `git rm`.
    crate::git::run_git(
        &test_repo.workdir(),
        &["rm", "-r", "-q", "--cached", "Data"],
    )
    .unwrap();
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["other.txt"]);
    test_repo.commit_staged("Remove submodule");
    let head_oid = test_repo.head_oid();

    super::fold_commit_file_to_unstaged(&test_repo.repo, &head_oid.to_string(), "Data", &[])
        .unwrap();

    let new_head = test_repo.head_oid();
    assert_eq!(test_repo.submodule_oid(new_head, "Data"), first);
    // The checkout is left on disk, untracked, rather than deleted.
    assert_eq!(
        test_repo.status_porcelain().trim(),
        "D  Data
?? Data/"
    );
}

/// With the checkout gone too, the removal is an ordinary unstaged deletion.
#[test]
fn fold_commit_file_to_unstaged_submodule_remove_drops_the_checkout() {
    let test_repo = TestRepo::new();
    let (first, _second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");

    crate::git::run_git(&test_repo.workdir(), &["rm", "-r", "-q", "Data"]).unwrap();
    test_repo.commit_staged("Remove submodule");
    let head_oid = test_repo.head_oid();

    super::fold_commit_file_to_unstaged(&test_repo.repo, &head_oid.to_string(), "Data", &[])
        .unwrap();

    let new_head = test_repo.head_oid();
    assert_eq!(test_repo.submodule_oid(new_head, "Data"), first);
    assert_eq!(test_repo.status_porcelain().trim(), "D Data");
}

/// Below HEAD the removal is staged after a rebase has already landed, where a
/// miss cannot be rolled back.
#[test]
fn fold_commit_file_to_unstaged_submodule_remove_non_head() {
    let test_repo = TestRepo::new_with_remote();
    let (first, _second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");

    crate::git::run_git(
        &test_repo.workdir(),
        &["rm", "-r", "-q", "--cached", "Data"],
    )
    .unwrap();
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["other.txt"]);
    test_repo.commit_staged("Remove submodule");
    let remove_oid = test_repo.head_oid();

    test_repo.write_file("later.txt", "later");
    test_repo.stage_files(&["later.txt"]);
    test_repo.commit_staged("Later");

    super::fold_commit_file_to_unstaged(&test_repo.repo, &remove_oid.to_string(), "Data", &[])
        .unwrap();

    assert_eq!(test_repo.submodule_oid(test_repo.head_oid(), "Data"), first);
    assert_eq!(
        test_repo.status_porcelain().trim(),
        "D  Data
?? Data/"
    );
}

/// The whole-commit uncommit loses a removal the same way, and needs the same
/// treatment.
#[test]
fn fold_commit_to_unstaged_submodule_remove() {
    let test_repo = TestRepo::new();
    let (first, _second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");

    crate::git::run_git(
        &test_repo.workdir(),
        &["rm", "-r", "-q", "--cached", "Data"],
    )
    .unwrap();
    test_repo.commit_staged("Remove submodule");
    let head_oid = test_repo.head_oid();

    super::fold_commit_to_unstaged(&test_repo.repo, &head_oid.to_string()).unwrap();

    assert_eq!(test_repo.submodule_oid(test_repo.head_oid(), "Data"), first);
    // The checkout is left on disk, untracked, rather than deleted.
    assert_eq!(
        test_repo.status_porcelain().trim(),
        "D  Data
?? Data/"
    );
}

/// Uncommitting a whole commit below HEAD goes through the weave rebase, which
/// must leave the submodule checkout alone for the bump to land unstaged —
/// `submodule.recurse` set or not, since `git rebase` does not honour it.
#[test]
fn fold_commit_to_unstaged_submodule_non_head() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.set_config("submodule.recurse", "true");
    let (first, second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");

    test_repo.checkout_submodule("Data", second);
    test_repo.stage_files(&["Data"]);
    test_repo.commit_staged("Bump submodule");
    let bump_oid = test_repo.head_oid();

    test_repo.write_file("later.txt", "later");
    test_repo.stage_files(&["later.txt"]);
    test_repo.commit_staged("Later");

    super::fold_commit_to_unstaged(&test_repo.repo, &bump_oid.to_string()).unwrap();

    assert_eq!(test_repo.submodule_oid(test_repo.head_oid(), "Data"), first);
    assert_eq!(test_repo.status_porcelain().trim(), "M Data");
}

/// `core.quotePath` is on by default, so git writes a non-ASCII path into the
/// diff header escaped and quoted. Nothing may decide a submodule's fate by
/// reading a path back out of patch text.
#[test]
fn fold_commit_file_to_unstaged_submodule_non_ascii_path() {
    let test_repo = TestRepo::new();
    let (first, second) = test_repo.add_submodule("Dätä");
    test_repo.commit_staged("Add submodule");

    test_repo.checkout_submodule("Dätä", second);
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["Dätä", "other.txt"]);
    test_repo.commit_staged("Bump submodule");
    let head_oid = test_repo.head_oid();

    super::fold_commit_file_to_unstaged(&test_repo.repo, &head_oid.to_string(), "Dätä", &[])
        .unwrap();

    let new_head = test_repo.head_oid();
    assert_eq!(test_repo.commit_file_paths(new_head), ["other.txt"]);
    assert_eq!(test_repo.submodule_oid(new_head, "Dätä"), first);
    // git2 reports the real path; `git status` would quote and escape it.
    let changed: Vec<String> = test_repo
        .repo
        .statuses(None)
        .unwrap()
        .iter()
        .filter_map(|e| e.path().map(str::to_string).ok())
        .collect();
    assert_eq!(changed, ["Dätä"]);
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
        super::fold_commit_file_to_unstaged(&test_repo.repo, &c1_oid.to_string(), "file1.txt", &[])
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

    let result = super::fold_commit_file_to_unstaged(
        &test_repo.repo,
        &head_oid.to_string(),
        "nonexistent.txt",
        &[],
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
        &[],
    );

    assert!(
        result.is_ok(),
        "fold_commit_file_to_commit failed: {:?}",
        result
    );

    assert_eq!(test_repo.get_message(0), "Target commit");
    assert_eq!(test_repo.get_message(1), "Source commit");

    assert_eq!(test_repo.read_file("file1.txt"), "content1");
    assert_eq!(test_repo.read_file("file2.txt"), "content2");
}

#[test]
fn fold_commit_file_to_commit_submodule() {
    let test_repo = TestRepo::new_with_remote();
    let (first, second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");

    test_repo.checkout_submodule("Data", second);
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["Data", "other.txt"]);
    test_repo.commit_staged("Bump submodule");
    let source_oid = test_repo.head_oid();

    test_repo.write_file("later.txt", "later");
    test_repo.stage_files(&["later.txt"]);
    test_repo.commit_staged("Later");
    let target_oid = test_repo.head_oid();

    super::fold_commit_file_to_commit(
        &test_repo.repo,
        &source_oid.to_string(),
        "Data",
        &target_oid.to_string(),
        &[],
    )
    .unwrap();

    let source = test_repo.get_oid(1);
    let target = test_repo.head_oid();
    assert_eq!(test_repo.commit_file_paths(source), ["other.txt"]);
    assert_eq!(test_repo.submodule_oid(source, "Data"), first);
    assert_eq!(test_repo.submodule_oid(target, "Data"), second);
    test_repo.assert_working_tree_clean();
}

/// The source-newer branch runs two rebase phases through `_loom-track`; a
/// gitlink has to survive both.
#[test]
fn fold_commit_file_to_commit_submodule_source_newer() {
    let test_repo = TestRepo::new_with_remote();
    let (first, second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");

    test_repo.write_file("target.txt", "target");
    test_repo.stage_files(&["target.txt"]);
    test_repo.commit_staged("Target commit");
    let target_oid = test_repo.head_oid();

    test_repo.checkout_submodule("Data", second);
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["Data", "other.txt"]);
    test_repo.commit_staged("Source commit");
    let source_oid = test_repo.head_oid();

    super::fold_commit_file_to_commit(
        &test_repo.repo,
        &source_oid.to_string(),
        "Data",
        &target_oid.to_string(),
        &[],
    )
    .unwrap();

    let source = test_repo.head_oid();
    let target = test_repo.get_oid(1);
    assert_eq!(test_repo.commit_file_paths(source), ["other.txt"]);
    assert_eq!(test_repo.submodule_oid(target, "Data"), second);
    assert_ne!(test_repo.submodule_oid(target, "Data"), first);
    test_repo.assert_working_tree_clean();
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
        &[],
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
        &[],
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
        &[],
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

    let result = super::fold_commit_file_to_commit(
        &test_repo.repo,
        &c2_oid.to_string(),
        "file_a.txt",
        &c1_oid.to_string(),
        &[],
    );

    assert!(
        result.is_ok(),
        "fold_commit_file_to_commit (newer→older) failed: {:?}",
        result
    );

    assert_eq!(test_repo.get_message(0), "Add file_b and modify file_a");
    assert_eq!(test_repo.get_message(1), "Add file_a");

    assert_eq!(test_repo.read_file("file_a.txt"), "aaa modified");

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

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");

    test_repo.write_file("fa1.txt", "feature-a file 1");
    test_repo.write_file("fa2.txt", "feature-a file 2");
    test_repo.stage_files(&["fa1.txt", "fa2.txt"]);
    test_repo.commit_staged("A1: two files");

    test_repo.create_branch_at("feature-b", &test_repo.head_oid().to_string());
    test_repo.switch_branch("feature-b");

    test_repo.write_file("fb1.txt", "feature-b file 1");
    test_repo.stage_files(&["fb1.txt"]);
    test_repo.commit_staged("B1: one file");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-b");

    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a should exist before fold"
    );
    assert!(
        test_repo.branch_exists("feature-b"),
        "feature-b should exist before fold"
    );
    assert_eq!(test_repo.read_file("fa1.txt"), "feature-a file 1");

    let fa_tip = test_repo.get_branch_target("feature-a");

    let result =
        super::fold_commit_file_to_unstaged(&test_repo.repo, &fa_tip.to_string(), "fa1.txt", &[]);

    assert!(
        result.is_ok(),
        "fold_commit_file_to_unstaged (stacked branch) failed: {:?}",
        result
    );

    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a branch should still exist after fold"
    );

    assert!(
        test_repo.branch_exists("feature-b"),
        "feature-b branch should still exist after fold"
    );

    assert_eq!(test_repo.read_file("fa1.txt"), "feature-a file 1");

    assert_eq!(test_repo.read_file("fa2.txt"), "feature-a file 2");

    assert_eq!(test_repo.read_file("fb1.txt"), "feature-b file 1");
}

/// Bug: moving a file between commits in a stacked branch should
/// preserve all branch refs. Tests moving from a newer branch commit
/// to an older branch commit (which triggers the direction bug).
#[test]
fn fold_commit_file_to_commit_stacked_branch() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");

    test_repo.write_file("fa1.txt", "feature-a file 1");
    test_repo.stage_files(&["fa1.txt"]);
    test_repo.commit_staged("A1");
    let a1_oid = test_repo.head_oid();

    test_repo.create_branch_at("feature-b", &a1_oid.to_string());
    test_repo.switch_branch("feature-b");

    test_repo.write_file("fb1.txt", "feature-b file 1");
    test_repo.write_file("fb2.txt", "feature-b file 2");
    test_repo.stage_files(&["fb1.txt", "fb2.txt"]);
    test_repo.commit_staged("B1");
    let b1_oid = test_repo.head_oid();

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-b");

    let result = super::fold_commit_file_to_commit(
        &test_repo.repo,
        &b1_oid.to_string(),
        "fb1.txt",
        &a1_oid.to_string(),
        &[],
    );

    assert!(
        result.is_ok(),
        "fold_commit_file_to_commit (stacked branch) failed: {:?}",
        result
    );

    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a should still exist after fold"
    );
    assert!(
        test_repo.branch_exists("feature-b"),
        "feature-b should still exist after fold"
    );

    assert_eq!(test_repo.read_file("fb1.txt"), "feature-b file 1");

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

    test_repo.create_branch_at("foo1", &base_oid.to_string());
    test_repo.switch_branch("foo1");
    test_repo.write_file("feature1", "feat 1");
    test_repo.stage_files(&["feature1"]);
    test_repo.commit_staged("Feature 1");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("foo1");

    test_repo.create_branch_at("foo2", &base_oid.to_string());
    test_repo.switch_branch("foo2");
    test_repo.write_file("feature2", "feat 2");
    test_repo.stage_files(&["feature2"]);
    test_repo.commit_staged("Feature 2");
    let foo2_tip = test_repo.head_oid();

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

    let result = super::fold_commit_file_to_commit(
        &test_repo.repo,
        &foo3_tip.to_string(),
        "feature7",
        &foo2_tip.to_string(),
        &[],
    );

    assert!(
        result.is_ok(),
        "fold_commit_file_to_commit (woven branches) failed: {:?}",
        result
    );

    assert!(test_repo.branch_exists("foo1"), "foo1 should still exist");
    assert!(test_repo.branch_exists("foo2"), "foo2 should still exist");
    assert!(test_repo.branch_exists("foo3"), "foo3 should still exist");

    assert_eq!(test_repo.read_file("feature7"), "feat 7");

    let foo2_new_tip = test_repo.get_branch_target("foo2");
    let foo2_diff = test_repo.diff_commit(&foo2_new_tip.to_string());
    assert!(
        foo2_diff.contains("feature7"),
        "foo2 should now include feature7, but diff is:\n{}",
        foo2_diff
    );

    let foo3_new_tip = test_repo.get_branch_target("foo3");
    let foo3_diff = test_repo.diff_commit(&foo3_new_tip.to_string());
    assert!(
        !foo3_diff.contains("feature7"),
        "foo3 should no longer include feature7, but diff contains:\n{}",
        foo3_diff
    );
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
    let staged_before = crate::git::diff_cached(&workdir).unwrap();

    let result = super::fold_files_into_commit(
        &test_repo.repo,
        &["a1.txt".to_string()],
        &a_oid.to_string(),
        false,
        &[],
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
    assert_eq!(
        crate::git::diff_cached(&workdir).unwrap(),
        staged_before,
        "a refusal before the rebase must not touch the index at all"
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
        &[],
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
            None,
            HunkArgs::default(),
            vec![m2.to_string(), m1.to_string(), "feature-a".to_string()],
            vec![],
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

    test_repo.write_file("file1.txt", "staged content");
    test_repo.stage_files(&["file1.txt"]);

    let head_oid = test_repo.head_oid();

    let result = super::run_staged(&test_repo.repo, &head_oid.to_string(), &[]);
    assert!(result.is_ok(), "run_staged failed: {:?}", result);

    assert_eq!(test_repo.get_message(0), "Second commit");
    assert_ne!(test_repo.head_oid(), head_oid);
    assert_eq!(test_repo.read_file("file1.txt"), "staged content");
}

#[test]
fn fold_staged_nothing_staged_fails() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");

    let head_oid = test_repo.head_oid();

    let result = super::run_staged(&test_repo.repo, &head_oid.to_string(), &[]);
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

    test_repo.write_file("file1.txt", "staged content");
    test_repo.write_file("file2.txt", "unstaged content");
    test_repo.stage_files(&["file1.txt"]);

    let head_oid = test_repo.head_oid();

    let result = super::run_staged(&test_repo.repo, &head_oid.to_string(), &[]);
    assert!(result.is_ok(), "run_staged failed: {:?}", result);

    // Only file1.txt should be in the commit; file2.txt should remain as unstaged
    assert_eq!(test_repo.read_file("file1.txt"), "staged content");
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
    let result = test_repo.in_dir(|| super::run_staged(&test_repo.repo, "feature-a", &[]));
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
        &[],
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
        &[],
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
        &[],
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
        &[],
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

/// The `-p` uncommit of a picked submodule: the entry leaves the commit through
/// the index, and the checkout stays where the user put it, so the bump shows up
/// as an unstaged change exactly like the whole-file form.
#[test]
fn apply_and_amend_uncommits_a_picked_submodule() {
    let test_repo = TestRepo::new();
    let (first, second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");
    test_repo.checkout_submodule("Data", second);
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["Data", "other.txt"]);
    test_repo.commit_staged("Bump submodule");

    let workdir = test_repo.workdir();
    let head = test_repo.head_oid().to_string();
    let selections = [FileEntry {
        path: String::from("Data"),
        hunks: vec![HunkEntry {
            hunk: crate::core::diff::DiffHunk {
                text: String::from("(submodule)"),
                modified_lines: vec![],
            },
            selected: true,
            origin: HunkOrigin::Commit,
        }],
        index_status: 'M',
        worktree_status: ' ',
        binary: true,
    }];
    let gitlinks = super::picked_whole_files(&workdir, &head, &selections).unwrap();

    super::apply_and_amend(&workdir, &selections, "", &gitlinks, true, &[]).unwrap();

    let new_head = test_repo.head_oid();
    assert_eq!(test_repo.commit_file_paths(new_head), ["other.txt"]);
    assert_eq!(test_repo.submodule_oid(new_head, "Data"), first);
    assert_eq!(test_repo.status_porcelain().trim(), "M Data");
}

// ── Relative moves (--above / --below) ─────────────────────────────────

/// Summaries of `branch`, tip first, down to (excluding) `base`.
fn branch_log(test_repo: &TestRepo, branch: &str, base: git2::Oid) -> Vec<String> {
    let mut out = Vec::new();
    let mut commit = test_repo.find_commit(test_repo.get_branch_target(branch));
    while commit.id() != base {
        out.push(commit.summary().unwrap().unwrap().to_string());
        commit = commit.parent(0).unwrap();
    }
    out
}

#[test]
fn fold_commit_below_reorders_within_its_branch() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    let a1_oid = test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    let a3_oid = test_repo.commit("A3", "a3.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

    super::fold_commit_relative(
        &test_repo.repo,
        &a3_oid.to_string(),
        &a1_oid.to_string(),
        Position::Below,
    )
    .expect("moving A3 below A1");

    assert_eq!(
        branch_log(&test_repo, "feature-a", base_oid),
        ["A2", "A1", "A3"]
    );
    let tip = test_repo.get_branch_target("feature-a");
    assert!(test_repo.commit_has_file(tip, "a3.txt"));
    assert!(
        !test_repo.branch_exists("_loom-track"),
        "the tracking branch is cleaned up"
    );
}

/// Moving into another branch below one of its commits parks the emptied
/// source branch, and the target branch keeps its tip commit.
#[test]
fn fold_commit_below_across_branches_parks_the_emptied_source() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    let a2_oid = test_repo.commit("A2", "a2.txt");

    test_repo.create_branch_at("feature-b", &base_oid.to_string());
    test_repo.switch_branch("feature-b");
    let b1_oid = test_repo.commit("B1", "b1.txt");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");
    test_repo.merge_no_ff("feature-b");

    let (_, parked) = super::plan_relative(
        &test_repo.repo,
        &[b1_oid.to_string()],
        &a2_oid.to_string(),
        Position::Below,
    )
    .expect("planning B1 below A2");
    assert_eq!(parked, vec!["feature-b".to_string()]);

    super::fold_commit_relative(
        &test_repo.repo,
        &b1_oid.to_string(),
        &a2_oid.to_string(),
        Position::Below,
    )
    .expect("moving B1 below A2");

    assert_eq!(
        branch_log(&test_repo, "feature-a", base_oid),
        ["A2", "B1", "A1"]
    );
    assert_eq!(test_repo.get_branch_target("feature-b"), base_oid);
}

/// `--above` the tip of a branch is the branch move: the branch advances.
#[test]
fn fold_commits_above_a_branch_tip_advance_the_branch_in_order() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    let a1_oid = test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");
    // Loose commits go before the merge: a libgit2 commit right after a CLI
    // merge works from a stale index and drops the merged files.
    let c1_oid = test_repo.commit("C1", "c1.txt");
    let c2_oid = test_repo.commit("C2", "c2.txt");
    test_repo.merge_no_ff("feature-a");

    super::move_commits_relative_and_report(
        &test_repo.repo,
        &[c1_oid.to_string(), c2_oid.to_string()],
        &a1_oid.to_string(),
        Position::Above,
    )
    .expect("moving C1 and C2 above A1");

    assert_eq!(
        branch_log(&test_repo, "feature-a", base_oid),
        ["C2", "C1", "A1"]
    );
    assert_eq!(
        test_repo.head_commit().parent_count(),
        2,
        "HEAD is the merge again"
    );
}

#[test]
fn fold_file_out_of_a_commit_refuses_when_the_replay_is_dropped() {
    // fold owns most of the edit stops, and this path amends whatever the
    // rebase left at HEAD — the base, once the target is dropped.
    let (t, target) = crate::core::test_helpers::repo_with_dropped_replay();
    let head_before = t.head_oid();
    let alpha_before = t.get_branch_target("alpha");

    let err = super::fold_commit_file_to_unstaged(&t.repo, &target.to_string(), "one.txt", &[])
        .unwrap_err()
        .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(t.head_oid(), head_before, "{err}");
    assert_eq!(t.get_branch_target("alpha"), alpha_before, "{err}");
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()), "{err}");
}

#[test]
fn fold_between_commits_walks_past_a_redundant_one() {
    // Source older than target: one rebase with two `edit` stops, and the
    // commit that replays empty sits between them, on the continue.
    let (t, older, newer) = crate::core::test_helpers::repo_with_a_redundant_commit_between();

    super::fold_commit_file_to_commit(
        &t.repo,
        &older.to_string(),
        "moved.txt",
        &newer.to_string(),
        &[],
    )
    .unwrap();

    assert!(!crate::git::rebase_is_in_progress(t.repo.path()));
    assert!(!t.commit_messages().contains(&"branch change".to_string()));
    let newer_oid = t.get_branch_target("alpha");
    assert!(t.commit_has_file(newer_oid, "moved.txt"));
    assert!(t.commit_has_file(newer_oid, "newer.txt"));
}

#[test]
fn fold_commit_relative_refuses_when_the_moved_commit_replays_empty() {
    // `_loom-track` follows the moved commit; dropping it would slide that ref
    // onto the commit below and report it as the one that moved.
    let (t, redundant, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_below();
    let head_before = t.head_oid();
    let alpha_before = t.get_branch_target("alpha");
    let staged_before = stage_a_tracked_edit(&t);

    let err = super::fold_commit_relative(
        &t.repo,
        &redundant.to_string(),
        &keeper.to_string(),
        Position::Above,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(t.head_oid(), head_before, "{err}");
    assert_eq!(t.get_branch_target("alpha"), alpha_before, "{err}");
    assert!(!t.branch_exists("_loom-track"), "{err}");
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()), "{err}");
    assert!(
        !t.repo.path().join("loom").join("state.json").exists(),
        "{err}"
    );
    assert_eq!(
        crate::git::diff_cached(&t.workdir()).unwrap(),
        staged_before,
        "{err}"
    );
}

#[test]
fn fold_commit_into_commit_refuses_when_the_target_replays_empty() {
    // Here `_loom-track` follows the target, and the fixup would land on
    // whatever commit git left below it.
    let (t, redundant, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_below();
    let head_before = t.head_oid();
    let alpha_before = t.get_branch_target("alpha");
    let staged_before = stage_a_tracked_edit(&t);

    let err = super::fold_commit_into_commit(&t.repo, &keeper.to_string(), &redundant.to_string())
        .unwrap_err()
        .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(t.head_oid(), head_before, "{err}");
    assert_eq!(t.get_branch_target("alpha"), alpha_before, "{err}");
    assert!(!t.branch_exists("_loom-track"), "{err}");
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()), "{err}");
    assert!(
        !t.repo.path().join("loom").join("state.json").exists(),
        "{err}"
    );
    assert_eq!(
        crate::git::diff_cached(&t.workdir()).unwrap(),
        staged_before,
        "{err}"
    );
}

#[test]
fn fold_files_into_commit_refuses_when_the_target_replays_empty() {
    // The `fixup!` commit this path makes first has to come back too.
    let (t, redundant, _keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_below();
    let head_before = t.head_oid();
    t.write_file("three.txt", "folded\n");

    let err = super::fold_files_into_commit(
        &t.repo,
        &["three.txt".to_string()],
        &redundant.to_string(),
        false,
        &[],
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(t.head_oid(), head_before, "{err}");
    assert!(!t.branch_exists("_loom-track"), "{err}");
    assert_eq!(t.read_file("three.txt"), "folded\n", "{err}");
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()), "{err}");
    assert!(
        !t.repo.path().join("loom").join("state.json").exists(),
        "{err}"
    );
}

#[test]
fn fold_commit_to_branch_refuses_when_the_moved_commit_replays_empty() {
    // The branch ref is what names the moved commit afterwards, so a drop
    // would report the tip it was appended to.
    let (t, redundant, _keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_below();
    t.create_branch_at(
        "beta",
        &t.find_remote_branch_target("origin/main").to_string(),
    );
    let head_before = t.head_oid();
    let beta_before = t.get_branch_target("beta");

    let err = super::fold_commit_to_branch(&t.repo, &redundant.to_string(), "beta")
        .unwrap_err()
        .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(t.head_oid(), head_before, "{err}");
    assert_eq!(t.get_branch_target("beta"), beta_before, "{err}");
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()), "{err}");
    assert!(
        !t.repo.path().join("loom").join("state.json").exists(),
        "{err}"
    );
}

#[test]
fn fold_several_commits_to_a_branch_refuses_when_one_replays_empty() {
    // The multi-commit move reports how many commits it moved, so one dropped
    // as empty is a commit the user is told moved and cannot find.
    let (t, redundant, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_below();
    t.create_branch_at(
        "beta",
        &t.find_remote_branch_target("origin/main").to_string(),
    );
    let head_before = t.head_oid();

    let err = super::move_commits_and_report(
        &t.workdir(),
        &t.repo,
        &[redundant.to_string(), keeper.to_string()],
        "beta",
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(t.head_oid(), head_before, "{err}");
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()), "{err}");
}

/// Phase 2 rewrites the target underneath the source, so the source hash
/// phase 1 produced is stale by the time the move is reported.
#[test]
fn fold_patch_between_commits_names_the_source_that_survives_phase_two() {
    let t = TestRepo::new_with_remote();
    let body = "1\n2\n3\n4\n5\n6\n7\n8\n";
    t.commit_multi(&[("f.txt", body)], "Base");
    let target = t.commit_multi(&[("t.txt", "target\n")], "Target");
    let moved = body.replace("2\n", "TWO\n");
    let source = t.commit_multi(&[("f.txt", &moved), ("g.txt", "g\n")], "Source");

    let workdir = t.workdir();
    let mut selections =
        crate::core::staging::collect_commit_hunks(&workdir, &source.to_string(), &[]).unwrap();
    for file in &mut selections {
        if file.path == "f.txt" {
            for hunk in &mut file.hunks {
                hunk.selected = true;
            }
        }
    }

    let (new_source, new_target) = super::fold_selected_hunks_to_commit(
        &t.repo,
        &workdir,
        &source.to_string(),
        &target.to_string(),
        "Target",
        &selections,
        &[],
    )
    .unwrap();

    assert_eq!(new_source, t.head_oid().to_string());
    assert_eq!(new_target, t.get_oid(1).to_string());
    assert_eq!(t.commit_file_paths(t.head_oid()), ["g.txt"]);
    assert!(t.commit_has_file(t.get_oid(1), "f.txt"));
    assert!(!t.branch_exists("_loom-track"));
}

#[test]
fn fold_several_commits_to_a_branch_refusal_leaves_the_index_as_it_was() {
    // The refusal aborts a rebase that has already autostashed.
    let (t, redundant, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_below();
    t.create_branch_at(
        "beta",
        &t.find_remote_branch_target("origin/main").to_string(),
    );
    let staged_before = stage_a_tracked_edit(&t);

    let err = super::move_commits_and_report(
        &t.workdir(),
        &t.repo,
        &[redundant.to_string(), keeper.to_string()],
        "beta",
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(
        crate::git::diff_cached(&t.workdir()).unwrap(),
        staged_before
    );
}

#[test]
fn fold_several_commits_relative_refusal_leaves_the_index_as_it_was() {
    let (t, redundant, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_below();
    // Somewhere in the weave to move them to: the base is upstream, not in it.
    let anchor = t.commit_multi(&[("anchor.txt", "anchor\n")], "anchor");
    let staged_before = stage_a_tracked_edit(&t);

    let err = super::move_commits_relative_and_report(
        &t.repo,
        &[redundant.to_string(), keeper.to_string()],
        &anchor.to_string(),
        Position::Above,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(
        crate::git::diff_cached(&t.workdir()).unwrap(),
        staged_before
    );
}

/// A staged edit to a *tracked* file is the case that goes wrong: git's
/// autostash replay brings a staged new file back staged, but a modification
/// comes back unstaged.
fn stage_a_tracked_edit(t: &TestRepo) -> String {
    t.write_file("three.txt", "three\nstaged edit\n");
    t.stage_files(&["three.txt"]);
    crate::git::diff_cached(&t.workdir()).unwrap()
}

// ── Whole-file picks for `-p` ─────────────────────────────────────────

/// A commit that deletes `gone.txt` and changes `kept.txt`: one entry the
/// picker offers as a whole file, beside one it offers as a hunk.
fn deleted_and_changed() -> TestRepo {
    let test_repo = TestRepo::new();
    test_repo.commit("Add gone", "gone.txt");
    test_repo.commit("Add kept", "kept.txt");
    std::fs::remove_file(test_repo.workdir().join("gone.txt")).unwrap();
    test_repo.write_file("kept.txt", "changed");
    test_repo.stage_files(&["gone.txt", "kept.txt"]);
    test_repo.commit_staged("Delete one, change another");
    test_repo
}

/// A commit that changes a binary file, the one thing `-p` cannot move.
fn changed_binary() -> TestRepo {
    let test_repo = TestRepo::new();
    test_repo.commit("Add kept", "kept.txt");
    test_repo.write_file("blob.bin", "\u{0}\u{1}old\u{0}");
    test_repo.stage_files(&["blob.bin"]);
    test_repo.commit_staged("Add binary");
    test_repo.write_file("blob.bin", "\u{0}\u{1}new\u{0}");
    test_repo.write_file("kept.txt", "changed");
    test_repo.stage_files(&["blob.bin", "kept.txt"]);
    test_repo.commit_staged("Change binary and another");
    test_repo
}

/// Every hunk of `oid`'s diff, selected, as the picker would hand them over.
fn all_selected(test_repo: &TestRepo, oid: git2::Oid) -> Vec<FileEntry> {
    let mut entries =
        crate::core::staging::collect_commit_hunks(&test_repo.workdir(), &oid.to_string(), &[])
            .unwrap();
    for file in &mut entries {
        for hunk in &mut file.hunks {
            hunk.selected = true;
        }
    }
    entries
}

fn picked_whole_files(
    test_repo: &TestRepo,
    oid: git2::Oid,
    selections: &[FileEntry],
) -> Vec<super::PickedWholeFile> {
    super::picked_whole_files(&test_repo.workdir(), &oid.to_string(), selections).unwrap()
}

/// A deleted file's whole-file label is not a hunk. Letting it through built
/// `(file deleted)--- a/kept.txt`, which `git apply` rejects mid-rebase
/// instead of the operation refusing up front.
#[test]
fn build_selected_patch_leaves_out_a_deleted_file() {
    let test_repo = deleted_and_changed();

    let patch = super::build_selected_patch(&all_selected(&test_repo, test_repo.head_oid()));

    assert!(!patch.contains("gone.txt"), "{patch}");
    assert!(!patch.contains("(file deleted)"), "{patch}");
    assert!(
        patch.starts_with("--- a/kept.txt\n+++ b/kept.txt\n@@"),
        "{patch}"
    );
}

/// A binary file has no hunk either, and the header test is what catches it:
/// `build_selected_patch` no longer reads `file.binary`.
#[test]
fn build_selected_patch_leaves_out_a_binary_file() {
    let test_repo = changed_binary();

    let selections = all_selected(&test_repo, test_repo.head_oid());
    let binary = selections.iter().find(|f| f.path == "blob.bin").unwrap();
    assert!(!binary.hunks[0].hunk.is_text());

    let patch = super::build_selected_patch(&selections);
    assert!(!patch.contains("blob.bin"), "{patch}");
}

/// The deletion travels as the commit's own diff, which alone carries the
/// `deleted file mode` header a hunk patch has no way to write.
#[test]
fn picked_whole_files_carries_a_deletion() {
    let test_repo = deleted_and_changed();
    let head = test_repo.head_oid();

    let selections = all_selected(&test_repo, head);
    let picked = picked_whole_files(&test_repo, head, &selections);

    assert_eq!(picked.len(), 1);
    assert_eq!(picked[0].path, "gone.txt");
    assert!(matches!(picked[0].kind, super::WholeFileKind::Deletion));
    assert!(picked[0].diff.contains("deleted file mode"));

    let mut unpicked = all_selected(&test_repo, head);
    for file in &mut unpicked {
        if file.path == "gone.txt" {
            file.hunks.iter_mut().for_each(|h| h.selected = false);
        }
    }
    assert!(picked_whole_files(&test_repo, head, &unpicked).is_empty());
}

/// Phase one of a move: reversing the selection out of the source commit has
/// to write the file back and stage it, or the deletion stays put.
#[test]
fn a_picked_deletion_leaves_the_source_commit() {
    let test_repo = deleted_and_changed();
    let workdir = test_repo.workdir();
    let head = test_repo.head_oid();
    let selections = all_selected(&test_repo, head);
    let picked = picked_whole_files(&test_repo, head, &selections);
    let patch = super::build_selected_patch(&selections);

    super::apply_and_amend(&workdir, &selections, &patch, &picked, true, &[]).unwrap();

    let amended = crate::git::diff_commit_name_status(&workdir, "HEAD").unwrap();
    assert!(amended.is_empty(), "{amended:?}");
    assert_eq!(test_repo.read_file("gone.txt"), "Add gone");
    test_repo.assert_working_tree_clean();
}

/// `git add` refuses a path an ignore rule matches, so restoring a deletion by
/// staging the file back could not work for a file committed before it was
/// gitignored. The index is written by the apply itself instead.
#[test]
fn a_picked_deletion_of_an_ignored_file_still_moves() {
    let test_repo = TestRepo::new();
    test_repo.commit("Add gone", "gone.txt");
    test_repo.commit("Add kept", "kept.txt");
    test_repo.write_file(".gitignore", "gone.txt\n");
    test_repo.stage_files(&[".gitignore"]);
    test_repo.commit_staged("Ignore it");
    std::fs::remove_file(test_repo.workdir().join("gone.txt")).unwrap();
    test_repo.write_file("kept.txt", "changed");
    test_repo.stage_files(&["gone.txt", "kept.txt"]);
    test_repo.commit_staged("Delete one, change another");

    let workdir = test_repo.workdir();
    let head = test_repo.head_oid();
    let selections = all_selected(&test_repo, head);
    let picked = picked_whole_files(&test_repo, head, &selections);
    let patch = super::build_selected_patch(&selections);

    super::apply_and_amend(&workdir, &selections, &patch, &picked, true, &[]).unwrap();

    assert_eq!(test_repo.read_file("gone.txt"), "Add gone");
    let amended = crate::git::diff_commit_name_status(&workdir, "HEAD").unwrap();
    assert!(amended.is_empty(), "{amended:?}");
    test_repo.assert_working_tree_clean();
}

/// A deleted file is one entry before it is anything else, binary included:
/// `collect_commit_hunks` tests the status first. Its diff carries no content,
/// only `--full-index` blob ids, which is enough to write the file back.
#[test]
fn a_picked_deletion_of_a_binary_file_still_moves() {
    let test_repo = TestRepo::new();
    test_repo.commit("Add kept", "kept.txt");
    test_repo.write_file("blob.bin", "\u{0}\u{1}old\u{0}");
    test_repo.stage_files(&["blob.bin"]);
    test_repo.commit_staged("Add binary");
    std::fs::remove_file(test_repo.workdir().join("blob.bin")).unwrap();
    test_repo.write_file("kept.txt", "changed");
    test_repo.stage_files(&["blob.bin", "kept.txt"]);
    test_repo.commit_staged("Delete the binary, change another");

    let workdir = test_repo.workdir();
    let head = test_repo.head_oid();
    let selections = all_selected(&test_repo, head);
    let picked = picked_whole_files(&test_repo, head, &selections);
    assert!(matches!(picked[0].kind, super::WholeFileKind::Deletion));
    let patch = super::build_selected_patch(&selections);

    super::apply_and_amend(&workdir, &selections, &patch, &picked, true, &[]).unwrap();

    assert_eq!(test_repo.read_file("blob.bin"), "\u{0}\u{1}old\u{0}");
    let amended = crate::git::diff_commit_name_status(&workdir, "HEAD").unwrap();
    assert!(amended.is_empty(), "{amended:?}");
    test_repo.assert_working_tree_clean();
}

/// An empty file's deletion diff is a header and nothing else — no hunk body
/// for the apply to work from.
#[test]
fn a_picked_deletion_of_an_empty_file_still_moves() {
    let test_repo = TestRepo::new();
    test_repo.commit("Add kept", "kept.txt");
    test_repo.write_file("empty.txt", "");
    test_repo.stage_files(&["empty.txt"]);
    test_repo.commit_staged("Add empty");
    std::fs::remove_file(test_repo.workdir().join("empty.txt")).unwrap();
    test_repo.write_file("kept.txt", "changed");
    test_repo.stage_files(&["empty.txt", "kept.txt"]);
    test_repo.commit_staged("Delete the empty one, change another");

    let workdir = test_repo.workdir();
    let head = test_repo.head_oid();
    let selections = all_selected(&test_repo, head);
    let picked = picked_whole_files(&test_repo, head, &selections);
    let patch = super::build_selected_patch(&selections);

    super::apply_and_amend(&workdir, &selections, &patch, &picked, true, &[]).unwrap();

    assert_eq!(test_repo.read_file("empty.txt"), "");
    let amended = crate::git::diff_commit_name_status(&workdir, "HEAD").unwrap();
    assert!(amended.is_empty(), "{amended:?}");
    test_repo.assert_working_tree_clean();
}

/// Phase two: the same selection applied forward onto the target commit, where
/// the file still exists.
#[test]
fn a_picked_deletion_enters_the_target_commit() {
    let test_repo = deleted_and_changed();
    let workdir = test_repo.workdir();
    let head = test_repo.head_oid();
    let selections = all_selected(&test_repo, head);
    let picked = picked_whole_files(&test_repo, head, &selections);
    let patch = super::build_selected_patch(&selections);

    // Stand where the rebase pauses on the target: one commit below the source.
    test_repo.reset_hard(test_repo.get_oid(1));

    super::apply_and_amend(&workdir, &selections, &patch, &picked, false, &[]).unwrap();

    let amended = crate::git::diff_commit_name_status(&workdir, "HEAD").unwrap();
    assert!(
        amended.contains(&('D', "gone.txt".to_string())),
        "{amended:?}"
    );
    test_repo.assert_working_tree_clean();
}

/// `fold -p <commit> zz` on a deletion: the commit gets the file back, and the
/// deletion itself shows up unstaged, like any other uncommitted hunk.
#[test]
fn a_picked_deletion_uncommits_as_an_unstaged_deletion() {
    let test_repo = deleted_and_changed();
    let workdir = test_repo.workdir();
    let head = test_repo.head_oid();
    let selections = all_selected(&test_repo, head);
    let picked = picked_whole_files(&test_repo, head, &selections);
    let patch = super::build_selected_patch(&selections);

    super::apply_and_amend(&workdir, &selections, &patch, &picked, true, &[]).unwrap();
    super::restore_to_worktree(&workdir, &patch, &picked).unwrap();

    assert!(!workdir.join("gone.txt").exists());
    let status = test_repo.status_porcelain();
    assert!(status.contains(" D gone.txt"), "{status}");
    assert!(status.contains(" M kept.txt"), "{status}");
}

/// With nothing left, the guard that states the rule is what the caller hits
/// (Spec 007), rather than a rebase that dies on a malformed patch.
#[test]
fn a_binary_file_alone_leaves_nothing_to_fold() {
    let test_repo = TestRepo::new();
    test_repo.commit("Add kept", "kept.txt");
    test_repo.write_file("blob.bin", "\u{0}\u{1}old\u{0}");
    test_repo.stage_files(&["blob.bin"]);
    test_repo.commit_staged("Add binary");
    test_repo.write_file("blob.bin", "\u{0}\u{1}new\u{0}");
    test_repo.stage_files(&["blob.bin"]);
    test_repo.commit_staged("Change binary");

    let head = test_repo.head_oid();
    let selections = all_selected(&test_repo, head);
    assert_eq!(selections.len(), 1, "the binary file is offered anyway");

    let picked = picked_whole_files(&test_repo, head, &selections);
    let err = super::build_movable_patch(&selections, &picked, "cd").unwrap_err();
    assert_eq!(
        err.to_string(),
        "No text hunks selected — binary files are not supported with -p"
    );
}

/// The warning names what stays behind, and never a file that travels whole.
#[test]
fn unmovable_picks_names_only_what_stays_behind() {
    let test_repo = changed_binary();
    let head = test_repo.head_oid();

    let selections = all_selected(&test_repo, head);
    assert_eq!(super::unmovable_picks(&selections, &[]), ["blob.bin"]);

    // Left unpicked, it is nobody's business.
    let mut unpicked = all_selected(&test_repo, head);
    for file in &mut unpicked {
        if file.path == "blob.bin" {
            file.hunks.iter_mut().for_each(|h| h.selected = false);
        }
    }
    assert!(super::unmovable_picks(&unpicked, &[]).is_empty());
}

#[test]
fn unmovable_picks_leaves_out_a_picked_deletion() {
    let test_repo = deleted_and_changed();
    let head = test_repo.head_oid();

    let selections = all_selected(&test_repo, head);
    let picked = picked_whole_files(&test_repo, head, &selections);

    assert!(super::unmovable_picks(&selections, &picked).is_empty());
}

/// A path is a pathspec, and `[1]` is a glob: the commit's diff for
/// `a[1].txt` also carried `a1.txt`, so picking the deletion moved a file
/// nobody selected. Also the only test that folds a deletion on its own, with
/// no hunk beside it.
#[test]
fn a_picked_deletion_with_a_glob_in_its_name_moves_alone() {
    let test_repo = TestRepo::new();
    let workdir = test_repo.workdir();
    test_repo.commit("Add a1", "a1.txt");
    test_repo.write_file("a[1].txt", "bracket");
    // Not `stage_files`: `git add` reads its paths as pathspecs too.
    crate::git::run_git(&workdir, &["add", "-A"]).unwrap();
    test_repo.commit_staged("Add bracket");
    std::fs::remove_file(workdir.join("a[1].txt")).unwrap();
    test_repo.write_file("a1.txt", "changed");
    crate::git::run_git(&workdir, &["add", "-A"]).unwrap();
    test_repo.commit_staged("Delete bracket, change a1");

    let head = test_repo.head_oid();
    let mut selections = all_selected(&test_repo, head);
    for file in &mut selections {
        if file.path != "a[1].txt" {
            file.hunks.iter_mut().for_each(|h| h.selected = false);
        }
    }
    let picked = picked_whole_files(&test_repo, head, &selections);
    assert_eq!(picked.len(), 1);
    assert!(!picked[0].diff.contains("a/a1.txt"), "{}", picked[0].diff);

    let patch = super::build_selected_patch(&selections);
    assert!(patch.is_empty(), "only the deletion was picked: {patch}");

    super::apply_and_amend(&workdir, &selections, &patch, &picked, true, &[]).unwrap();

    // The deletion left the commit; the change nobody picked stayed in it.
    let amended = crate::git::diff_commit_name_status(&workdir, "HEAD").unwrap();
    assert_eq!(amended, [('M', String::from("a1.txt"))], "{amended:?}");
    assert_eq!(test_repo.read_file("a[1].txt"), "bracket");
}

/// A removed submodule answers to both kinds: `commit_gitlinks` maps it, and
/// the commit's name-status calls it a deletion. Gitlink has to win — `--index`
/// would push the 160000 entry at the working tree, and the removal report
/// would stop firing.
#[test]
fn a_picked_submodule_removal_stays_a_gitlink() {
    let test_repo = TestRepo::new();
    let (first, _second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");
    // `--cached` leaves the checkout behind, unlike a plain `git rm`.
    crate::git::run_git(
        &test_repo.workdir(),
        &["rm", "-r", "-q", "--cached", "Data"],
    )
    .unwrap();
    test_repo.write_file("other.txt", "other");
    test_repo.stage_files(&["other.txt"]);
    test_repo.commit_staged("Remove submodule");

    let workdir = test_repo.workdir();
    let head = test_repo.head_oid();
    let mut selections = all_selected(&test_repo, head);
    for file in &mut selections {
        if file.path != "Data" {
            file.hunks.iter_mut().for_each(|h| h.selected = false);
        }
    }
    let picked = picked_whole_files(&test_repo, head, &selections);

    assert_eq!(picked.len(), 1);
    assert!(matches!(
        picked[0].kind,
        super::WholeFileKind::Gitlink { removed: true }
    ));

    super::apply_and_amend(&workdir, &selections, "", &picked, true, &[]).unwrap();

    assert_eq!(test_repo.submodule_oid(test_repo.head_oid(), "Data"), first);
    assert!(workdir.join("Data").exists(), "the checkout stays on disk");
}

#[test]
fn unmovable_picks_leaves_out_a_picked_submodule() {
    let test_repo = TestRepo::new();
    let (_first, second) = test_repo.add_submodule("Data");
    test_repo.commit_staged("Add submodule");
    test_repo.checkout_submodule("Data", second);
    test_repo.stage_files(&["Data"]);
    test_repo.commit_staged("Bump submodule");

    let head = test_repo.head_oid();
    let selections = all_selected(&test_repo, head);
    let picked = picked_whole_files(&test_repo, head, &selections);

    assert_eq!(picked.len(), 1, "the submodule is picked up whole");
    assert!(super::unmovable_picks(&selections, &picked).is_empty());
}

/// The wording is quoted in `docs/src/commands/fold.md`, and the fold still
/// goes ahead on the hunks it can move.
#[test]
fn a_left_behind_file_is_named_and_the_fold_proceeds() {
    let test_repo = changed_binary();
    let head = test_repo.head_oid();
    let selections = all_selected(&test_repo, head);
    let picked = picked_whole_files(&test_repo, head, &selections);

    let patch = super::build_movable_patch(&selections, &picked, "zz").unwrap();
    assert!(patch.starts_with("--- a/kept.txt"), "{patch}");

    // A `<commit>:<index>` id only resolves through the short-ID allocator, so
    // the hint names where to read one instead of printing one that will not.
    assert_eq!(
        super::unmovable_warning(&["blob.bin"], "zz"),
        "Left behind, no hunk to move: blob.bin\n\
         To move one whole, take its `<commit>:<index>` id from `loom status -f` \
         and run `loom fold <id> zz`"
    );
}

// ── Staging survives a rebase that completed ─────────────────────────────

/// `origin/main` → B1 → merge(A1, `feature-a`) → C1 (loose, on integration).
/// `b1.txt` is the tracked file a move leaves where it is.
fn woven_repo_with_a_loose_commit() -> TestRepo {
    let t = TestRepo::new_with_remote();
    let a1 = t.commit("A1", "a1.txt");
    t.create_branch_at("feature-a", &a1.to_string());
    let base = t.find_remote_branch_target("origin/main");
    t.commit("B1", "b1.txt");
    t.rebase_onto(&base.to_string(), &a1.to_string());
    t.merge_no_ff("feature-a");
    t.commit("C1", "c1.txt");
    t
}

/// A staged edit to a tracked file, plus a staged new file: the autostash
/// replay brings the new file back staged, the modification unstaged.
fn stage_a_mix(t: &TestRepo, tracked: &str) -> String {
    t.write_file(tracked, "staged edit\n");
    t.write_file("brand-new.txt", "new\n");
    t.stage_files(&[tracked, "brand-new.txt"]);
    t.status_porcelain()
}

#[test]
fn fold_commit_into_commit_keeps_staging_on_success() {
    let t = TestRepo::new_with_remote();
    let c1 = t.commit("First", "first.txt");
    t.commit("Second", "second.txt");
    let c3 = t.commit("Third", "third.txt");
    let before = stage_a_mix(&t, "first.txt");

    super::fold_commit_into_commit(&t.repo, &c3.to_string(), &c1.to_string()).unwrap();

    assert_eq!(t.status_porcelain(), before);
}

#[test]
fn fold_commit_to_branch_keeps_staging_on_success() {
    let t = woven_repo_with_a_loose_commit();
    let loose = t.head_oid();
    let before = stage_a_mix(&t, "b1.txt");

    super::fold_commit_to_branch(&t.repo, &loose.to_string(), "feature-a").unwrap();

    assert_eq!(t.status_porcelain(), before);
}

#[test]
fn move_commits_to_branch_keeps_staging_on_success() {
    let t = woven_repo_with_a_loose_commit();
    let c1 = t.head_oid();
    let c2 = t.commit("C2", "c2.txt");
    let before = stage_a_mix(&t, "b1.txt");

    let workdir = t.workdir();
    super::move_commits_and_report(
        &workdir,
        &t.repo,
        &[c1.to_string(), c2.to_string()],
        "feature-a",
        None,
    )
    .unwrap();

    assert_eq!(t.status_porcelain(), before);
}

#[test]
fn fold_commit_relative_keeps_staging_on_success() {
    let t = TestRepo::new_with_remote();
    let base = t.find_remote_branch_target("origin/main").to_string();
    t.create_branch_at("feature-a", &base);
    t.switch_branch("feature-a");
    let a1 = t.commit("A1", "a1.txt");
    t.commit("A2", "a2.txt");
    let a3 = t.commit("A3", "a3.txt");
    t.switch_branch("integration");
    t.merge_no_ff("feature-a");
    let before = stage_a_mix(&t, "a1.txt");

    super::fold_commit_relative(&t.repo, &a3.to_string(), &a1.to_string(), Position::Below)
        .unwrap();

    assert_eq!(t.status_porcelain(), before);
}

#[test]
fn move_commits_relative_keeps_staging_on_success() {
    let t = TestRepo::new_with_remote();
    let base = t.find_remote_branch_target("origin/main").to_string();
    t.create_branch_at("feature-a", &base);
    t.switch_branch("feature-a");
    let a1 = t.commit("A1", "a1.txt");
    t.commit("A2", "a2.txt");
    let a3 = t.commit("A3", "a3.txt");
    let a4 = t.commit("A4", "a4.txt");
    t.switch_branch("integration");
    t.merge_no_ff("feature-a");
    let before = stage_a_mix(&t, "a1.txt");

    super::move_commits_relative_and_report(
        &t.repo,
        &[a3.to_string(), a4.to_string()],
        &a1.to_string(),
        Position::Below,
    )
    .unwrap();

    assert_eq!(t.status_porcelain(), before);
}

/// `loom continue` finishes the rebase, so it owns the restore the `Completed`
/// arm would have done.
#[test]
fn fold_keeps_staging_across_continue() {
    let t = TestRepo::new_with_remote();

    // Folding C into A rewrites `shared.txt`; replaying B then expects A's
    // original content → conflict.
    let a_oid = t.commit("version-a", "shared.txt");
    t.write_file("shared.txt", "version-b");
    t.stage_files(&["shared.txt"]);
    t.commit_staged("Commit B");
    t.write_file("shared.txt", "version-c");
    t.stage_files(&["shared.txt"]);
    t.commit_staged("Commit C");
    let c_oid = t.head_oid();

    t.write_file("bystander.txt", "bystander\n");
    t.stage_files(&["bystander.txt"]);
    t.commit_staged("Commit D");
    let before = stage_a_mix(&t, "bystander.txt");

    super::fold_commit_into_commit(&t.repo, &c_oid.to_string(), &a_oid.to_string()).unwrap();
    assert!(crate::git::rebase_is_in_progress(t.repo.path()));

    t.write_file("shared.txt", "version-b");
    t.stage_files(&["shared.txt"]);
    let workdir = t.workdir();
    crate::core::transaction::continue_cmd(&workdir, t.repo.path()).unwrap();

    assert!(!crate::git::rebase_is_in_progress(t.repo.path()));
    assert_eq!(t.status_porcelain(), before);
}

#[test]
fn fold_commit_to_unstaged_keeps_staging_on_success() {
    let t = TestRepo::new_with_remote();
    t.commit("First", "first.txt");
    let c2 = t.commit("Second", "second.txt");
    t.commit("Third", "third.txt");
    let before = stage_a_mix(&t, "first.txt");

    super::fold_commit_to_unstaged(&t.repo, &c2.to_string()).unwrap();

    // The uncommitted commit's own file lands unstaged on top; the staged set
    // is what has to be unchanged.
    let after = t.status_porcelain();
    let staged: Vec<&str> = after.lines().filter(|l| !l.starts_with("??")).collect();
    assert_eq!(staged.join("\n") + "\n", before);
    assert!(after.contains("?? second.txt"));
}

#[test]
fn fold_commit_file_to_unstaged_keeps_staging_on_success() {
    let t = TestRepo::new_with_remote();
    t.commit("First", "first.txt");
    let c2 = t.commit_multi(&[("a.txt", "a\n"), ("b.txt", "b\n")], "Second");
    t.commit("Third", "third.txt");
    let before = stage_a_mix(&t, "first.txt");

    super::fold_commit_file_to_unstaged(&t.repo, &c2.to_string(), "a.txt", &[]).unwrap();

    // `a.txt` leaves the commit and lands untracked; the staged set is what
    // has to be unchanged.
    let after = t.status_porcelain();
    let staged: Vec<&str> = after.lines().filter(|l| !l.starts_with("??")).collect();
    assert_eq!(staged.join("\n") + "\n", before);
    assert!(after.contains("?? a.txt"));
}

#[test]
fn fold_commit_file_to_commit_keeps_staging_on_success() {
    let t = TestRepo::new_with_remote();
    let c1 = t.commit("First", "first.txt");
    let c2 = t.commit_multi(&[("a.txt", "a\n"), ("b.txt", "b\n")], "Second");
    t.commit("Third", "third.txt");
    let before = stage_a_mix(&t, "first.txt");

    super::fold_commit_file_to_commit(&t.repo, &c2.to_string(), "a.txt", &c1.to_string(), &[])
        .unwrap();

    assert_eq!(t.status_porcelain(), before);
}

// ── Forwarded git arguments (Spec 021) ──────────────────────────────────

/// The fixup path squashes whatever sits on HEAD, so it has to know that git
/// put a commit there; `--amend` would have replaced the user's own instead.
#[test]
fn committed_onto_sees_what_git_did() {
    let t = TestRepo::new();
    let base = t.commit("Base", "base.txt");
    assert!(!super::committed_onto(&t.workdir(), base));

    let child = t.commit("Child", "child.txt");
    assert!(super::committed_onto(&t.workdir(), base));
    assert!(!super::committed_onto(&t.workdir(), child));
}

/// The fixup path's last resort, reachable only through an argument loom does
/// not know: git amended HEAD instead of committing on top of it, and the
/// squash would have taken the user's own commit into the target.
#[test]
fn an_amend_that_replaced_head_is_taken_back() {
    let t = TestRepo::new();
    t.commit("First", "file1.txt");
    t.commit("Second", "other.txt");
    t.write_file("other.txt", "staged by the user");
    t.stage_files(&["other.txt"]);
    t.write_file("file1.txt", "folded");

    let workdir = t.workdir();
    let saved =
        crate::core::staging::save_and_unstage_other_staged(&t.repo, &workdir, &["file1.txt"])
            .unwrap();
    crate::git::stage_files(&workdir, &["file1.txt"]).unwrap();
    let head = t.head_oid();
    crate::git::run_git(&workdir, &["commit", "--amend", "--no-edit"]).unwrap();
    assert_ne!(t.head_oid(), head, "the amend should have moved HEAD");

    super::undo_commit_attempt(&workdir, head, &["file1.txt"], saved);

    assert_eq!(t.head_oid(), head);
    assert_eq!(t.get_message(0), "Second");
    assert_eq!(t.read_file("file1.txt"), "folded");
    let status = t.status_porcelain();
    assert!(status.contains(" M file1.txt"), "{status}");
    assert!(status.contains("M  other.txt"), "{status}");
}

/// The `-p` amend happens at a rebase pause, where the forwarded arguments have
/// to arrive too.
#[cfg(unix)]
#[test]
fn a_patch_fold_forwards_to_the_amend_at_the_rebase_pause() {
    let body = "1\n2\n3\n4\n5\n6\n7\n8\n";
    let select_moved_hunks = |t: &TestRepo, source: git2::Oid| {
        let mut selections =
            crate::core::staging::collect_commit_hunks(&t.workdir(), &source.to_string(), &[])
                .unwrap();
        for file in &mut selections {
            for hunk in &mut file.hunks {
                hunk.selected = true;
            }
        }
        selections
    };
    let build = || {
        let t = TestRepo::new_with_remote();
        t.commit_multi(&[("f.txt", body)], "Base");
        let target = t.commit_multi(&[("t.txt", "target\n")], "Target");
        let source = t.commit_multi(&[("f.txt", &body.replace("2\n", "TWO\n"))], "Source");
        t.install_hook("pre-commit", "exit 1\n");
        (t, target, source)
    };

    let (t, target, source) = build();
    let selections = select_moved_hunks(&t, source);
    assert!(
        super::fold_selected_hunks_to_commit(
            &t.repo,
            &t.workdir(),
            &source.to_string(),
            &target.to_string(),
            "Target",
            &selections,
            &[],
        )
        .is_err(),
        "the hook should block the amend"
    );

    let (t, target, source) = build();
    let selections = select_moved_hunks(&t, source);
    let (_, new_target) = super::fold_selected_hunks_to_commit(
        &t.repo,
        &t.workdir(),
        &source.to_string(),
        &target.to_string(),
        "Target",
        &selections,
        &["--no-verify"],
    )
    .unwrap();

    assert!(t.commit_has_file(git2::Oid::from_str(&new_target).unwrap(), "f.txt"));
    assert_eq!(t.commit_messages()[..3], ["Source", "Target", "Base"]);
}

/// Moving a file between commits amends at a rebase pause, which is a third
/// place the hooks run and the forwarded arguments have to reach.
#[cfg(unix)]
#[test]
fn a_commit_file_fold_forwards_to_its_amend() {
    let build = || {
        let t = TestRepo::new_with_remote();
        t.write_file("file1.txt", "content1");
        t.write_file("file2.txt", "content2");
        t.stage_files(&["file1.txt", "file2.txt"]);
        t.commit_staged("Source commit");
        let source = t.head_oid();
        t.write_file("file3.txt", "content3");
        t.stage_files(&["file3.txt"]);
        t.commit_staged("Target commit");
        let target = t.head_oid();
        t.install_hook("pre-commit", "exit 1\n");
        (t, source, target)
    };

    let (t, source, target) = build();
    assert!(
        super::fold_commit_file_to_commit(
            &t.repo,
            &source.to_string(),
            "file1.txt",
            &target.to_string(),
            &[],
        )
        .is_err(),
        "the hook should block the amend"
    );

    let (t, source, target) = build();
    super::fold_commit_file_to_commit(
        &t.repo,
        &source.to_string(),
        "file1.txt",
        &target.to_string(),
        &["--no-verify"],
    )
    .unwrap();

    assert_eq!(t.commit_messages()[..2], ["Target commit", "Source commit"]);
    assert!(t.commit_has_file(t.head_oid(), "file1.txt"));
    assert_eq!(t.read_file("file1.txt"), "content1");
}

/// A forwarded argument must not cost the commits above the target: the fixup
/// path squashes, and a squash that starts from the wrong commit eats one.
#[cfg(unix)]
#[test]
fn a_forwarded_fold_into_an_older_commit_keeps_the_commits_above_it() {
    let t = TestRepo::new_with_remote();
    t.commit("Base", "base.txt");
    let first = t.commit("First", "f1.txt");
    t.commit("Second", "f2.txt");
    t.install_hook("pre-commit", "exit 1\n");
    t.write_file("f1.txt", "folded");

    super::fold_files_into_commit(
        &t.repo,
        &["f1.txt".to_string()],
        &first.to_string(),
        false,
        &["--no-verify"],
    )
    .unwrap();

    assert_eq!(t.commit_messages()[..3], ["Second", "First", "Base"]);
    assert_eq!(t.read_file("f1.txt"), "folded");
}

/// `--amend` on the fixup path makes git rewrite the user's own HEAD instead
/// of committing the fixup; the squash would then have eaten that commit.
#[test]
fn a_forwarded_amend_on_the_fixup_path_rolls_back() {
    let t = TestRepo::new_with_remote();
    t.commit("Base", "base.txt");
    let first = t.commit("First", "f1.txt");
    t.commit("Second", "f2.txt");
    let before = stage_a_mix(&t, "base.txt");
    t.write_file("f1.txt", "folded");

    let err = super::fold_files_into_commit(
        &t.repo,
        &["f1.txt".to_string()],
        &first.to_string(),
        false,
        &["--amend"],
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("left no new commit on HEAD"),
        "{err}"
    );
    assert_eq!(t.commit_messages()[..3], ["Second", "First", "Base"]);
    assert_eq!(t.read_file("f1.txt"), "folded");
    let status = t.status_porcelain();
    assert!(status.contains(" M f1.txt"), "{status}");
    assert_eq!(
        status.lines().filter(|l| !l.contains("f1.txt")).count(),
        before.lines().count(),
        "the user's own staged set is back: {status}"
    );
}

/// A dry run prints and exits 0 without committing, so the amend has to catch
/// it before the fold reports success.
#[test]
fn a_forwarded_dry_run_on_the_amend_rolls_back() {
    let t = TestRepo::new_with_remote();
    t.commit("Base", "base.txt");
    let head = t.commit("Only", "f1.txt");
    let before = stage_a_mix(&t, "base.txt");
    t.write_file("f1.txt", "folded");

    let err = super::fold_files_into_commit(
        &t.repo,
        &["f1.txt".to_string()],
        &head.to_string(),
        false,
        &["--dry-run"],
    )
    .unwrap_err();

    assert!(err.to_string().contains("nothing was amended"), "{err}");
    assert_eq!(t.head_oid(), head);
    assert_eq!(t.read_file("f1.txt"), "folded");
    assert_eq!(t.status_porcelain(), format!("{before} M f1.txt\n"));
}

/// The fixup path's `git commit` can fail outright — a refusing `pre-commit`
/// hook — and must hand the index back exactly as it found it.
#[cfg(unix)]
#[test]
fn a_refused_fixup_commit_gives_the_index_back() {
    let t = TestRepo::new_with_remote();
    t.commit("Base", "base.txt");
    let first = t.commit("First", "f1.txt");
    t.commit("Second", "f2.txt");
    let before = stage_a_mix(&t, "base.txt");
    let head = t.head_oid();
    t.write_file("f1.txt", "folded");
    t.install_hook("pre-commit", "exit 1\n");

    assert!(
        super::fold_files_into_commit(
            &t.repo,
            &["f1.txt".to_string()],
            &first.to_string(),
            false,
            &[],
        )
        .is_err()
    );

    assert_eq!(t.head_oid(), head);
    assert_eq!(t.commit_messages()[..3], ["Second", "First", "Base"]);
    assert_eq!(t.status_porcelain(), format!("{before} M f1.txt\n"));
}

/// Every whole-commit form rejects the separator, and each reaches the check
/// from a different arm of `run` (Spec 021).
#[test]
fn every_whole_commit_form_rejects_the_separator() {
    let t = TestRepo::new_with_remote();
    t.commit("First", "f1.txt");
    let first = t.head_oid().to_string();
    t.commit("Second", "f2.txt");
    let second = t.head_oid().to_string();
    t.create_branch_at("other", &first);

    let cases: [(bool, Option<super::Anchor>, Vec<String>, &str); 5] = [
        (
            false,
            Some(super::Anchor::Above(first.clone())),
            vec![second.clone()],
            "moving commits next to another",
        ),
        (
            true,
            None,
            vec![second.clone(), "brand-new".into()],
            "moving commits to a new branch",
        ),
        (
            false,
            None,
            vec![second.clone(), first.clone()],
            "folding a commit into another",
        ),
        (
            false,
            None,
            vec![second.clone(), "other".into()],
            "moving commits to a branch",
        ),
        (
            false,
            None,
            vec![second.clone(), "zz".into()],
            "uncommitting a commit",
        ),
    ];

    for (create, anchor, args, what) in cases {
        let result = t.in_dir(|| {
            super::run(
                create,
                false,
                anchor.clone(),
                HunkArgs::default(),
                args.clone(),
                vec!["--no-verify".into()],
                &crate::core::graph::Theme::dark(),
            )
        });
        let err = result.expect_err(what).to_string();
        assert!(err.contains(what), "{what}: {err}");
        assert!(
            err.contains("takes no arguments after `--`"),
            "{what}: {err}"
        );
    }

    assert_eq!(t.commit_messages()[..2], ["Second", "First"]);
}

/// Moving a file between commits amends both, so a forwarded message source
/// rewords both. Pinned because the docs promise exactly this (Spec 021).
#[test]
fn a_forwarded_message_rewords_both_commits_of_a_move() {
    let t = TestRepo::new_with_remote();
    t.commit("Base", "base.txt");
    let target = t.commit("Target", "t.txt");
    let source = t.commit_multi(&[("moved.txt", "m"), ("stays.txt", "s")], "Source");

    super::fold_commit_file_to_commit(
        &t.repo,
        &source.to_string(),
        "moved.txt",
        &target.to_string(),
        &["-m", "hijacked"],
    )
    .unwrap();

    assert_eq!(t.commit_messages()[..3], ["hijacked", "hijacked", "Base"]);
}

/// `--only` with no pathspec commits none of the index, and `--allow-empty`
/// lets the result through, so git makes a `fixup!` child holding nothing.
/// Squashing that rewrites the target with nothing in it and reports success.
#[test]
fn an_empty_fixup_commit_is_refused_before_the_squash() {
    let t = TestRepo::new_with_remote();
    t.commit("Base", "base.txt");
    let first = t.commit("First", "f1.txt");
    t.commit("Second", "f2.txt");
    let before = stage_a_mix(&t, "base.txt");
    let head = t.head_oid();
    t.write_file("f1.txt", "folded");

    let err = super::fold_files_into_commit(
        &t.repo,
        &["f1.txt".to_string()],
        &first.to_string(),
        false,
        &["--only", "--allow-empty"],
    )
    .unwrap_err();

    assert!(err.to_string().contains("empty `fixup!` commit"), "{err}");
    assert_eq!(t.head_oid(), head, "nothing was rewritten");
    assert_eq!(t.commit_messages()[..3], ["Second", "First", "Base"]);
    assert_eq!(t.status_porcelain(), format!("{before} M f1.txt\n"));
}
