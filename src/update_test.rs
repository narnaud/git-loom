use crate::test_helpers::TestRepo;
use git2::{BranchType, Repository, Signature};

#[test]
fn update_pulls_upstream_changes() {
    let test_repo = TestRepo::new_with_remote();

    // Add commits to the remote
    let remote_oid = test_repo.add_remote_commits(&["Remote commit 1"]);

    // Before update, integration should be behind
    let before_oid = test_repo.head_oid();
    assert_ne!(before_oid, remote_oid);

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // After update, integration should point at the remote commit
    let after_oid = test_repo.head_oid();
    assert_eq!(
        after_oid, remote_oid,
        "HEAD should point at the remote commit after pull"
    );
}

#[test]
fn update_works_when_already_up_to_date() {
    let test_repo = TestRepo::new_with_remote();

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());
}

#[test]
fn update_fails_on_detached_head() {
    let test_repo = TestRepo::new_with_remote();
    let oid = test_repo.head_oid();
    test_repo.set_detached_head(oid);

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("detached"),
        "Expected detached HEAD error, got: {}",
        err
    );
}

#[test]
fn update_fails_without_upstream() {
    let test_repo = TestRepo::new();
    // new() creates a repo without remote/upstream

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("upstream") || err.contains("init"),
        "Expected upstream error, got: {}",
        err
    );
}

#[test]
fn update_rebases_local_commits_on_top_of_upstream() {
    let test_repo = TestRepo::new_with_remote();

    // Add a local commit on the integration branch
    test_repo.commit("Local work", "local.txt");

    // Add commits to the remote
    test_repo.add_remote_commits(&["Remote commit"]);

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // Local commit should still be on top
    assert_eq!(test_repo.get_message(0), "Local work");
}

#[test]
fn update_fetches_tags_from_remote() {
    let test_repo = TestRepo::new_with_remote();

    // Create a tag on the remote
    let remote_path = test_repo.remote_path().unwrap();
    let remote_repo = Repository::open_bare(&remote_path).unwrap();
    let remote_head = remote_repo
        .find_branch("main", BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    let remote_commit = remote_repo.find_commit(remote_head).unwrap();
    let sig = Signature::now("Test", "test@test.com").unwrap();
    remote_repo
        .tag(
            "v1.0.0",
            remote_commit.as_object(),
            &sig,
            "Release 1.0",
            false,
        )
        .unwrap();

    // Tag should not exist locally yet
    assert!(
        test_repo.repo.find_reference("refs/tags/v1.0.0").is_err(),
        "Tag should not exist locally before update"
    );

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // Tag should now exist locally
    assert!(
        test_repo.repo.find_reference("refs/tags/v1.0.0").is_ok(),
        "Tag should exist locally after update"
    );
}

#[test]
fn update_prunes_deleted_remote_branches() {
    let test_repo = TestRepo::new_with_remote();

    // Create a branch on the remote
    let remote_path = test_repo.remote_path().unwrap();
    {
        let remote_repo = Repository::open_bare(&remote_path).unwrap();
        let remote_head = remote_repo
            .find_branch("main", BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap();
        let commit = remote_repo.find_commit(remote_head).unwrap();
        remote_repo.branch("feature-temp", &commit, false).unwrap();
    }

    // Fetch so that origin/feature-temp appears locally
    test_repo.fetch_remote();
    // Also fetch the new branch specifically
    test_repo
        .repo
        .find_remote("origin")
        .unwrap()
        .fetch(&["feature-temp"], None, None)
        .unwrap();

    assert!(
        test_repo
            .repo
            .find_branch("origin/feature-temp", BranchType::Remote)
            .is_ok(),
        "Remote-tracking branch should exist after fetch"
    );

    // Delete the branch on the remote
    {
        let remote_repo = Repository::open_bare(&remote_path).unwrap();
        let mut branch = remote_repo
            .find_branch("feature-temp", BranchType::Local)
            .unwrap();
        branch.delete().unwrap();
    }

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // Remote-tracking branch should be pruned
    assert!(
        test_repo
            .repo
            .find_branch("origin/feature-temp", BranchType::Remote)
            .is_err(),
        "Remote-tracking branch should be pruned after update"
    );
}

#[test]
fn update_preserves_merge_topology() {
    let test_repo = TestRepo::new_with_remote();

    // Build a woven topology:
    //   merge-base (Initial) <- feature commit <- merge commit (HEAD)
    //                        ^--- on feature-a ---^

    // Create a feature branch at the current HEAD (merge-base)
    let merge_base_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", merge_base_oid);

    // Add a commit on the feature branch
    test_repo.switch_branch("feature-a");
    test_repo.commit("Feature A work", "feature-a.txt");
    let feature_tip = test_repo.head_oid();

    // Switch back to integration and create a merge commit
    test_repo.switch_branch("integration");
    test_repo.commit_merge("Merge branch 'feature-a'", merge_base_oid, feature_tip);

    // Verify we have a merge commit (2 parents)
    let head = test_repo.head_commit();
    assert_eq!(head.parent_count(), 2, "HEAD should be a merge commit");

    // Add upstream commits
    test_repo.add_remote_commits(&["Upstream change"]);

    // Run update
    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // After update, HEAD should still be a merge commit (topology preserved)
    // Need to re-open since the rebase changed things
    let log = test_repo.in_dir(|| {
        let workdir = test_repo.workdir();
        let output = std::process::Command::new("git")
            .current_dir(&workdir)
            .args(["log", "--oneline", "--graph", "--all", "-10"])
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap()
    });

    // The HEAD commit should still be a merge
    let repo = &test_repo.repo;
    // Force re-read of HEAD after rebase
    let new_head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(
        new_head.parent_count(),
        2,
        "HEAD should still be a merge commit after update, preserving woven topology.\nGraph:\n{}",
        log
    );

    // The merge commit's message should be preserved
    assert!(
        new_head.message().unwrap().contains("Merge branch"),
        "Merge commit message should be preserved"
    );
}

#[test]
fn update_removes_branches_with_gone_upstream() {
    let test_repo = TestRepo::new_with_remote();
    let remote_path = test_repo.remote_path().unwrap();

    // Create feature-x on the remote and fetch it so origin/feature-x exists locally
    {
        let remote_repo = Repository::open_bare(&remote_path).unwrap();
        let remote_head = remote_repo
            .find_branch("main", BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap();
        let commit = remote_repo.find_commit(remote_head).unwrap();
        remote_repo.branch("feature-x", &commit, false).unwrap();
    }
    test_repo
        .repo
        .find_remote("origin")
        .unwrap()
        .fetch(&["feature-x"], None, None)
        .unwrap();

    // Create a local branch tracking origin/feature-x
    test_repo.create_branch_tracking("feature-x", "origin/feature-x");

    assert!(
        test_repo.branch_exists("feature-x"),
        "feature-x should exist before update"
    );

    // Delete feature-x from the remote (simulates upstream deletion)
    {
        let remote_repo = Repository::open_bare(&remote_path).unwrap();
        let mut branch = remote_repo
            .find_branch("feature-x", BranchType::Local)
            .unwrap();
        branch.delete().unwrap();
    }

    // Run update with --yes to skip the interactive prompt
    let result = test_repo.in_dir(|| super::run(true));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // The local branch with gone upstream should be removed
    assert!(
        !test_repo.branch_exists("feature-x"),
        "feature-x should be removed after update (upstream is gone)"
    );
}

#[test]
fn update_keeps_branches_without_tracking_config() {
    let test_repo = TestRepo::new_with_remote();

    // Create a local branch with no upstream tracking configured
    test_repo.create_branch("local-only");

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // Branch without upstream config should not be touched
    assert!(
        test_repo.branch_exists("local-only"),
        "local-only branch should be preserved (no upstream configured)"
    );
}

/// Regression test: upstream commits must NOT end up inside a woven feature
/// branch after update.  Before the fix, plain `git rebase --rebase-merges`
/// preserved the merge topology literally, placing new upstream commits on
/// the branch side of the merge commit.
#[test]
fn update_does_not_put_upstream_commits_in_feature_branch() {
    let test_repo = TestRepo::new_with_remote();

    // Build a woven topology:
    //   *   (HEAD, integration) Merge branch 'feature-a'
    //   |\
    //   | * (feature-a) Feature A work
    //   |/
    //   * (origin/main, main) Initial commit

    let merge_base_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", merge_base_oid);
    test_repo.switch_branch("feature-a");
    test_repo.commit("Feature A work", "feature-a.txt");
    let feature_tip = test_repo.head_oid();
    test_repo.switch_branch("integration");
    test_repo.commit_merge("Merge branch 'feature-a'", merge_base_oid, feature_tip);

    // Push 3 new commits to the remote (simulating teammates' work)
    test_repo.add_remote_commits(&["Remote 1", "Remote 2", "Remote 3"]);

    // Run update
    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // Verify: HEAD is still a merge commit
    let repo = &test_repo.repo;
    let new_head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(
        new_head.parent_count(),
        2,
        "HEAD should still be a merge commit"
    );

    // Verify: the second parent of the merge (the feature branch side)
    // should have exactly 1 commit before reaching the new base.
    // If upstream commits leaked into the branch, there would be more.
    let merge_second_parent = new_head.parent(1).unwrap();
    let new_base = new_head.parent(0).unwrap(); // first parent = base line

    // Walk the second parent line back, counting non-merge commits until
    // we reach the base.
    let mut count = 0;
    let mut current = merge_second_parent.id();
    let base_oid = new_base.id();
    loop {
        if current == base_oid {
            break;
        }
        count += 1;
        let commit = repo.find_commit(current).unwrap();
        current = match commit.parent_id(0) {
            Ok(oid) => oid,
            Err(_) => break,
        };
        // Safety: don't loop forever
        assert!(count <= 100, "Infinite loop walking branch commits");
    }

    assert_eq!(
        count, 1,
        "Feature branch side of the merge should have exactly 1 commit (the feature commit), \
         but found {}. Upstream commits leaked into the feature branch.",
        count
    );

    // Verify: feature-a branch ref should still exist and point at the
    // rebased feature commit (not at an upstream commit).
    let feature_branch = repo
        .find_branch("feature-a", BranchType::Local)
        .expect("feature-a branch should still exist");
    let feature_oid = feature_branch.get().target().unwrap();
    let feature_commit = repo.find_commit(feature_oid).unwrap();
    assert_eq!(
        feature_commit.summary().unwrap(),
        "Feature A work",
        "feature-a should still point at its own commit"
    );
}

/// Test that update works correctly with multiple woven branches.
#[test]
fn update_with_multiple_woven_branches() {
    let test_repo = TestRepo::new_with_remote();

    // Build:
    //   *   (HEAD, integration) Merge branch 'feature-b'
    //   |\
    //   | * (feature-b) Feature B work
    //   * | Merge branch 'feature-a'
    //   |\|
    //   | * (feature-a) Feature A work
    //   |/
    //   * (origin/main) Initial commit

    let merge_base_oid = test_repo.head_oid();

    // Feature A
    test_repo.create_branch_at_commit("feature-a", merge_base_oid);
    test_repo.switch_branch("feature-a");
    test_repo.commit("Feature A work", "feature-a.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

    // Feature B
    test_repo.create_branch_at_commit("feature-b", merge_base_oid);
    test_repo.switch_branch("feature-b");
    test_repo.commit("Feature B work", "feature-b.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-b");

    // Push upstream commits
    test_repo.add_remote_commits(&["Remote work"]);

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // Verify merge topology is preserved (HEAD is a merge)
    let repo = &test_repo.repo;
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2, "HEAD should be a merge (feature-b)");

    // Walk first-parent to find the feature-a merge
    let first_parent = head.parent(0).unwrap();
    assert_eq!(
        first_parent.parent_count(),
        2,
        "First parent of HEAD should be a merge (feature-a)"
    );

    // Both feature branches should still exist
    assert!(test_repo.branch_exists("feature-a"));
    assert!(test_repo.branch_exists("feature-b"));

    // Feature commits should be preserved
    let fa = repo
        .find_branch("feature-a", BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    let fb = repo
        .find_branch("feature-b", BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    assert_eq!(
        repo.find_commit(fa).unwrap().summary().unwrap(),
        "Feature A work"
    );
    assert_eq!(
        repo.find_commit(fb).unwrap().summary().unwrap(),
        "Feature B work"
    );
}

/// Test that a multi-commit branch is fully preserved after update with
/// new upstream commits (no cherry-picks involved).
#[test]
fn update_preserves_multi_commit_branch() {
    let test_repo = TestRepo::new_with_remote();

    // Starting state:
    //   *   (HEAD, integration) Merge branch 'feature-a'
    //   |\
    //   | * (feature-a) F3
    //   | * F2
    //   | * F1
    //   |/
    //   * (origin/main) Initial commit

    let merge_base_oid = test_repo.head_oid();

    test_repo.create_branch_at_commit("feature-a", merge_base_oid);
    test_repo.switch_branch("feature-a");
    test_repo.commit("F1", "f1.txt");
    test_repo.commit("F2", "f2.txt");
    test_repo.commit("F3", "f3.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

    // Push upstream changes (no overlap with feature commits)
    test_repo.add_remote_commits(&["Upstream work"]);

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    // Verify merge topology
    let repo = &test_repo.repo;
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2, "HEAD should still be a merge");

    // Feature-a should still exist and point at F3
    let fa = repo
        .find_branch("feature-a", BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    assert_eq!(repo.find_commit(fa).unwrap().summary().unwrap(), "F3");

    // Count commits on the branch side: should be 3 (F1, F2, F3)
    let merge_second_parent = head.parent(1).unwrap();
    let merge_first_parent = head.parent(0).unwrap();
    let mut count = 0;
    let mut current = merge_second_parent.id();
    loop {
        if current == merge_first_parent.id() {
            break;
        }
        count += 1;
        let commit = repo.find_commit(current).unwrap();
        current = match commit.parent_id(0) {
            Ok(oid) => oid,
            Err(_) => break,
        };
        assert!(count <= 100, "Infinite loop");
    }
    assert_eq!(
        count, 3,
        "Branch side should have exactly 3 commits (F1, F2, F3), got {}",
        count
    );
}

/// When 2 of 3 feature commits are cherry-picked upstream, only the
/// remaining commit should appear on the branch side of the merge.
#[test]
fn update_with_partially_cherry_picked_branch() {
    let test_repo = TestRepo::new_with_remote();

    // Starting state:
    //   *   (HEAD, integration) Merge branch 'feature-a'
    //   |\
    //   | * (feature-a) F3
    //   | * F2
    //   | * F1
    //   |/
    //   * (origin/main) Initial commit
    //
    // Then cherry-pick F1, F2 to upstream.
    // After update, feature-a should only show F3.

    let merge_base_oid = test_repo.head_oid();

    test_repo.create_branch_at_commit("feature-a", merge_base_oid);
    test_repo.switch_branch("feature-a");
    let f1_oid = test_repo.commit("F1", "f1.txt");
    let f2_oid = test_repo.commit("F2", "f2.txt");
    test_repo.commit("F3", "f3.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

    // Upstream cherry-picks F1 and F2
    test_repo.cherry_pick_to_remote(f1_oid, "F1");
    test_repo.cherry_pick_to_remote(f2_oid, "F2");

    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    let repo = &test_repo.repo;
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2, "HEAD should still be a merge");

    // Feature-a should point at F3
    let fa = repo
        .find_branch("feature-a", BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    assert_eq!(repo.find_commit(fa).unwrap().summary().unwrap(), "F3");

    // Branch side should have only 1 commit (F3), F1/F2 were dropped
    let merge_second_parent = head.parent(1).unwrap();
    let merge_first_parent = head.parent(0).unwrap();
    let mut count = 0;
    let mut current = merge_second_parent.id();
    loop {
        if current == merge_first_parent.id() {
            break;
        }
        count += 1;
        let commit = repo.find_commit(current).unwrap();
        current = match commit.parent_id(0) {
            Ok(oid) => oid,
            Err(_) => break,
        };
        assert!(count <= 100, "Infinite loop");
    }
    assert_eq!(
        count, 1,
        "Branch side should have exactly 1 commit (F3) after F1/F2 cherry-picked upstream, got {}",
        count
    );
}

/// When a feature branch's commit has been cherry-picked into upstream,
/// the weave-based rebase should handle it gracefully (git detects the
/// duplicate via patch-id and skips it).
#[test]
fn update_handles_branch_cherry_picked_into_upstream() {
    let test_repo = TestRepo::new_with_remote();

    // Build:
    //   *   (HEAD, integration) Merge branch 'feature-a'
    //   |\
    //   | * (feature-a) Feature A work
    //   |/
    //   * (origin/main) Initial

    let merge_base_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", merge_base_oid);
    test_repo.switch_branch("feature-a");
    let feature_oid = test_repo.commit("Feature A work", "feature-a.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

    // Simulate: upstream cherry-picks the feature commit
    test_repo.cherry_pick_to_remote(feature_oid, "Feature A work");

    // Run update
    let result = test_repo.in_dir(|| super::run(false));
    assert!(
        result.is_ok(),
        "update should succeed when branch is cherry-picked upstream: {:?}",
        result.err()
    );
}

/// When some commits in a branch are cherry-picked upstream but others
/// are not, the update should keep the non-cherry-picked commits and
/// skip the duplicates.
#[test]
fn update_handles_partial_cherry_pick_to_upstream() {
    let test_repo = TestRepo::new_with_remote();

    let merge_base_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", merge_base_oid);
    test_repo.switch_branch("feature-a");
    let f1_oid = test_repo.commit("F1", "f1.txt");
    test_repo.commit("F2", "f2.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

    // Upstream cherry-picks only F1
    test_repo.cherry_pick_to_remote(f1_oid, "F1");

    let result = test_repo.in_dir(|| super::run(false));
    assert!(
        result.is_ok(),
        "update should succeed with partial cherry-pick: {:?}",
        result.err()
    );

    // F2 should survive (it wasn't cherry-picked)
    let repo = &test_repo.repo;
    let fa = repo
        .find_branch("feature-a", BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    assert_eq!(
        repo.find_commit(fa).unwrap().summary().unwrap(),
        "F2",
        "feature-a should still have F2 after update"
    );

    // Branch side should have only 1 commit (F2), F1 was dropped
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let merge_second_parent = head.parent(1).unwrap();
    let merge_first_parent = head.parent(0).unwrap();
    let mut count = 0;
    let mut current = merge_second_parent.id();
    loop {
        if current == merge_first_parent.id() {
            break;
        }
        count += 1;
        let commit = repo.find_commit(current).unwrap();
        current = match commit.parent_id(0) {
            Ok(oid) => oid,
            Err(_) => break,
        };
        assert!(count <= 100, "Infinite loop");
    }
    assert_eq!(
        count, 1,
        "Branch side should have 1 commit (F2) after F1 cherry-picked upstream, got {}",
        count
    );
}

/// When ALL commits in a branch have been cherry-picked upstream, the
/// branch section becomes empty. Verify the update still succeeds and
/// produces a reasonable result.
#[test]
fn update_handles_fully_cherry_picked_branch() {
    let test_repo = TestRepo::new_with_remote();

    let merge_base_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", merge_base_oid);
    test_repo.switch_branch("feature-a");
    let f1_oid = test_repo.commit("F1", "f1.txt");
    let f2_oid = test_repo.commit("F2", "f2.txt");
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");

    // Upstream cherry-picks ALL feature commits
    test_repo.cherry_pick_to_remote(f1_oid, "F1");
    test_repo.cherry_pick_to_remote(f2_oid, "F2");

    let result = test_repo.in_dir(|| super::run(false));
    assert!(
        result.is_ok(),
        "update should succeed when all branch commits are cherry-picked: {:?}",
        result.err()
    );

    let repo = &test_repo.repo;
    let head = repo.head().unwrap().peel_to_commit().unwrap();

    // HEAD should still be on a branch
    assert!(
        repo.head().unwrap().is_branch(),
        "HEAD should still be on a branch after update"
    );

    // The merge commit should either survive as an empty merge (no unique
    // commits on the branch side) or collapse to linear history. Either is
    // acceptable — what matters is that feature-a does NOT point at unrelated
    // upstream commits.
    if head.parent_count() == 2 {
        // Merge survived: branch side should have 0 unique commits
        // (the merge exists but the branch section is empty)
        let merge_second_parent = head.parent(1).unwrap();
        let merge_first_parent = head.parent(0).unwrap();
        assert_eq!(
            merge_second_parent.id(),
            merge_first_parent.id(),
            "Empty merge should have both parents pointing at the same commit (the base)"
        );
    }

    // feature-a should still exist
    let fa_branch = repo.find_branch("feature-a", BranchType::Local);
    if let Ok(fa) = fa_branch {
        let fa_oid = fa.get().target().unwrap();
        let fa_commit = repo.find_commit(fa_oid).unwrap();
        // feature-a must NOT point at an upstream commit (the original bug).
        // It should point at a commit with one of its own messages, or at the
        // merge base if all its commits were dropped.
        let summary = fa_commit.summary().unwrap_or("");
        assert!(
            summary == "F1" || summary == "F2" || summary == head.summary().unwrap_or(""),
            "feature-a should not point at an unrelated upstream commit, \
             but points at: {}",
            summary
        );
    }
}

/// When the integration branch has a merge with inverted parent ordering
/// (feature as 1st parent, main as 2nd parent) and the feature branch was
/// already merged upstream, the update should drop the redundant merge and
/// produce linear history — not replay all feature commits and conflict.
///
/// Remote topology:
///   R ── C1 ────────────── merge_up(C1,F1) ── C2(feature.txt="feat v2") ── C3
///   └── F1(feature.txt="feat") ──┘
///
/// Integration topology (before update):
///   merge_local(F1, C1) ← HEAD  [inverted: 1st=F1, 2nd=C1]
///
/// Without the fix, walk_first_parent_line follows F1 → R → root (never
/// reaching the merge base). The fallback rebase replays F1 on top of C3,
/// which CONFLICTS because F1 writes feature.txt="feat" while C3 already
/// has feature.txt="feat v2" (modified in C2 after the upstream merge).
///
/// With the fix, inverted parents are detected and swapped. F1 goes to a
/// branch section. filter_upstream_commits sees F1 is already in upstream
/// (via merge_up) and drops it. Empty section → merge removed → linear.
#[test]
fn update_handles_inverted_parent_merge_on_integration() {
    let test_repo = TestRepo::new_with_remote();
    let remote_path = test_repo.remote_path().unwrap();
    let sig = Signature::now("Test", "test@test.com").unwrap();

    // Build remote: R ── C1 ── merge_up(C1,F1) ── C2
    //               └── F1 ──┘
    let (c1_oid, f1_oid) = {
        let rr = Repository::open_bare(&remote_path).unwrap();
        let root_oid = rr
            .find_branch("main", BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap();
        let root = rr.find_commit(root_oid).unwrap();

        // C1: adds main.txt
        let mut b = rr.treebuilder(Some(&root.tree().unwrap())).unwrap();
        b.insert("main.txt", rr.blob(b"main").unwrap(), 0o100644)
            .unwrap();
        let c1_tree = rr.find_tree(b.write().unwrap()).unwrap();
        let c1_oid = rr
            .commit(
                Some("refs/heads/main"),
                &sig,
                &sig,
                "C1",
                &c1_tree,
                &[&root],
            )
            .unwrap();
        let c1 = rr.find_commit(c1_oid).unwrap();

        // F1: adds feature.txt (forked from root, before C1)
        let mut b = rr.treebuilder(Some(&root.tree().unwrap())).unwrap();
        b.insert("feature.txt", rr.blob(b"feat").unwrap(), 0o100644)
            .unwrap();
        let f1_tree = rr.find_tree(b.write().unwrap()).unwrap();
        let f1_oid = rr
            .commit(None, &sig, &sig, "F1", &f1_tree, &[&root])
            .unwrap();
        let f1 = rr.find_commit(f1_oid).unwrap();

        // merge_up: normal merge of F1 into C1
        let mut b = rr.treebuilder(Some(&c1_tree)).unwrap();
        b.insert("feature.txt", rr.blob(b"feat").unwrap(), 0o100644)
            .unwrap();
        let mt = rr.find_tree(b.write().unwrap()).unwrap();
        let mu_oid = rr
            .commit(None, &sig, &sig, "Merge F1", &mt, &[&c1, &f1])
            .unwrap();
        rr.reference("refs/heads/main", mu_oid, true, "merge")
            .unwrap();

        // C2: post-merge work — modifies feature.txt so that replaying
        // F1 (which writes "feat") on top of C2 (which has "feat v2")
        // would CONFLICT without the fix.
        let mu = rr.find_commit(mu_oid).unwrap();
        let mut b = rr.treebuilder(Some(&mu.tree().unwrap())).unwrap();
        b.insert("feature.txt", rr.blob(b"feat v2").unwrap(), 0o100644)
            .unwrap();
        let c2_tree = rr.find_tree(b.write().unwrap()).unwrap();
        rr.commit(Some("refs/heads/main"), &sig, &sig, "C2", &c2_tree, &[&mu])
            .unwrap();

        (c1_oid, f1_oid)
    };

    // Fetch remote state
    test_repo
        .repo
        .find_remote("origin")
        .unwrap()
        .fetch(&["main"], None, None)
        .unwrap();

    let c1 = test_repo.repo.find_commit(c1_oid).unwrap();
    let f1 = test_repo.repo.find_commit(f1_oid).unwrap();

    // Set integration to C1 (merge base)
    test_repo
        .repo
        .reference("refs/heads/integration", c1_oid, true, "to C1")
        .unwrap();

    // Create INVERTED merge on integration: F1 (1st) + C1 (2nd)
    let mut b = test_repo
        .repo
        .treebuilder(Some(&c1.tree().unwrap()))
        .unwrap();
    b.insert(
        "feature.txt",
        test_repo.repo.blob(b"feat").unwrap(),
        0o100644,
    )
    .unwrap();
    let mt = test_repo.repo.find_tree(b.write().unwrap()).unwrap();
    let merge_oid = test_repo
        .repo
        .commit(
            None,
            &sig,
            &sig,
            "Merge main into feature",
            &mt,
            &[&f1, &c1],
        )
        .unwrap();

    test_repo
        .repo
        .reference("refs/heads/integration", merge_oid, true, "merge")
        .unwrap();
    test_repo.repo.set_head("refs/heads/integration").unwrap();
    test_repo
        .repo
        .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();

    // Verify inverted parents
    let head = test_repo.repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.parent_count(), 2);
    assert_eq!(head.parent_id(0).unwrap(), f1_oid);

    // Add one more remote commit
    test_repo.add_remote_commits(&["C3"]);

    // Run update — should succeed and flatten (no conflicts)
    let result = test_repo.in_dir(|| super::run(false));
    assert!(
        result.is_ok(),
        "update should handle inverted-parent merges: {:?}",
        result.err()
    );

    // Verify the rebase fully completed (not paused on conflicts)
    let git_dir = test_repo.repo.path().to_path_buf();
    assert!(
        !crate::git_commands::git_rebase::is_in_progress(&git_dir),
        "rebase should not be in progress — expected clean completion, not conflict pause"
    );

    // HEAD should be linear (redundant merge dropped)
    let new_head = test_repo.repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(
        new_head.parent_count(),
        1,
        "HEAD should be linear after update (redundant merge dropped)"
    );
}
