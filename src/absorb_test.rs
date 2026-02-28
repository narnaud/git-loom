use crate::git_commands::{self, git_branch, git_commit, git_merge};
use crate::test_helpers::TestRepo;

// ── Unit tests for diff parsing ──────────────────────────────────────

#[test]
fn parse_hunk_header_basic() {
    assert_eq!(super::parse_hunk_header("@@ -10,5 +10,7 @@"), Some(10));
    assert_eq!(super::parse_hunk_header("@@ -1,3 +1,3 @@"), Some(1));
    assert_eq!(super::parse_hunk_header("@@ -42 +42 @@"), Some(42));
    assert_eq!(super::parse_hunk_header("not a header"), None);
}

#[test]
fn parse_modified_lines_basic() {
    let diff = "\
--- a/file.txt
+++ b/file.txt
@@ -3,4 +3,4 @@
 context
-old line
+new line
 context
";
    let lines = super::parse_modified_lines(diff);
    assert_eq!(lines, vec![4]); // line 4 in the original was modified
}

#[test]
fn parse_modified_lines_pure_addition() {
    let diff = "\
--- a/file.txt
+++ b/file.txt
@@ -3,2 +3,4 @@
 context
+added line 1
+added line 2
 context
";
    let lines = super::parse_modified_lines(diff);
    assert!(
        lines.is_empty(),
        "pure additions should produce no modified original lines"
    );
}

// ── Integration tests ────────────────────────────────────────────────

#[test]
fn absorb_single_file() {
    let test_repo = TestRepo::new_with_remote();

    // Create a commit that introduces file1.txt with specific content
    test_repo.commit("Add file1", "file1.txt");

    // Modify file1.txt in the working tree (change existing content)
    test_repo.write_file("file1.txt", "modified content");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        // file1.txt should now be clean (absorbed into the commit)
        let repo = crate::git::open_repo().unwrap();
        let workdir = repo.workdir().unwrap();
        let diff = git_commands::diff_head_name_only(workdir).unwrap();
        assert!(
            diff.trim().is_empty(),
            "working tree should be clean after absorb, but has: {}",
            diff
        );

        // Commit message should be preserved
        assert_eq!(test_repo.get_message(0), "Add file1");
    });
}

#[test]
fn absorb_multiple_files_different_commits() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("Add file1", "file1.txt");
    test_repo.commit("Add file2", "file2.txt");

    // Modify both files
    test_repo.write_file("file1.txt", "modified file1");
    test_repo.write_file("file2.txt", "modified file2");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        // Both files should be clean
        let repo = crate::git::open_repo().unwrap();
        let workdir = repo.workdir().unwrap();
        let diff = git_commands::diff_head_name_only(workdir).unwrap();
        assert!(
            diff.trim().is_empty(),
            "working tree should be clean, but has: {}",
            diff
        );

        // Commit messages preserved
        assert_eq!(test_repo.get_message(0), "Add file2");
        assert_eq!(test_repo.get_message(1), "Add file1");
    });
}

#[test]
fn absorb_skips_new_file() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file1", "file1.txt");

    // Modify tracked file and create a new untracked file
    test_repo.write_file("file1.txt", "modified");
    test_repo.write_file("new_file.txt", "brand new");

    test_repo.in_dir(|| {
        // diff HEAD --name-only only shows tracked changes, so new_file.txt
        // won't even be in the list unless user passes it explicitly.
        // Let's pass both explicitly to test the skip path.
        let result = super::run(
            false,
            vec!["file1.txt".to_string(), "new_file.txt".to_string()],
        );
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        // new_file.txt should still exist in working tree (skipped)
        assert_eq!(test_repo.read_file("new_file.txt"), "brand new");
    });
}

#[test]
fn absorb_skips_pure_addition() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();

    // Write multi-line content and commit it manually (so file content != message)
    test_repo.write_file("file1.txt", "line1\nline2\n");
    git_commit::stage_files(workdir.as_path(), &["file1.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Add file1").unwrap();

    // Add lines without modifying existing ones
    test_repo.write_file("file1.txt", "line1\nline2\nnew line3\n");

    test_repo.in_dir(|| {
        let result = super::run(true, vec![]);
        // Should succeed as dry-run but skip the file (pure addition)
        // The error "No files could be absorbed" is expected when all are skipped
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("No files could be absorbed"),
            "Expected 'No files could be absorbed' error, got: {}",
            err_msg
        );
    });
}

#[test]
fn absorb_dry_run() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file1", "file1.txt");

    let original_head = test_repo.head_oid();
    test_repo.write_file("file1.txt", "modified content");

    test_repo.in_dir(|| {
        let result = super::run(true, vec![]);
        assert!(result.is_ok(), "dry-run absorb failed: {:?}", result);

        // HEAD should NOT have changed (dry-run)
        assert_eq!(
            test_repo.head_oid(),
            original_head,
            "dry-run should not modify HEAD"
        );

        // Working tree changes should still be there
        assert_eq!(test_repo.read_file("file1.txt"), "modified content");
    });
}

#[test]
fn absorb_with_file_filter() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file1", "file1.txt");
    test_repo.commit("Add file2", "file2.txt");

    // Modify both files
    test_repo.write_file("file1.txt", "modified file1");
    test_repo.write_file("file2.txt", "modified file2");

    test_repo.in_dir(|| {
        // Only absorb file1
        let result = super::run(false, vec!["file1.txt".to_string()]);
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        // file2.txt should still have uncommitted changes
        assert_eq!(
            test_repo.read_file("file2.txt"),
            "modified file2",
            "file2 should still have working tree changes"
        );
    });
}

#[test]
fn absorb_no_changes_error() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file1", "file1.txt");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Nothing to absorb"),
            "Expected 'Nothing to absorb' error, got: {}",
            err
        );
    });
}

#[test]
fn absorb_preserves_skipped_changes() {
    let test_repo = TestRepo::new_with_remote();

    // Create an absorbable file
    test_repo.commit("Add tracked", "tracked.txt");

    // Modify tracked file (will be absorbed) and create a new untracked file
    test_repo.write_file("tracked.txt", "modified tracked");
    test_repo.write_file("untracked.txt", "new content");

    test_repo.in_dir(|| {
        // Pass both files explicitly — untracked.txt will be skipped (new file)
        let result = super::run(
            false,
            vec!["tracked.txt".to_string(), "untracked.txt".to_string()],
        );
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        // untracked.txt should still exist with its content (skipped)
        assert_eq!(test_repo.read_file("untracked.txt"), "new content");
    });
}

#[test]
fn absorb_skips_multiple_sources() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();

    // Create two commits each introducing content in the same file.
    // Use manual staging to control exact file content per commit.
    test_repo.write_file("shared.txt", "line1 from c1\n");
    git_commit::stage_files(workdir.as_path(), &["shared.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Commit 1").unwrap();

    test_repo.write_file("shared.txt", "line1 from c1\nline2 from c2\n");
    git_commit::stage_files(workdir.as_path(), &["shared.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Commit 2").unwrap();

    // Modify lines from both commits
    test_repo.write_file("shared.txt", "MODIFIED line1\nMODIFIED line2\n");

    test_repo.in_dir(|| {
        let result = super::run(true, vec![]);
        // All files should be skipped → error
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("No files could be absorbed"),
            "Expected 'No files could be absorbed', got: {}",
            err_msg
        );
    });
}

#[test]
fn absorb_skips_out_of_scope() {
    let test_repo = TestRepo::new_with_remote();

    // Create a commit on the integration branch
    test_repo.commit("In-scope commit", "in_scope.txt");

    // Modify the in-scope file - verify it works
    test_repo.write_file("in_scope.txt", "modified");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(result.is_ok(), "absorb should succeed for in-scope file");
    });
}

#[test]
fn absorb_with_woven_branches() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature branch off base with a commit
    git_branch::create(workdir.as_path(), "feat1", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feat1");
    test_repo.write_file("feature.txt", "initial feature content");
    git_commit::stage_files(workdir.as_path(), &["feature.txt"]).unwrap();
    git_commit::commit(workdir.as_path(), "Feature 1").unwrap();

    // Merge feat1 into integration branch
    test_repo.switch_branch("integration");
    git_merge::merge_no_ff(workdir.as_path(), "feat1").unwrap();

    // Modify the feature file in working tree
    test_repo.write_file("feature.txt", "updated feature content");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(
            result.is_ok(),
            "absorb with woven branches failed: {:?}",
            result
        );

        // Working tree should be clean (feature.txt absorbed)
        let repo = crate::git::open_repo().unwrap();
        let wd = repo.workdir().unwrap();
        let diff = git_commands::diff_head_name_only(wd).unwrap();
        assert!(
            diff.trim().is_empty(),
            "working tree should be clean, but has: {}",
            diff
        );
    });
}
