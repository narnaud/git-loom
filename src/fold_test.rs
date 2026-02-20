use crate::git;
use crate::git_commands::{self, git_branch, git_merge};
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
