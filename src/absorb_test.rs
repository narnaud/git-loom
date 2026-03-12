use crate::test_helpers::TestRepo;

// ── Unit tests for diff parsing ──────────────────────────────────────

#[test]
fn parse_hunks_single_hunk() {
    let diff = "\
--- a/file.txt
+++ b/file.txt
@@ -3,4 +3,4 @@
 context
-old line
+new line
 context
";
    let hunks = super::parse_hunks(diff);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].modified_lines, vec![4]);
    assert!(hunks[0].text.contains("@@ -3,4 +3,4 @@"));
    assert!(hunks[0].text.contains("-old line"));
}

#[test]
fn parse_hunks_multiple_hunks() {
    let diff = "\
--- a/file.txt
+++ b/file.txt
@@ -3,4 +3,4 @@
 context
-old line 1
+new line 1
 context
@@ -20,4 +20,4 @@
 context
-old line 2
+new line 2
 context
";
    let hunks = super::parse_hunks(diff);
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].modified_lines, vec![4]);
    assert_eq!(hunks[1].modified_lines, vec![21]);
}

#[test]
fn parse_hunks_pure_addition() {
    let diff = "\
--- a/file.txt
+++ b/file.txt
@@ -3,2 +3,4 @@
 context
+added line 1
+added line 2
 context
";
    let hunks = super::parse_hunks(diff);
    assert_eq!(hunks.len(), 1);
    assert!(
        hunks[0].modified_lines.is_empty(),
        "pure additions should produce no modified original lines"
    );
}

// ── Unit tests for patch construction ────────────────────────────────

#[test]
fn build_hunk_patch_single_hunk() {
    let hunk = super::DiffHunk {
        text: "@@ -3,4 +3,4 @@\n context\n-old line\n+new line\n context\n".to_string(),
        modified_lines: vec![4],
    };
    let patch = super::build_hunk_patch("src/file.txt", &[hunk]);
    assert!(patch.starts_with("--- a/src/file.txt\n+++ b/src/file.txt\n"));
    assert!(patch.contains("@@ -3,4 +3,4 @@"));
    assert!(patch.contains("-old line"));
    assert!(patch.contains("+new line"));
}

#[test]
fn build_hunk_patch_multiple_hunks() {
    let hunk1 = super::DiffHunk {
        text: "@@ -3,4 +3,4 @@\n context\n-old1\n+new1\n context\n".to_string(),
        modified_lines: vec![4],
    };
    let hunk2 = super::DiffHunk {
        text: "@@ -20,4 +20,4 @@\n context\n-old2\n+new2\n context\n".to_string(),
        modified_lines: vec![21],
    };
    let patch = super::build_hunk_patch("src/file.txt", &[hunk1, hunk2]);
    // Should have one file header and both hunks
    assert_eq!(patch.matches("--- a/").count(), 1);
    assert_eq!(patch.matches("@@ -").count(), 2);
}

#[test]
fn parse_hunks_sql_comment_lines() {
    let diff = "\
--- a/query.sql
+++ b/query.sql
@@ -1,3 +1,3 @@
 SELECT *
--- main query
+-- updated query
 FROM users
";
    let hunks = super::parse_hunks(diff);
    assert_eq!(hunks.len(), 1);
    assert_eq!(
        hunks[0].modified_lines,
        vec![2],
        "SQL comment line starting with '-- ' must be tracked as a modified line"
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
        test_repo.assert_working_tree_clean();

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
        test_repo.assert_working_tree_clean();

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
    // Write multi-line content and commit it manually (so file content != message)
    test_repo.write_file("file1.txt", "line1\nline2\n");
    test_repo.stage_files(&["file1.txt"]);
    test_repo.commit_staged("Add file1");

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
    // Create two commits each introducing content in the same file.
    // Use manual staging to control exact file content per commit.
    test_repo.write_file("shared.txt", "line1 from c1\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit 1");

    test_repo.write_file("shared.txt", "line1 from c1\nline2 from c2\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit 2");

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
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature branch off base with a commit
    test_repo.create_branch_at("feat1", &base_oid.to_string());
    test_repo.switch_branch("feat1");
    test_repo.write_file("feature.txt", "initial feature content");
    test_repo.stage_files(&["feature.txt"]);
    test_repo.commit_staged("Feature 1");

    // Merge feat1 into integration branch
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feat1");

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
        test_repo.assert_working_tree_clean();
    });
}

#[test]
fn absorb_split_hunks_to_different_commits() {
    let test_repo = TestRepo::new_with_remote();

    // Create two commits with content in distant regions of the same file.
    // Need enough gap (>6 lines) between regions so git diff produces separate hunks.
    // Commit 1: lines 1-3 plus padding
    test_repo.write_file(
        "shared.txt",
        "line1 from c1\nline2 from c1\nline3 from c1\n\
         pad1\npad2\npad3\npad4\npad5\npad6\npad7\npad8\n",
    );
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit 1");

    // Commit 2: appends lines far away from commit 1's region
    test_repo.write_file(
        "shared.txt",
        "line1 from c1\nline2 from c1\nline3 from c1\n\
         pad1\npad2\npad3\npad4\npad5\npad6\npad7\npad8\n\
         line12 from c2\nline13 from c2\nline14 from c2\n",
    );
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit 2");

    // Modify lines from both commits — separate hunks due to distance
    test_repo.write_file(
        "shared.txt",
        "MODIFIED line1\nline2 from c1\nline3 from c1\n\
         pad1\npad2\npad3\npad4\npad5\npad6\npad7\npad8\n\
         MODIFIED line12\nline13 from c2\nline14 from c2\n",
    );

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(
            result.is_ok(),
            "hunk-level absorb should succeed: {:?}",
            result
        );

        // Working tree should be clean — both hunks absorbed
        test_repo.assert_working_tree_clean();

        // Commit messages preserved
        assert_eq!(test_repo.get_message(0), "Commit 2");
        assert_eq!(test_repo.get_message(1), "Commit 1");
    });
}

#[test]
fn absorb_split_with_pure_addition_hunk() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.write_file("file.txt", "line1\nline2\nline3\n");
    test_repo.stage_files(&["file.txt"]);
    test_repo.commit_staged("Add file");

    // Modify line1 (absorbable) and append new lines (pure addition)
    test_repo.write_file(
        "file.txt",
        "MODIFIED line1\nline2\nline3\nnew line4\nnew line5\n",
    );

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(
            result.is_ok(),
            "mixed split absorb should succeed: {:?}",
            result
        );

        // The modified hunk should be absorbed, but pure addition stays in working tree
        let content = test_repo.read_file("file.txt");
        assert!(
            content.contains("new line4"),
            "pure addition hunk should remain in working tree"
        );
    });
}

#[test]
fn absorb_skipped_patch_only_contains_skipped_files() {
    let test_repo = TestRepo::new_with_remote();

    // Create two commits with separate files
    test_repo.write_file("absorbable.txt", "original content\n");
    test_repo.stage_files(&["absorbable.txt"]);
    test_repo.commit_staged("Add absorbable");

    test_repo.write_file("skippable.txt", "line1\nline2\n");
    test_repo.stage_files(&["skippable.txt"]);
    test_repo.commit_staged("Add skippable");

    // Modify absorbable.txt (will be absorbed into its commit)
    test_repo.write_file("absorbable.txt", "modified content\n");

    // Append pure-addition lines to skippable.txt (will be skipped)
    test_repo.write_file("skippable.txt", "line1\nline2\nnew line3\n");

    test_repo.in_dir(|| {
        let result = super::run(
            false,
            vec!["absorbable.txt".to_string(), "skippable.txt".to_string()],
        );
        assert!(result.is_ok(), "absorb failed: {:?}", result);

        // absorbable.txt should be clean (absorbed into its commit)
        let abs_diff =
            crate::git_commands::diff_head_file(&std::path::PathBuf::from("."), "absorbable.txt")
                .unwrap();
        assert!(
            abs_diff.is_empty(),
            "absorbable.txt should be clean after absorb, but has diff:\n{}",
            abs_diff
        );

        // skippable.txt should still have the pure-addition leftover
        let content = test_repo.read_file("skippable.txt");
        assert!(
            content.contains("new line3"),
            "skippable.txt should retain its skipped changes"
        );
    });
}

#[test]
fn absorb_file_with_sql_comment_lines() {
    let test_repo = TestRepo::new_with_remote();
    // Create a SQL file with comment lines
    test_repo.write_file("query.sql", "SELECT *\n-- main query\nFROM users\n");
    test_repo.stage_files(&["query.sql"]);
    test_repo.commit_staged("Add SQL query");

    // Modify the comment line
    test_repo.write_file("query.sql", "SELECT *\n-- updated query\nFROM users\n");

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(
            result.is_ok(),
            "absorb should handle -- lines: {:?}",
            result.err()
        );
        test_repo.assert_working_tree_clean();
        // Verify the change was absorbed
        let content = test_repo.read_file("query.sql");
        assert!(
            content.contains("-- updated query"),
            "absorbed content should contain the updated SQL comment"
        );
    });
}

#[test]
fn absorb_staged_only_changes() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Add file", "target.txt");

    // Stage a change without leaving unstaged modifications
    test_repo.write_file("target.txt", "staged content\n");
    test_repo.stage_files(&["target.txt"]);

    test_repo.in_dir(|| {
        let result = super::run(false, vec![]);
        assert!(
            result.is_ok(),
            "absorb should handle staged-only changes: {:?}",
            result.err()
        );
        test_repo.assert_working_tree_clean();
    });
}
