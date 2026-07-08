use crate::core::repo::gather_repo_info;
use crate::core::shortid::IdAllocator;
use crate::core::test_helpers::TestRepo;

use super::{hide_branches, resolve_commit_filter};

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

// ── top_commit tests ────────────────────────────────────────────────────────

#[test]
fn top_commit_returns_tip_loose_commit() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit_empty("C1");
    test_repo.commit_empty("C2");
    let c2 = test_repo.head_oid();

    let top = super::top_commit(&test_repo.repo).unwrap();
    assert_eq!(top, Some(c2));
}

#[test]
fn top_commit_skips_merge_of_hidden_branch() {
    // Reproduces the bug: HEAD is a merge of a hidden `local-` branch. The top
    // of status is the integration commit below the merge, not the merge itself.
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit_empty("Base");
    let base = test_repo.head_oid();

    // A local-only branch that diverges from the base with its own commit.
    test_repo.create_branch_at_commit("local-only", base);
    test_repo.switch_branch("local-only");
    test_repo.commit_empty("L1");

    // Meanwhile the integration line advances with a real commit.
    test_repo.switch_branch("integration");
    test_repo.commit_empty("C1");
    let c1 = test_repo.head_oid();

    // Merge local-only back into integration, leaving a merge commit at the tip.
    test_repo.merge_no_ff("local-only");
    assert!(test_repo.head_commit().parent_count() > 1, "tip is a merge");

    let top = super::top_commit(&test_repo.repo).unwrap();
    assert_eq!(top, Some(c1), "should skip the merge and pick C1");
}

// ── resolve_commit_filter tests ─────────────────────────────────────────────

#[test]
fn filter_by_full_git_hash_shows_only_that_commit() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("C1", "file1.txt");
    test_repo.commit("C2", "file2.txt");

    let info = gather_repo_info(&test_repo.repo, true, 1).unwrap();
    assert_eq!(info.commits.len(), 2);
    assert!(info.commits.iter().all(|c| !c.files.is_empty()));

    let allocator = IdAllocator::new(info.collect_entities());
    let filter = resolve_commit_filter(&test_repo.repo, &[c1_oid.to_string()], &info, &allocator);
    assert!(filter.contains(&c1_oid));
    assert_eq!(filter.len(), 1);
}

#[test]
fn filter_by_loom_short_id_shows_only_that_commit() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("C1", "file1.txt");
    let c2_oid = test_repo.commit("C2", "file2.txt");

    let info = gather_repo_info(&test_repo.repo, true, 1).unwrap();
    let allocator = IdAllocator::new(info.collect_entities());
    let c2_sid = allocator.get_commit(c2_oid).to_string();

    let filter = resolve_commit_filter(&test_repo.repo, &[c2_sid], &info, &allocator);
    assert!(filter.contains(&c2_oid));
    assert_eq!(filter.len(), 1);
}

#[test]
fn filter_unknown_id_silently_skipped() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("C1", "file1.txt");

    let info = gather_repo_info(&test_repo.repo, true, 1).unwrap();
    let allocator = IdAllocator::new(info.collect_entities());
    let filter = resolve_commit_filter(
        &test_repo.repo,
        &["nonexistent_id".to_string()],
        &info,
        &allocator,
    );
    assert!(filter.is_empty());
}
