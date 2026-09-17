use crate::core::test_helpers::TestRepo;
use crate::core::ui;
use crate::core::weave::Weave;

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
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    test_repo.switch_branch("feature-a");
    for i in 1..=num_commits {
        test_repo.commit(&format!("A{}", i), &format!("a{}.txt", i));
    }

    test_repo.switch_branch("integration");

    // Add a commit on integration BEFORE merging to prevent fast-forward
    test_repo.commit("Int", "int.txt");

    // Merge feature-a (creates a real merge commit since integration diverged)
    test_repo.merge_no_ff("feature-a");

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

/// Dropping the only commit of a branch leaves the branch behind, empty, at
/// the base it built on. Removing it too is `loom drop feature-a`.
#[test]
fn drop_last_commit_on_branch_leaves_it_empty() {
    let test_repo = setup_woven_branch(1);
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    let branch_oid = test_repo.get_branch_target("feature-a");

    super::drop_commit(&test_repo.repo, &branch_oid.to_string(), true)
        .expect("dropping the only commit of a branch");

    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a should survive its commit"
    );
    assert_eq!(test_repo.get_branch_target("feature-a"), base_oid);
    assert!(!test_repo.commit_messages().contains(&"A1".to_string()));
    // The merge went with the emptied section
    assert_eq!(test_repo.head_commit().parent_count(), 1);
}

/// A stacked branch owns no section of its own, so it takes the plain commit
/// path — and survives its only commit the same way.
#[test]
fn drop_sole_commit_of_a_stacked_branch_leaves_it_empty() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("inner", &base_oid.to_string());
    test_repo.switch_branch("inner");
    let i1_oid = test_repo.commit("I1", "i1.txt");

    test_repo.create_branch_at("outer", &i1_oid.to_string());
    test_repo.switch_branch("outer");
    test_repo.commit("O1", "o1.txt");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("outer");

    super::drop_commit(&test_repo.repo, &i1_oid.to_string(), true)
        .expect("dropping the sole commit of a stacked branch");

    assert!(test_repo.branch_exists("inner"), "inner should survive");
    assert_eq!(test_repo.get_branch_target("inner"), base_oid);
    let outer = test_repo.get_branch_target("outer");
    let o1 = test_repo.find_commit(outer);
    assert_eq!(o1.summary().unwrap().unwrap(), "O1");
    assert_eq!(o1.parent_id(0).unwrap(), base_oid, "I1 is gone");
}

/// The rebase moves an emptied branch's ref like any other, so a checkout of
/// it elsewhere refuses the drop before any history is rewritten.
#[test]
fn drop_refuses_when_an_emptied_branch_is_checked_out_elsewhere() {
    let test_repo = setup_woven_branch(1);
    let a1_oid = test_repo.get_branch_target("feature-a");
    let old_head = test_repo.head_oid();

    let wt = test_repo.workdir().parent().unwrap().join("wt-feature-a");
    crate::git::run_git(
        &test_repo.workdir(),
        &["worktree", "add", wt.to_str().unwrap(), "feature-a"],
    )
    .unwrap();

    let err = super::drop_commit(&test_repo.repo, &a1_oid.to_string(), true)
        .expect_err("feature-a is checked out elsewhere");
    assert!(err.to_string().contains("feature-a"), "{err}");

    assert_eq!(test_repo.head_oid(), old_head, "history must be untouched");
    assert_eq!(test_repo.get_branch_target("feature-a"), a1_oid);
    assert!(
        !crate::core::transaction::state_path(test_repo.repo.path()).exists(),
        "no rebase ever started, so the state file must not be left behind"
    );
}

#[test]
fn drop_one_of_two_commits_preserves_branch() {
    let test_repo = setup_woven_branch(2);

    let a2_oid = test_repo.get_branch_target("feature-a");

    let result = super::drop_commit(&test_repo.repo, &a2_oid.to_string(), true);
    assert!(result.is_ok(), "drop_commit failed: {:?}", result);

    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a should still exist"
    );
}

#[test]
fn drop_stale_sha_fails_without_touching_history() {
    let test_repo = TestRepo::new_with_remote();
    let b_oid = test_repo.commit("B", "b.txt");
    let stale_oid = test_repo.commit("C", "c.txt");

    // Rewrite history: C's object stays alive in the reflog but is no longer
    // in the upstream..HEAD range
    test_repo.reset_hard(b_oid);
    let new_tip = test_repo.commit("C2", "c2.txt");

    let result = super::drop_commit(&test_repo.repo, &stale_oid.to_string(), true);
    let err = result.expect_err("dropping a stale SHA must fail");
    assert!(
        err.to_string().contains("not in the local commits"),
        "unexpected error: {}",
        err
    );
    assert_eq!(test_repo.head_oid(), new_tip, "history must be unchanged");
}

#[test]
fn drop_upstream_commit_fails_without_touching_history() {
    let test_repo = TestRepo::new_with_remote();
    let local_oid = test_repo.commit("Local", "local.txt");
    let upstream_oid = test_repo.find_remote_branch_target("origin/main");

    let result = super::drop_commit(&test_repo.repo, &upstream_oid.to_string(), true);
    let err = result.expect_err("dropping an upstream commit must fail");
    assert!(
        err.to_string().contains("already in the upstream"),
        "unexpected error: {}",
        err
    );
    assert_eq!(test_repo.head_oid(), local_oid, "history must be unchanged");
}

// ── Drop branch tests ───────────────────────────────────────────────────

#[test]
fn drop_woven_branch_removes_commits_and_ref() {
    let test_repo = setup_woven_branch(2);

    let result = super::drop_branch(&test_repo.repo, "feature-a", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    assert!(
        !test_repo.branch_exists("feature-a"),
        "feature-a should be deleted"
    );

    let messages = test_repo.commit_messages();
    assert!(!messages.contains(&"A1".to_string()), "A1 should be gone");
    assert!(!messages.contains(&"A2".to_string()), "A2 should be gone");
    assert!(
        messages.contains(&"Int".to_string()),
        "Int commit should remain"
    );
}

#[test]
fn drop_branch_at_merge_base_just_deletes_ref() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create a branch at merge-base (no commits, no weaving)
    test_repo.create_branch_at("empty-branch", &base_oid.to_string());

    test_repo.commit("C1", "c1.txt");

    let result = super::drop_branch(&test_repo.repo, "empty-branch", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    assert!(
        !test_repo.branch_exists("empty-branch"),
        "empty-branch should be deleted"
    );

    assert_eq!(test_repo.get_message(0), "C1");
}

#[test]
fn messages_state_how_many_commits_go_with_the_branch() {
    use super::DropScope::Commits;
    assert_eq!(
        super::drop_prompt("feat", &Commits(1)),
        "Drop branch `feat` and its 1 commit?"
    );
    assert_eq!(
        super::drop_prompt("feat", &Commits(3)),
        "Drop branch `feat` and its 3 commits?"
    );
    assert_eq!(
        super::dropped_message("feat", &Commits(3)),
        "Dropped branch `feat` and its 3 commits"
    );
}

#[test]
fn the_empty_branch_message_says_it_was_empty() {
    assert_eq!(
        super::dropped_message("feat", &super::DropScope::Empty),
        "Dropped empty branch `feat`"
    );
}

/// With nothing removed, neither message claims a number.
#[test]
fn messages_claim_no_count_when_nothing_is_removed() {
    use super::DropScope::Commits;
    assert_eq!(
        super::drop_prompt("feat", &Commits(0)),
        "Drop branch `feat`?"
    );
    assert_eq!(
        super::dropped_message("feat", &Commits(0)),
        "Dropped branch `feat`"
    );
}

/// A co-located drop removes no commit, so both messages name the sibling
/// keeping them instead of a count.
#[test]
fn messages_name_the_sibling_that_keeps_the_commits() {
    use super::DropScope::KeptBy;
    assert_eq!(
        super::drop_prompt("feat", &KeptBy("sibling")),
        "Drop branch `feat`, keeping its commits on `sibling`?"
    );
    assert_eq!(
        super::dropped_message("feat", &KeptBy("sibling")),
        "Dropped branch `feat`, its commits stay on `sibling`"
    );
}

#[test]
fn messages_claim_no_keeper_when_no_ref_names_the_section() {
    use super::DropScope::KeptInHistory;
    assert_eq!(
        super::drop_prompt("feat", &KeptInHistory),
        "Drop branch `feat`, keeping its commits in history?"
    );
    assert_eq!(
        super::dropped_message("feat", &KeptInHistory),
        "Dropped branch `feat`, its commits stay in history"
    );
}

#[test]
fn drop_non_woven_branch_removes_commits_and_ref() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");

    // Switch back to integration and fast-forward merge feature-a
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

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

    let messages = test_repo.commit_messages();
    assert!(!messages.contains(&"A1".to_string()), "A1 should be gone");
    assert!(!messages.contains(&"A2".to_string()), "A2 should be gone");
    assert!(
        messages.contains(&"Int".to_string()),
        "Int commit should remain"
    );
}

#[test]
fn drop_file_target_fails() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("C1", "c1.txt");

    // "nonexistent" doesn't resolve to anything
    let result = test_repo.in_dir(|| super::run(vec!["nonexistent".to_string()], true));

    assert!(result.is_err());
}

#[test]
fn drop_woven_branch_with_two_branches_preserves_other() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    test_repo.switch_branch("integration");

    test_repo.create_branch_at("feature-b", &base_oid.to_string());
    test_repo.switch_branch("feature-b");
    test_repo.commit("B1", "b1.txt");
    test_repo.switch_branch("integration");

    // Add integration commit to prevent fast-forward, then weave both
    test_repo.commit("Int", "int.txt");
    test_repo.merge_no_ff("feature-a");
    test_repo.merge_no_ff("feature-b");

    let result = super::drop_branch(&test_repo.repo, "feature-a", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    assert!(!test_repo.branch_exists("feature-a"));
    assert!(test_repo.branch_exists("feature-b"));

    let messages = test_repo.commit_messages();
    assert!(!messages.contains(&"A1".to_string()), "A1 should be gone");
    assert!(!messages.contains(&"A2".to_string()), "A2 should be gone");
    assert!(messages.contains(&"B1".to_string()), "B1 should remain");
}

// ── Co-located branch tests (same tip) ───────────────────────────────────

#[test]
fn drop_colocated_non_woven_preserves_other_branch_and_commits() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    test_repo.merge_no_ff("feature-a");
    test_repo.force_checkout();

    // Create feature-b at the same tip as feature-a (co-located)
    let fa_tip = test_repo.get_branch_target("feature-a");
    test_repo.create_branch_at("feature-b", &fa_tip.to_string());

    // Add a commit after so branches are not at HEAD
    test_repo.commit("Int", "int.txt");

    // Drop feature-a — feature-b shares the same tip
    let result = super::drop_branch(&test_repo.repo, "feature-a", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    assert!(
        !test_repo.branch_exists("feature-a"),
        "feature-a should be deleted"
    );

    assert!(
        test_repo.branch_exists("feature-b"),
        "feature-b should still exist"
    );

    // Commits should still be in history (not dropped)
    let messages = test_repo.commit_messages();
    assert!(
        messages.contains(&"A1".to_string()),
        "A1 should still be in history"
    );
    assert!(
        messages.contains(&"Int".to_string()),
        "Int should still be in history"
    );
}

#[test]
fn drop_colocated_woven_preserves_other_branch_and_commits() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    // Create feature-b at the same tip as feature-a (co-located)
    let fa_tip = test_repo.get_branch_target("feature-a");
    test_repo.create_branch_at("feature-b", &fa_tip.to_string());

    // Add integration commit and weave feature-a (creates merge topology)
    test_repo.commit("Int", "int.txt");
    test_repo.merge_no_ff("feature-a");

    // Drop feature-a — feature-b shares the same tip
    let result = super::drop_branch(&test_repo.repo, "feature-a", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    assert!(
        !test_repo.branch_exists("feature-a"),
        "feature-a should be deleted"
    );

    assert!(
        test_repo.branch_exists("feature-b"),
        "feature-b should still exist"
    );

    // A1 should still be in history (feature-b still needs it)
    let messages = test_repo.commit_messages();
    assert!(
        messages.contains(&"A1".to_string()),
        "A1 should still be in history"
    );
    assert!(
        messages.contains(&"Int".to_string()),
        "Int should still be in history"
    );
}

#[test]
fn drop_stacked_outer_branch_preserves_inner_branch() {
    // Stacked topology: feat2 is stacked on feat1.
    //   origin/main → A1 (feat1) → A2 (feat2)
    //                                         ↘
    //                            Int --------→ merge (HEAD, integration)
    //
    // Dropping feat2 should keep feat1 and its commit A1.
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feat1", &base_oid.to_string());
    test_repo.switch_branch("feat1");
    test_repo.commit("A1", "a1.txt");
    let feat1_tip = test_repo.head_oid();

    // Create feat2 stacked on feat1 with one commit
    test_repo.create_branch_at("feat2", &feat1_tip.to_string());
    test_repo.switch_branch("feat2");
    test_repo.commit("A2", "a2.txt");
    test_repo.switch_branch("integration");

    // Add integration commit and weave feat2 (which includes feat1)
    test_repo.commit("Int", "int.txt");
    test_repo.merge_no_ff("feat2");

    let result = super::drop_branch(&test_repo.repo, "feat2", true);
    assert!(result.is_ok(), "drop_branch failed: {:?}", result);

    assert!(!test_repo.branch_exists("feat2"), "feat2 should be deleted");

    assert!(test_repo.branch_exists("feat1"), "feat1 should still exist");

    // A1 should remain in history, A2 should be gone
    let messages = test_repo.commit_messages();
    assert!(
        messages.contains(&"A1".to_string()),
        "A1 should still be in history"
    );
    assert!(!messages.contains(&"A2".to_string()), "A2 should be gone");
    assert!(
        messages.contains(&"Int".to_string()),
        "Int should still be in history"
    );
}

/// Two branches at the same sole commit inside an outer branch both survive,
/// parked at the base that commit built on.
#[test]
fn drop_sole_commit_shared_by_two_branches_leaves_them_empty() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("inner", &base_oid.to_string());
    test_repo.switch_branch("inner");
    let i1_oid = test_repo.commit("I1", "i1.txt");
    test_repo.create_branch_at("inner-too", &i1_oid.to_string());

    test_repo.create_branch_at("outer", &i1_oid.to_string());
    test_repo.switch_branch("outer");
    test_repo.commit("O1", "o1.txt");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("outer");

    super::drop_commit(&test_repo.repo, &i1_oid.to_string(), true)
        .expect("dropping the sole commit of two inner branches");

    for name in ["inner", "inner-too"] {
        assert!(test_repo.branch_exists(name), "{name} should survive");
        assert_eq!(test_repo.get_branch_target(name), base_oid);
    }
    let repo = &test_repo.repo;
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2, "HEAD should still merge outer");
    let outer = test_repo.get_branch_target("outer");
    assert_eq!(head.parent_id(1).unwrap(), outer);
    let o1 = repo.find_commit(outer).unwrap();
    assert_eq!(o1.summary().unwrap().unwrap(), "O1");
    assert_eq!(o1.parent_id(0).unwrap(), base_oid, "I1 is gone");
}

#[test]
fn drop_stacked_inner_branch_deletes_only_the_ref() {
    // base -> A1 (feat1) -> A2 (feat2), woven in: feat1 is inside feat2's section.
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feat1", &base_oid.to_string());
    test_repo.switch_branch("feat1");
    test_repo.commit("A1", "a1.txt");
    let feat1_tip = test_repo.head_oid();

    test_repo.create_branch_at("feat2", &feat1_tip.to_string());
    test_repo.switch_branch("feat2");
    test_repo.commit("A2", "a2.txt");
    test_repo.switch_branch("integration");

    test_repo.commit("Int", "int.txt");
    test_repo.merge_no_ff("feat2");
    let head_before = test_repo.head_oid();
    let feat2_before = test_repo.get_branch_target("feat2");

    super::drop_branch(&test_repo.repo, "feat1", true).expect("dropping an inner branch");

    assert!(!test_repo.branch_exists("feat1"), "feat1 should be deleted");
    assert!(test_repo.branch_exists("feat2"), "feat2 should still exist");
    assert_eq!(test_repo.head_oid(), head_before, "no history rewritten");
    assert_eq!(test_repo.get_branch_target("feat2"), feat2_before);
    let messages = test_repo.commit_messages();
    assert!(messages.contains(&"A1".to_string()));
    assert!(messages.contains(&"A2".to_string()));
}

/// Deleting the outer ref leaves a section carrying a generated `section-<hash>`
/// label; the drop must not offer that label as the branch keeping the commits.
#[test]
fn drop_inner_branch_of_an_unnamed_section_names_no_keeper() {
    // base -> A1 (feat1) -> A2, merged in, then the outer ref is deleted.
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feat1", &base_oid.to_string());
    test_repo.switch_branch("feat1");
    test_repo.commit("A1", "a1.txt");
    let feat1_tip = test_repo.head_oid();

    test_repo.create_branch_at("feat2", &feat1_tip.to_string());
    test_repo.switch_branch("feat2");
    test_repo.commit("A2", "a2.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feat2");
    test_repo.delete_branch("feat2");
    let head_before = test_repo.head_oid();

    let graph = Weave::from_repo(&test_repo.repo).expect("weave graph");
    assert_eq!(
        graph.inner_branch_keeper("feat1"),
        None,
        "no ref names the section, so it has no keeper to show"
    );

    super::drop_branch(&test_repo.repo, "feat1", true).expect("dropping an inner branch");

    assert!(!test_repo.branch_exists("feat1"), "feat1 should be deleted");
    assert_eq!(test_repo.head_oid(), head_before, "no history rewritten");
    let messages = test_repo.commit_messages();
    assert!(messages.contains(&"A1".to_string()));
    assert!(messages.contains(&"A2".to_string()));
}

/// Guards the inner-branch shortcut against a branch on the integration line
/// that a later section's commits also cover: only the `assigned_branches`
/// ordering in `Weave::from_repo_with_info` keeps it off the shortcut, so a
/// reorder there would silently turn this drop into a bare ref delete.
#[test]
fn non_woven_branch_covered_by_a_woven_section_is_not_inner() {
    // base -> I1 (old) on the line; feat forks at I1 and is woven, so feat's
    // section walks back over I1 too.
    let test_repo = TestRepo::new_with_remote();

    let i1_oid = test_repo.commit("I1", "i1.txt");
    test_repo.create_branch_at("old", &i1_oid.to_string());
    test_repo.create_branch_at("feat", &i1_oid.to_string());
    test_repo.switch_branch("feat");
    test_repo.commit("F1", "f1.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feat");
    let head_before = test_repo.head_oid();

    let graph = Weave::from_repo(&test_repo.repo).expect("weave graph");
    assert!(
        !graph.is_inner_branch("old"),
        "the Pick owns the ref, not feat"
    );
    assert_eq!(graph.inner_branch_keeper("old"), None);

    super::drop_branch(&test_repo.repo, "old", true).expect("dropping a non-woven branch");

    assert!(!test_repo.branch_exists("old"), "old should be deleted");
    assert_ne!(
        test_repo.head_oid(),
        head_before,
        "the non-woven path rebases; the inner shortcut would not"
    );
}

/// Guards the inner-branch shortcut against a stack left on the integration
/// line: no weave section covers it, so it takes the non-woven path and its
/// commits do go, rewriting the branch stacked on top.
#[test]
fn drop_non_woven_stacked_inner_branch_drops_its_commits() {
    // base -> A1 (feat1) -> A2 (feat2), never woven: both tips are on the
    // integration first-parent line.
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    let a1_oid = test_repo.commit("A1", "a1.txt");
    test_repo.create_branch_at("feat1", &a1_oid.to_string());
    let a2_oid = test_repo.commit("A2", "a2.txt");
    test_repo.create_branch_at("feat2", &a2_oid.to_string());

    super::drop_branch(&test_repo.repo, "feat1", true).expect("dropping a non-woven inner branch");

    assert!(!test_repo.branch_exists("feat1"), "feat1 should be deleted");
    assert!(test_repo.branch_exists("feat2"), "feat2 should still exist");
    let messages = test_repo.commit_messages();
    assert!(!messages.contains(&"A1".to_string()), "A1 should be gone");
    assert!(messages.contains(&"A2".to_string()), "A2 should survive");
    let feat2 = test_repo.find_commit(test_repo.get_branch_target("feat2"));
    assert_eq!(
        feat2.parent_id(0).unwrap(),
        base_oid,
        "feat2 replayed on base"
    );
}

/// Aborting a drop that conflicted puts the branch it would have emptied back
/// at its original tip, not at the base it was parked on.
#[test]
fn drop_abort_restores_an_emptied_branch() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // feature-a's only commit creates shared.txt
    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    let a1_oid = test_repo.commit("version-a", "shared.txt");

    test_repo.switch_branch("integration");
    test_repo.commit("Int", "int.txt");
    test_repo.merge_no_ff("feature-a");

    // An integration commit modifies shared.txt, so replaying it without A1
    // conflicts and the drop pauses.
    test_repo.write_file("shared.txt", "version-b");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit B");
    let old_head = test_repo.head_oid();

    super::drop_commit(&test_repo.repo, &a1_oid.to_string(), true)
        .expect("drop_commit should pause on conflict");
    let git_dir = test_repo.repo.path().to_path_buf();
    assert!(
        crate::core::transaction::state_path(&git_dir).exists(),
        "loom state must exist while the drop is paused"
    );

    crate::core::transaction::abort_cmd(&test_repo.workdir(), &git_dir).unwrap();

    assert_eq!(test_repo.head_oid(), old_head, "HEAD must be restored");
    assert!(test_repo.branch_exists("feature-a"));
    assert_eq!(
        test_repo.get_branch_target("feature-a"),
        a1_oid,
        "feature-a must be back at its own commit, not parked at the base"
    );
}

// ── Drop via run() (end-to-end) ─────────────────────────────────────────

#[test]
fn run_drop_commit_by_hash() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Keep", "keep.txt");
    let drop_oid = test_repo.commit("Drop me", "drop.txt");
    test_repo.commit("Keep2", "keep2.txt");

    let result = test_repo.in_dir(|| super::run(vec![drop_oid.to_string()], true));

    assert!(result.is_ok(), "run failed: {:?}", result);
    assert_eq!(test_repo.get_message(0), "Keep2");
    assert_eq!(test_repo.get_message(1), "Keep");
}

#[test]
fn run_drop_branch_by_name() {
    let test_repo = setup_woven_branch(2);

    let result = test_repo.in_dir(|| super::run(vec!["feature-a".to_string()], true));

    assert!(result.is_ok(), "run failed: {:?}", result);
    assert!(!test_repo.branch_exists("feature-a"));
}

// ── Drop file tests ─────────────────────────────────────────────────────

#[test]
fn drop_file_restores_tracked_modifications() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");

    test_repo.write_file("base.txt", "modified content");

    let result = super::drop_files(&test_repo.repo, &["base.txt".to_string()], true);
    assert!(result.is_ok(), "drop_file failed: {:?}", result);

    // File should be restored to its committed state (content == commit message)
    assert_eq!(test_repo.read_file("base.txt"), "Base");
    assert!(
        test_repo.status_porcelain().is_empty(),
        "working tree should be clean"
    );
}

#[test]
fn drop_file_deletes_untracked_file() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");

    test_repo.write_file("untracked.txt", "new content");

    let result = super::drop_files(&test_repo.repo, &["untracked.txt".to_string()], true);
    assert!(result.is_ok(), "drop_file failed: {:?}", result);

    let path = test_repo.workdir().join("untracked.txt");
    assert!(!path.exists(), "untracked.txt should be deleted");
    assert!(
        test_repo.status_porcelain().is_empty(),
        "working tree should be clean"
    );
}

#[test]
fn drop_file_deletes_staged_new_file() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");

    test_repo.write_file("new.txt", "new content");
    test_repo.stage_files(&["new.txt"]);

    let result = super::drop_files(&test_repo.repo, &["new.txt".to_string()], true);
    assert!(result.is_ok(), "drop_file failed: {:?}", result);

    let path = test_repo.workdir().join("new.txt");
    assert!(!path.exists(), "new.txt should be deleted");
    assert!(
        test_repo.status_porcelain().is_empty(),
        "working tree should be clean"
    );
}

#[test]
fn several_files_drop_together_after_one_confirmation() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");
    test_repo.write_file("base.txt", "modified content");
    test_repo.write_file("untracked.txt", "new content");

    let result = test_repo.in_dir(|| {
        super::run(
            vec!["base.txt".to_string(), "untracked.txt".to_string()],
            true,
        )
    });
    assert!(result.is_ok(), "drop failed: {:?}", result);

    assert_eq!(test_repo.read_file("base.txt"), "Base");
    assert!(!test_repo.workdir().join("untracked.txt").exists());
    assert!(test_repo.status_porcelain().is_empty());
}

#[test]
fn only_files_drop_together() {
    let test_repo = setup_woven_branch(1);
    test_repo.write_file("extra.txt", "new content");

    for other in ["feature-a", "zz"] {
        let result =
            test_repo.in_dir(|| super::run(vec!["extra.txt".to_string(), other.to_string()], true));
        let err = result.unwrap_err().to_string();
        assert_eq!(err, "Only files can be dropped together");
    }
    assert!(test_repo.branch_exists("feature-a"));
    assert!(test_repo.workdir().join("extra.txt").exists());
}

/// The directory's clean already removes the file; a delete of its own would
/// then fail on a path that is gone.
#[test]
fn a_file_inside_a_dropped_directory_goes_with_it() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");
    std::fs::create_dir_all(test_repo.workdir().join("newdir")).unwrap();
    test_repo.write_file("newdir/a.txt", "new");
    test_repo.write_file("newdir/b.txt", "new");

    let paths = ["newdir/a.txt".to_string(), "newdir".to_string()];
    let result = super::drop_files(&test_repo.repo, &paths, true);
    assert!(result.is_ok(), "drop failed: {:?}", result);
    assert!(!test_repo.workdir().join("newdir").exists());
    assert!(test_repo.status_porcelain().is_empty());
}

/// Spellings shell completion produces (`newdir/`, `./newdir`, `.`) name the
/// same directory; it must neither swallow itself nor miss its files.
#[test]
fn directory_spellings_drop_the_same_directory() {
    for args in [
        vec!["newdir/"],
        vec!["./newdir", "newdir/a.txt"],
        vec!["newdir/a.txt", "newdir/"],
        vec!["."],
        vec![".", "newdir/a.txt"],
    ] {
        let test_repo = TestRepo::new_with_remote();
        test_repo.commit("Base", "base.txt");
        std::fs::create_dir_all(test_repo.workdir().join("newdir")).unwrap();
        test_repo.write_file("newdir/a.txt", "new");
        test_repo.write_file("base.txt", "dirty");

        let targets: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let result = test_repo.in_dir(|| super::run(targets, true));
        assert!(result.is_ok(), "{args:?}: {:?}", result);
        assert!(
            !test_repo.workdir().join("newdir").exists(),
            "{args:?}: newdir should be gone"
        );
        // The root takes tracked changes too; a subdirectory leaves them.
        let expected = if args.contains(&".") { "Base" } else { "dirty" };
        assert_eq!(test_repo.read_file("base.txt"), expected, "{args:?}");
    }
}

/// A file named twice (here in two spellings) is dropped once.
#[test]
fn a_file_named_twice_is_dropped_once() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");
    test_repo.write_file("untracked.txt", "new content");

    let result = test_repo.in_dir(|| {
        super::run(
            vec!["untracked.txt".to_string(), "./untracked.txt".to_string()],
            true,
        )
    });
    assert!(result.is_ok(), "drop failed: {:?}", result);
    assert!(!test_repo.workdir().join("untracked.txt").exists());
}

#[test]
fn files_prompt_lists_restored_and_deleted_paths() {
    use super::FileOp;
    let plan = |path: &str, op| (path.to_string(), op);
    assert_eq!(
        super::files_prompt(&[plan("a", FileOp::Restore)]),
        "Discard changes to `a`?"
    );
    assert_eq!(
        super::files_prompt(&[plan("d", FileOp::RestoreDir)]),
        "Discard all changes in `d`?"
    );
    assert_eq!(
        super::files_prompt(&[plan("n", FileOp::RmStaged)]),
        "Delete `n`?"
    );
    assert_eq!(
        super::files_prompt(&[plan("a", FileOp::Restore), plan("b", FileOp::RestoreDir)]),
        "Discard all selected changes?\nrestore `a`\nrestore `b`"
    );
    assert_eq!(
        super::files_prompt(&[plan("n", FileOp::Remove), plan("d", FileOp::CleanDir)]),
        "Delete all selected files?\ndelete `n`\ndelete `d`"
    );
    assert_eq!(
        super::files_prompt(&[plan("a", FileOp::Restore), plan("n", FileOp::Remove)]),
        "Discard all selected changes and delete all selected files?\nrestore `a`\ndelete `n`"
    );
}

// ── Drop directory tests ─────────────────────────────────────────────────

#[test]
fn drop_dir_with_only_untracked_files() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");

    // Create a directory with only untracked files
    std::fs::create_dir_all(test_repo.workdir().join("newdir")).unwrap();
    test_repo.write_file("newdir/a.txt", "aaa");
    test_repo.write_file("newdir/b.txt", "bbb");

    let result = super::drop_files(&test_repo.repo, &["newdir".to_string()], true);
    assert!(result.is_ok(), "drop_file (dir) failed: {:?}", result);

    assert!(
        !test_repo.workdir().join("newdir").exists(),
        "newdir should be deleted"
    );
    assert!(
        test_repo.status_porcelain().is_empty(),
        "working tree should be clean"
    );
}

#[test]
fn drop_dir_with_only_tracked_modifications() {
    let test_repo = TestRepo::new_with_remote();

    std::fs::create_dir_all(test_repo.workdir().join("src")).unwrap();
    test_repo.write_file("src/one.txt", "original-one");
    test_repo.write_file("src/two.txt", "original-two");
    test_repo.stage_files(&["src/one.txt", "src/two.txt"]);
    test_repo.commit_staged("Initial src files");

    test_repo.write_file("src/one.txt", "modified-one");
    test_repo.write_file("src/two.txt", "modified-two");

    let result = super::drop_files(&test_repo.repo, &["src".to_string()], true);
    assert!(result.is_ok(), "drop_file (dir) failed: {:?}", result);

    assert_eq!(test_repo.read_file("src/one.txt"), "original-one");
    assert_eq!(test_repo.read_file("src/two.txt"), "original-two");
    assert!(
        test_repo.status_porcelain().is_empty(),
        "working tree should be clean"
    );
}

#[test]
fn drop_dir_with_mixed_tracked_and_untracked() {
    let test_repo = TestRepo::new_with_remote();

    std::fs::create_dir_all(test_repo.workdir().join("mix")).unwrap();
    test_repo.write_file("mix/tracked.txt", "original");
    test_repo.stage_files(&["mix/tracked.txt"]);
    test_repo.commit_staged("Initial mix file");

    // Modify tracked file + add untracked file
    test_repo.write_file("mix/tracked.txt", "modified");
    test_repo.write_file("mix/untracked.txt", "new");

    let result = super::drop_files(&test_repo.repo, &["mix".to_string()], true);
    assert!(result.is_ok(), "drop_file (dir) failed: {:?}", result);

    assert_eq!(test_repo.read_file("mix/tracked.txt"), "original");
    assert!(
        !test_repo.workdir().join("mix/untracked.txt").exists(),
        "untracked file should be deleted"
    );
    assert!(
        test_repo.status_porcelain().is_empty(),
        "working tree should be clean"
    );
}

#[test]
fn drop_dir_with_staged_new_files() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");

    std::fs::create_dir_all(test_repo.workdir().join("staged")).unwrap();
    test_repo.write_file("staged/a.txt", "aaa");
    test_repo.write_file("staged/b.txt", "bbb");
    test_repo.stage_files(&["staged/a.txt", "staged/b.txt"]);

    let result = super::drop_files(&test_repo.repo, &["staged".to_string()], true);
    assert!(result.is_ok(), "drop_file (dir) failed: {:?}", result);

    assert!(
        !test_repo.workdir().join("staged").exists(),
        "staged dir should be deleted"
    );
    assert!(
        test_repo.status_porcelain().is_empty(),
        "working tree should be clean"
    );
}

// ── Drop all (zz) tests ─────────────────────────────────────────────────

#[test]
fn drop_all_discards_tracked_and_untracked_changes() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");

    // Mix of changes: modify tracked file + create untracked file
    test_repo.write_file("base.txt", "modified");
    test_repo.write_file("untracked.txt", "new");

    let result = super::drop_all(&test_repo.repo, true);
    assert!(result.is_ok(), "drop_all failed: {:?}", result);

    assert!(
        test_repo.status_porcelain().is_empty(),
        "working tree should be clean"
    );
    let path = test_repo.workdir().join("untracked.txt");
    assert!(!path.exists(), "untracked.txt should be deleted");
    assert_eq!(test_repo.read_file("base.txt"), "Base");
}

#[test]
fn drop_all_fails_when_no_changes() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Base", "base.txt");

    let result = super::drop_all(&test_repo.repo, true);
    assert!(result.is_err(), "drop_all should fail with no changes");
    assert!(
        result.unwrap_err().to_string().contains("No local changes"),
        "error should mention no changes"
    );
}

// ── Integration tests for centralized resolver ───────────────────────────

#[test]
fn drop_merge_commit_fails() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("a.txt", "a.txt");
    let a_oid = test_repo.head_oid();
    let upstream_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.commit_merge("Merge side", a_oid, upstream_oid);
    let merge_oid = test_repo.head_oid();

    let result = test_repo.in_dir(|| crate::drop::run(vec![merge_oid.to_string()], true));
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("merge commit"),
        "Error should mention merge commit"
    );
}

#[test]
fn drop_prefers_file_over_branch_name_collision() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("collision", a1_oid);
    test_repo.write_file("collision", "dirty data");

    let result = test_repo.in_dir(|| crate::drop::run(vec!["collision".to_string()], true));
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
    // Branch should still exist (file was dropped, not the branch)
    assert!(test_repo.branch_exists("collision"));
}

#[test]
fn drop_file_resolves_from_nested_cwd() {
    let test_repo = TestRepo::new_with_remote();
    let sub_dir = test_repo.workdir().join("sub");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(sub_dir.join("file.txt"), "content").unwrap();
    test_repo.stage_files(&["sub/file.txt"]);
    test_repo.commit_staged("add sub/file.txt");
    std::fs::write(sub_dir.join("file.txt"), "modified").unwrap();

    let result = test_repo.in_dir_path(&sub_dir, || {
        crate::drop::run(vec!["file.txt".to_string()], true)
    });
    assert!(result.is_ok(), "Expected ok, got: {:?}", result);
}

// ── Abort preserves working state ────────────────────────────────────────

/// Regression: loom abort after a drop conflict must preserve staged changes,
/// unstaged changes on other files, and new untracked files.
///
/// Conflict setup: Commit A creates `shared.txt`; Commit B modifies it.
/// Dropping A forces B to be replayed without A — B's diff expects `shared.txt`
/// to exist with A's content, but the file no longer exists → conflict.
#[test]
fn drop_abort_preserves_working_state() {
    let test_repo = TestRepo::new_with_remote();

    // Commit A creates shared.txt; Commit B modifies it.
    let a_oid = test_repo.commit("version-a", "shared.txt");
    test_repo.write_file("shared.txt", "version-b");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit B");

    // Working state set up before running drop.
    test_repo.write_file("shared.txt", "working-edit"); // change to the conflicting file
    test_repo.write_file("other-staged.txt", "staged-content");
    test_repo.stage_files(&["other-staged.txt"]);
    test_repo.write_file("other-unstaged.txt", "unstaged-content");
    test_repo.write_file("new-file.txt", "new-content");

    // Drop A — B cannot be replayed without A's file → conflict → loom pauses.
    let result = super::drop_commit(&test_repo.repo, &a_oid.to_string(), true);
    assert!(
        result.is_ok(),
        "drop_commit should pause on conflict: {:?}",
        result
    );

    let state_path = test_repo.repo.path().join("loom").join("state.json");
    assert!(
        state_path.exists(),
        "loom state must exist when drop is paused on conflict"
    );

    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();
    crate::core::transaction::abort_cmd(&workdir, &git_dir).unwrap();

    // All working state preserved after abort.
    assert_eq!(test_repo.read_file("shared.txt"), "working-edit");
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

// ── Confirmation ─────────────────────────────────────────────────────────

/// Declining must raise the marker a dismissed prompt raises, or the TUI
/// reports a routine "no" as a failed command.
#[test]
fn declining_the_confirm_is_cancelled() {
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        ui::install(tx);
        let result = super::confirm_or_bail(false, "Drop commit `a1`?");
        ui::uninstall();
        result
    });

    let ui::Request::Prompt { reply, .. } = rx.recv().unwrap() else {
        panic!("expected a prompt");
    };
    reply.send(Some(ui::Answer::Bool(false))).unwrap();

    let err = worker.join().unwrap().unwrap_err();
    assert!(err.downcast_ref::<ui::Cancelled>().is_some());
    assert_eq!(err.to_string(), "Cancelled");
}
