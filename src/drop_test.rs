use crate::git;
use crate::git_commands::{git_branch, git_merge};
use crate::test_helpers::TestRepo;

// ── Helper: create a woven branch with commits ─────────────────────────

/// Set up a test repo with a woven feature-a branch containing the given
/// number of commits.
///
/// Creates a real merge topology (not fast-forward) by adding a commit on
/// integration before merging feature-a:
///
/// ```text
/// origin/main → Int (integration commit)
///             ↘                  ↘
///              A1 [→ A2] ──────→ merge (HEAD, integration)
///              (feature-a)
/// ```
fn setup_woven_branch(num_commits: usize) -> TestRepo {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at merge-base
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();

    // Switch to feature-a and add commits
    test_repo.switch_branch("feature-a");
    for i in 1..=num_commits {
        test_repo.commit(&format!("A{}", i), &format!("a{}.txt", i));
    }

    // Switch back to integration
    test_repo.switch_branch("integration");

    // Add a commit on integration BEFORE merging to prevent fast-forward
    test_repo.commit("Int", "int.txt");

    // Merge feature-a (creates a real merge commit since integration diverged)
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();

    test_repo
}

// ── Drop commit tests ───────────────────────────────────────────────────

#[test]
fn drop_commit_removes_it_from_history() {
    let test_repo = TestRepo::new_with_remote();
    let _c1_oid = test_repo.commit("Keep", "keep.txt");
    let c2_oid = test_repo.commit("Drop me", "drop.txt");
    test_repo.commit("Keep2", "keep2.txt");

    let result = super::drop_commit(&test_repo.repo, &c2_oid.to_string(), true);
    assert!(result.is_ok(), "drop_commit failed: {:?}", result);

    assert_eq!(test_repo.get_message(0), "Keep2");
    assert_eq!(test_repo.get_message(1), "Keep");
}

#[test]
fn drop_commit_dirty_tree_autostashed() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");
    let c1_oid = test_repo.commit("Commit", "file.txt");
    test_repo.write_file("base.txt", "dirty");

    let result = super::drop_commit(&test_repo.repo, &c1_oid.to_string(), true);
    assert!(
        result.is_ok(),
        "drop should succeed with autostash: {:?}",
        result
    );

    // Dirty changes should be preserved after autostash
    assert_eq!(test_repo.read_file("base.txt"), "dirty");
}

#[test]
fn drop_last_commit_on_branch_auto_deletes_branch() {
    let test_repo = setup_woven_branch(1);
    let branch_oid = test_repo.get_branch_target("feature-a");

    let result = super::drop_commit(&test_repo.repo, &branch_oid.to_string(), true);
    assert!(result.is_ok(), "drop_commit failed: {:?}", result);

    // feature-a should have been auto-deleted
    assert!(
        !test_repo.branch_exists("feature-a"),
        "feature-a should have been auto-deleted"
    );
}

#[test]
fn drop_one_of_two_commits_preserves_branch() {
    let test_repo = setup_woven_branch(2);

    // Get the tip commit of feature-a (A2)
    let a2_oid = test_repo.get_branch_target("feature-a");

    let result = super::drop_commit(&test_repo.repo, &a2_oid.to_string(), true);
    assert!(result.is_ok(), "drop_commit failed: {:?}", result);

    // Branch should still exist (A1 remains)
    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a should still exist"
    );
}

// ── Drop branch tests ───────────────────────────────────────────────────

#[test]
fn drop_woven_branch_removes_commits_and_ref() {
    let test_repo = setup_woven_branch(2);

    let result = super::drop_branch(&test_repo.repo, "feature-a", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    // Branch ref is deleted
    assert!(
        !test_repo.branch_exists("feature-a"),
        "feature-a should be deleted"
    );

    // A1, A2 are gone from history, Int should remain
    let info = git::gather_repo_info(&test_repo.repo, false).unwrap();
    let messages: Vec<&str> = info.commits.iter().map(|c| c.message.as_str()).collect();
    assert!(!messages.contains(&"A1"), "A1 should be gone");
    assert!(!messages.contains(&"A2"), "A2 should be gone");
    assert!(messages.contains(&"Int"), "Int commit should remain");
}

#[test]
fn drop_branch_at_merge_base_just_deletes_ref() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create a branch at merge-base (no commits, no weaving)
    git_branch::create(
        test_repo.workdir().as_path(),
        "empty-branch",
        &base_oid.to_string(),
    )
    .unwrap();

    // Add a commit on integration so there's something in the range
    test_repo.commit("C1", "c1.txt");

    let result = super::drop_branch(&test_repo.repo, "empty-branch", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    assert!(
        !test_repo.branch_exists("empty-branch"),
        "empty-branch should be deleted"
    );

    // C1 should still be there
    assert_eq!(test_repo.get_message(0), "C1");
}

#[test]
fn drop_non_woven_branch_removes_commits_and_ref() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at merge-base and add commits
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");

    // Switch back to integration and fast-forward merge feature-a
    test_repo.switch_branch("integration");
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();

    // Force checkout to sync working tree after merge
    test_repo.force_checkout();

    // Add a commit on integration after the merge so feature-a tip != HEAD
    test_repo.commit("Int", "int.txt");

    let result = super::drop_branch(&test_repo.repo, "feature-a", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    assert!(
        !test_repo.branch_exists("feature-a"),
        "feature-a should be deleted"
    );

    // A1, A2 should be gone, Int should remain
    let info = git::gather_repo_info(&test_repo.repo, false).unwrap();
    let messages: Vec<&str> = info.commits.iter().map(|c| c.message.as_str()).collect();
    assert!(!messages.contains(&"A1"), "A1 should be gone");
    assert!(!messages.contains(&"A2"), "A2 should be gone");
    assert!(messages.contains(&"Int"), "Int commit should remain");
}

#[test]
fn drop_file_target_fails() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("C1", "c1.txt");

    // "nonexistent" doesn't resolve to anything
    let result = test_repo.in_dir(|| super::run("nonexistent".to_string(), true));

    assert!(result.is_err());
}

#[test]
fn drop_woven_branch_with_two_branches_preserves_other() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a with commits
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    test_repo.switch_branch("integration");

    // Create feature-b with commits
    git_branch::create(workdir.as_path(), "feature-b", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-b");
    test_repo.commit("B1", "b1.txt");
    test_repo.switch_branch("integration");

    // Add integration commit to prevent fast-forward, then weave both
    test_repo.commit("Int", "int.txt");
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();
    git_merge::merge_no_ff(workdir.as_path(), "feature-b").unwrap();

    // Drop feature-a
    let result = super::drop_branch(&test_repo.repo, "feature-a", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    // feature-a gone, feature-b still exists
    assert!(!test_repo.branch_exists("feature-a"));
    assert!(test_repo.branch_exists("feature-b"));

    // B1 should remain, A1 and A2 should be gone
    let info = git::gather_repo_info(&test_repo.repo, false).unwrap();
    let messages: Vec<&str> = info.commits.iter().map(|c| c.message.as_str()).collect();
    assert!(!messages.contains(&"A1"), "A1 should be gone");
    assert!(!messages.contains(&"A2"), "A2 should be gone");
    assert!(messages.contains(&"B1"), "B1 should remain");
}

// ── Co-located branch tests (same tip) ───────────────────────────────────

#[test]
fn drop_colocated_non_woven_preserves_other_branch_and_commits() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a with a commit
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    // Fast-forward integration to feature-a
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();
    test_repo.force_checkout();

    // Create feature-b at the same tip as feature-a (co-located)
    let fa_tip = test_repo.get_branch_target("feature-a");
    git_branch::create(workdir.as_path(), "feature-b", &fa_tip.to_string()).unwrap();

    // Add a commit after so branches are not at HEAD
    test_repo.commit("Int", "int.txt");

    // Drop feature-a — feature-b shares the same tip
    let result = super::drop_branch(&test_repo.repo, "feature-a", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    // feature-a should be deleted
    assert!(
        !test_repo.branch_exists("feature-a"),
        "feature-a should be deleted"
    );

    // feature-b should still exist and point to the same commit content
    assert!(
        test_repo.branch_exists("feature-b"),
        "feature-b should still exist"
    );

    // Commits should still be in history (not dropped)
    let info = git::gather_repo_info(&test_repo.repo, false).unwrap();
    let messages: Vec<&str> = info.commits.iter().map(|c| c.message.as_str()).collect();
    assert!(messages.contains(&"A1"), "A1 should still be in history");
    assert!(messages.contains(&"Int"), "Int should still be in history");
}

#[test]
fn drop_colocated_woven_preserves_other_branch_and_commits() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a with a commit
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    // Create feature-b at the same tip as feature-a (co-located)
    let fa_tip = test_repo.get_branch_target("feature-a");
    git_branch::create(workdir.as_path(), "feature-b", &fa_tip.to_string()).unwrap();

    // Add integration commit and weave feature-a (creates merge topology)
    test_repo.commit("Int", "int.txt");
    git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();

    // Drop feature-a — feature-b shares the same tip
    let result = super::drop_branch(&test_repo.repo, "feature-a", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    // feature-a should be deleted
    assert!(
        !test_repo.branch_exists("feature-a"),
        "feature-a should be deleted"
    );

    // feature-b should still exist
    assert!(
        test_repo.branch_exists("feature-b"),
        "feature-b should still exist"
    );

    // A1 should still be in history (feature-b still needs it)
    let info = git::gather_repo_info(&test_repo.repo, false).unwrap();
    let messages: Vec<&str> = info.commits.iter().map(|c| c.message.as_str()).collect();
    assert!(messages.contains(&"A1"), "A1 should still be in history");
    assert!(messages.contains(&"Int"), "Int should still be in history");
}

// ── Stacked branch tests ─────────────────────────────────────────────────

#[test]
fn drop_stacked_outer_branch_preserves_inner_branch() {
    // Stacked topology: feat2 is stacked on feat1.
    //   origin/main → A1 (feat1) → A2 (feat2)
    //                                         ↘
    //                            Int --------→ merge (HEAD, integration)
    //
    // Dropping feat2 should keep feat1 and its commit A1.
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feat1 at merge-base with one commit
    git_branch::create(workdir.as_path(), "feat1", &base_oid.to_string()).unwrap();
    test_repo.switch_branch("feat1");
    test_repo.commit("A1", "a1.txt");
    let feat1_tip = test_repo.head_oid();

    // Create feat2 stacked on feat1 with one commit
    git_branch::create(workdir.as_path(), "feat2", &feat1_tip.to_string()).unwrap();
    test_repo.switch_branch("feat2");
    test_repo.commit("A2", "a2.txt");
    test_repo.switch_branch("integration");

    // Add integration commit and weave feat2 (which includes feat1)
    test_repo.commit("Int", "int.txt");
    git_merge::merge_no_ff(workdir.as_path(), "feat2").unwrap();

    // Drop feat2
    let result = super::drop_branch(&test_repo.repo, "feat2", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    // feat2 should be deleted
    assert!(!test_repo.branch_exists("feat2"), "feat2 should be deleted");

    // feat1 should still exist
    assert!(test_repo.branch_exists("feat1"), "feat1 should still exist");

    // A1 should remain in history, A2 should be gone
    let info = git::gather_repo_info(&test_repo.repo, false).unwrap();
    let messages: Vec<&str> = info.commits.iter().map(|c| c.message.as_str()).collect();
    assert!(messages.contains(&"A1"), "A1 should still be in history");
    assert!(!messages.contains(&"A2"), "A2 should be gone");
    assert!(messages.contains(&"Int"), "Int should still be in history");
}

// ── Drop via run() (end-to-end) ─────────────────────────────────────────

#[test]
fn run_drop_commit_by_hash() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Keep", "keep.txt");
    let drop_oid = test_repo.commit("Drop me", "drop.txt");
    test_repo.commit("Keep2", "keep2.txt");

    let result = test_repo.in_dir(|| super::run(drop_oid.to_string(), true));

    assert!(result.is_ok(), "run failed: {:?}", result);
    assert_eq!(test_repo.get_message(0), "Keep2");
    assert_eq!(test_repo.get_message(1), "Keep");
}

#[test]
fn run_drop_branch_by_name() {
    let test_repo = setup_woven_branch(2);

    let result = test_repo.in_dir(|| super::run("feature-a".to_string(), true));

    assert!(result.is_ok(), "run failed: {:?}", result);
    assert!(!test_repo.branch_exists("feature-a"));
}
