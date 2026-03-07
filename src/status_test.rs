use crate::git::gather_repo_info;
use crate::test_helpers::TestRepo;

use super::hide_branches;

#[test]
fn hidden_branch_removed_from_branches() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit_empty("C1");
    let tip = test_repo.head_oid();
    test_repo.create_branch_at_commit("local-secrets", tip);

    let mut info = gather_repo_info(&test_repo.repo, false, 1).unwrap();
    assert_eq!(info.branches.len(), 1);
    assert_eq!(info.branches[0].name, "local-secrets");

    hide_branches(&mut info, "local-");

    assert!(info.branches.is_empty());
}

#[test]
fn hidden_branch_commits_not_shown_as_loose() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit_empty("C1");
    let tip = test_repo.head_oid();
    test_repo.create_branch_at_commit("local-secrets", tip);

    let mut info = gather_repo_info(&test_repo.repo, false, 1).unwrap();
    assert_eq!(info.commits.len(), 1);

    hide_branches(&mut info, "local-");

    // Commit owned by the hidden branch must not appear as a loose commit.
    assert!(info.commits.is_empty());
}

#[test]
fn non_hidden_branch_unaffected() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit_empty("C1");
    let tip = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", tip);

    let mut info = gather_repo_info(&test_repo.repo, false, 1).unwrap();
    hide_branches(&mut info, "local-");

    assert_eq!(info.branches.len(), 1);
    assert_eq!(info.branches[0].name, "feature-a");
    assert_eq!(info.commits.len(), 1);
}

#[test]
fn hidden_branch_stacked_on_visible_branch() {
    // Stack: local-private (on top) → feature-a (below)
    // After hiding local-private, feature-a and its commits must remain.
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit_empty("A1");
    let a1 = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", a1);

    test_repo.commit_empty("P1");
    let p1 = test_repo.head_oid();
    test_repo.create_branch_at_commit("local-private", p1);

    let mut info = gather_repo_info(&test_repo.repo, false, 1).unwrap();
    assert_eq!(info.branches.len(), 2);
    assert_eq!(info.commits.len(), 2);

    hide_branches(&mut info, "local-");

    assert_eq!(info.branches.len(), 1);
    assert_eq!(info.branches[0].name, "feature-a");
    // Only feature-a's commit (A1) should remain; P1 is hidden.
    assert_eq!(info.commits.len(), 1);
    assert_eq!(info.commits[0].message, "A1");
}

#[test]
fn hidden_colocated_with_visible_branch_preserves_commit() {
    // local-backup and feature-a point to the same commit.
    // Hiding local-backup must not remove the shared commit.
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit_empty("A1");
    let tip = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", tip);
    test_repo.create_branch_at_commit("local-backup", tip);

    let mut info = gather_repo_info(&test_repo.repo, false, 1).unwrap();
    assert_eq!(info.branches.len(), 2);

    hide_branches(&mut info, "local-");

    // feature-a must survive with its commit intact.
    assert_eq!(info.branches.len(), 1);
    assert_eq!(info.branches[0].name, "feature-a");
    assert_eq!(info.commits.len(), 1);
    assert_eq!(info.commits[0].message, "A1");
}

#[test]
fn multiple_hidden_branches() {
    // Two local-* branches stacked; both should be removed.
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit_empty("S1");
    let s1 = test_repo.head_oid();
    test_repo.create_branch_at_commit("local-secrets", s1);

    test_repo.commit_empty("S2");
    let s2 = test_repo.head_oid();
    test_repo.create_branch_at_commit("local-config", s2);

    let mut info = gather_repo_info(&test_repo.repo, false, 1).unwrap();
    assert_eq!(info.branches.len(), 2);

    hide_branches(&mut info, "local-");

    assert!(info.branches.is_empty());
    assert!(info.commits.is_empty());
}
