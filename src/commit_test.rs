use crate::git_commands::{git_branch, git_merge};
use crate::test_helpers::TestRepo;

/// Helper: set up a test repo with an empty feature branch at the merge-base.
///
/// Creates:
///   origin/main → (merge-base) ← feature-a (empty, at merge-base)
///                              ← integration (HEAD)
fn setup_with_woven_branch() -> TestRepo {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at merge-base (no merge — it's empty)
    git_branch::create(
        test_repo.workdir().as_path(),
        "feature-a",
        &base_oid.to_string(),
    )
    .unwrap();

    test_repo
}

/// Helper: set up a test repo with two woven feature branches, each with a commit.
///
/// Creates:
///   origin/main → A1 (feature-a)
///              ↘              ↘
///               B1 --------→ merge₁ → merge₂ (HEAD on integration)
///               (feature-b)       ↗
fn setup_with_two_branches() -> TestRepo {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at merge-base with one commit
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    // Create feature-b at merge-base with one commit
    git_branch::create(workdir.as_path(), "feature-b", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-b");
    test_repo.commit("B1", "b1.txt");
    test_repo.switch_branch("integration");

    // Weave both branches into integration
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();
    git_merge::merge_no_ff(workdir.as_path(), "feature-b").unwrap();

    test_repo
}

// ── Staging resolution ───────────────────────────────────────────────────

#[test]
fn commit_stages_specific_file() {
    let test_repo = setup_with_woven_branch();

    test_repo.write_file("new.txt", "content");
    test_repo.write_file("other.txt", "other");

    let result = test_repo.in_dir(|| {
        super::run(
            Some("feature-a".to_string()),
            Some("Add new file".to_string()),
            vec!["new.txt".to_string()],
        )
    });

    assert!(result.is_ok(), "commit failed: {:?}", result);

    // feature-a should have the commit
    let branch_oid = test_repo.get_branch_target("feature-a");
    let commit = test_repo.find_commit(branch_oid);
    assert_eq!(commit.summary().unwrap(), "Add new file");
}

#[test]
fn commit_stages_zz_all_changes() {
    let test_repo = setup_with_woven_branch();

    test_repo.write_file("file1.txt", "content1");
    test_repo.write_file("file2.txt", "content2");

    let result = test_repo.in_dir(|| {
        super::run(
            Some("feature-a".to_string()),
            Some("Add files".to_string()),
            vec!["zz".to_string()],
        )
    });

    assert!(result.is_ok(), "commit failed: {:?}", result);

    // feature-a should have the commit
    let branch_oid = test_repo.get_branch_target("feature-a");
    let commit = test_repo.find_commit(branch_oid);
    assert_eq!(commit.summary().unwrap(), "Add files");
}

#[test]
fn commit_uses_already_staged() {
    let test_repo = setup_with_woven_branch();

    // Stage a file manually
    test_repo.write_file("staged.txt", "content");
    crate::git_commands::git_commit::stage_files(test_repo.workdir().as_path(), &["staged.txt"])
        .unwrap();

    let result = test_repo.in_dir(|| {
        super::run(
            Some("feature-a".to_string()),
            Some("Use staged".to_string()),
            vec![], // no file args = use index as-is
        )
    });

    assert!(result.is_ok(), "commit failed: {:?}", result);

    let branch_oid = test_repo.get_branch_target("feature-a");
    let commit = test_repo.find_commit(branch_oid);
    assert_eq!(commit.summary().unwrap(), "Use staged");
}

#[test]
fn commit_empty_index_fails() {
    let test_repo = setup_with_woven_branch();

    // No files staged, no file args
    let result = test_repo.in_dir(|| {
        super::run(
            Some("feature-a".to_string()),
            Some("Message".to_string()),
            vec![],
        )
    });

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Nothing to commit")
    );
}

// ── Branch resolution ────────────────────────────────────────────────────

#[test]
fn commit_to_non_woven_branch_fails() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");

    // Create a branch that tracks the same upstream as integration.
    // find_branches_in_range excludes such branches, so it won't be "woven".
    test_repo.create_branch_tracking("not-woven", "origin/main");

    test_repo.write_file("file.txt", "content");

    let result = test_repo.in_dir(|| {
        super::run(
            Some("not-woven".to_string()),
            Some("Message".to_string()),
            vec!["file.txt".to_string()],
        )
    });

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not woven"));
}

#[test]
fn commit_to_new_branch_creates_and_weaves() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.write_file("new.txt", "content");

    let result = test_repo.in_dir(|| {
        super::run(
            Some("feature-new".to_string()),
            Some("Add file".to_string()),
            vec!["new.txt".to_string()],
        )
    });

    assert!(result.is_ok(), "commit failed: {:?}", result);
    assert!(test_repo.branch_exists("feature-new"));

    // Branch should have the commit
    let branch_oid = test_repo.get_branch_target("feature-new");
    let commit = test_repo.find_commit(branch_oid);
    assert_eq!(commit.summary().unwrap(), "Add file");

    // Integration HEAD should be a merge commit (proper weave topology)
    let head = test_repo.head_commit();
    assert_eq!(
        head.parent_count(),
        2,
        "HEAD should be a merge commit with 2 parents"
    );

    // The branch commit should be reachable as the second parent of the merge
    let second_parent = head.parent(1).unwrap();
    assert_eq!(second_parent.summary().unwrap(), "Add file");
}

// ── Merge topology ──────────────────────────────────────────────────────

#[test]
fn commit_to_empty_branch_creates_merge_topology() {
    let test_repo = setup_with_woven_branch();

    test_repo.write_file("new.txt", "content");

    let result = test_repo.in_dir(|| {
        super::run(
            Some("feature-a".to_string()),
            Some("New commit".to_string()),
            vec!["new.txt".to_string()],
        )
    });

    assert!(result.is_ok(), "commit failed: {:?}", result);

    // feature-a should have the commit
    let branch_oid = test_repo.get_branch_target("feature-a");
    let commit = test_repo.find_commit(branch_oid);
    assert_eq!(commit.summary().unwrap(), "New commit");

    // Integration HEAD should be a merge commit (not the same as feature-a)
    let head = test_repo.head_commit();
    assert_eq!(
        head.parent_count(),
        2,
        "HEAD should be a merge commit (branch woven into integration)"
    );

    // feature-a's commit should be the second parent of the merge
    let second_parent = head.parent(1).unwrap();
    assert_eq!(second_parent.id(), branch_oid);
}

// ── Move via rebase ──────────────────────────────────────────────────────

#[test]
fn commit_moves_to_correct_branch_in_topology() {
    let test_repo = setup_with_two_branches();

    test_repo.write_file("new.txt", "content");

    let result = test_repo.in_dir(|| {
        super::run(
            Some("feature-a".to_string()),
            Some("New on A".to_string()),
            vec!["new.txt".to_string()],
        )
    });

    assert!(result.is_ok(), "commit failed: {:?}", result);

    // feature-a tip should be "New on A"
    let branch_oid = test_repo.get_branch_target("feature-a");
    let commit = test_repo.find_commit(branch_oid);
    assert_eq!(commit.summary().unwrap(), "New on A");

    // Its parent should be A1
    let parent = commit.parent(0).unwrap();
    assert_eq!(parent.summary().unwrap(), "A1");

    // feature-b should still be intact
    let branch_b_oid = test_repo.get_branch_target("feature-b");
    let commit_b = test_repo.find_commit(branch_b_oid);
    assert_eq!(commit_b.summary().unwrap(), "B1");
}

// ── Prerequisites ────────────────────────────────────────────────────────

#[test]
fn commit_not_on_integration_branch_fails() {
    // No upstream tracking = not an integration branch
    let test_repo = TestRepo::new();
    test_repo.commit("A1", "a1.txt");
    test_repo.write_file("new.txt", "content");

    let result = test_repo.in_dir(|| {
        super::run(
            Some("feature-a".to_string()),
            Some("Message".to_string()),
            vec!["new.txt".to_string()],
        )
    });

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("integration branch")
    );
}

// ── File resolution ──────────────────────────────────────────────────────

#[test]
fn commit_nonexistent_file_fails() {
    let test_repo = setup_with_woven_branch();

    let result = test_repo.in_dir(|| {
        super::run(
            Some("feature-a".to_string()),
            Some("Message".to_string()),
            vec!["nonexistent.txt".to_string()],
        )
    });

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}
