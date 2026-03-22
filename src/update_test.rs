use crate::git_commands;
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

#[test]
fn update_skips_integration_line_commits_already_in_upstream() {
    let test_repo = TestRepo::new_with_remote();

    // Create a commit on the integration line (local only, not yet in remote).
    test_repo.commit("Integration commit A", "a.txt");
    let commit_a = test_repo.head_oid();

    // Push commit A to a temporary remote branch so the bare repo holds the
    // object, without advancing origin/main yet.
    let workdir = test_repo.workdir();
    git_commands::run_git(&workdir, &["push", "origin", "HEAD:refs/heads/temp-a"]).unwrap();

    // Advance remote/main past commit A (simulating upstream absorbing it).
    let remote_path = test_repo.remote_path().unwrap();
    {
        let remote_repo = Repository::open_bare(&remote_path).unwrap();
        let sig = Signature::now("Test", "test@test.com").unwrap();
        let commit_a_obj = remote_repo.find_commit(commit_a).unwrap();
        let tree = commit_a_obj.tree().unwrap();
        let new_oid = remote_repo
            .commit(
                None,
                &sig,
                &sig,
                "New upstream commit",
                &tree,
                &[&commit_a_obj],
            )
            .unwrap();
        remote_repo
            .reference("refs/heads/main", new_oid, true, "advance main past A")
            .unwrap();
    }

    // origin/main in the work repo is still at Initial here. loom update
    // will fetch, detect that commit A is already in the new upstream, drop
    // it from the integration line, and fast-forward cleanly — no conflicts.
    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    let repo2 = Repository::open(test_repo.workdir()).unwrap();
    let new_head = repo2.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(
        new_head.summary().unwrap_or(""),
        "New upstream commit",
        "integration should be at the new upstream tip"
    );
    assert_eq!(new_head.parent_count(), 1);
}

#[test]
fn update_drops_merge_when_feature_branch_fully_integrated_into_upstream() {
    let test_repo = TestRepo::new_with_remote();
    let initial_oid = test_repo.head_oid();

    // Build woven topology: integration merges feature-a.
    test_repo.create_branch_at_commit("feature-a", initial_oid);
    test_repo.switch_branch("feature-a");
    test_repo.commit("Feature A work", "feature-a.txt");
    let feature_tip = test_repo.head_oid();

    test_repo.switch_branch("integration");
    test_repo.commit_merge("Merge branch 'feature-a'", initial_oid, feature_tip);
    assert_eq!(test_repo.head_commit().parent_count(), 2);

    // Push feature-a so the remote has the feature_tip object.
    let workdir = test_repo.workdir();
    git_commands::run_git(&workdir, &["push", "origin", "feature-a"]).unwrap();

    // Advance remote/main past feature-a to simulate it being merged upstream.
    let remote_path = test_repo.remote_path().unwrap();
    {
        let remote_repo = Repository::open_bare(&remote_path).unwrap();
        let sig = Signature::now("Test", "test@test.com").unwrap();
        let feature_commit = remote_repo.find_commit(feature_tip).unwrap();
        let tree = feature_commit.tree().unwrap();
        // Create commit without touching the ref (parent is not the current main tip).
        let new_oid = remote_repo
            .commit(
                None,
                &sig,
                &sig,
                "Upstream integration of feature-a",
                &tree,
                &[&feature_commit],
            )
            .unwrap();
        // Force-update refs/heads/main to the new commit.
        remote_repo
            .reference(
                "refs/heads/main",
                new_oid,
                true,
                "advance main past feature-a",
            )
            .unwrap();
    }

    // origin/main in the work repo is still at initial here — loom update
    // will fetch, detect that feature_tip is now in upstream, and remove the
    // null merge.
    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    let repo2 = Repository::open(test_repo.workdir()).unwrap();
    let new_head = repo2.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(
        new_head.parent_count(),
        1,
        "integration should be flat after feature-a is fully merged into upstream"
    );
}

#[test]
fn update_trims_partially_integrated_branch() {
    let test_repo = TestRepo::new_with_remote();
    let initial_oid = test_repo.head_oid();

    // Build a feature-a branch with two commits.
    test_repo.create_branch_at_commit("feature-a", initial_oid);
    test_repo.switch_branch("feature-a");
    test_repo.commit("Feature A part 1", "feature-a-1.txt");
    let part1_tip = test_repo.head_oid();
    test_repo.commit("Feature A part 2", "feature-a-2.txt");
    let part2_tip = test_repo.head_oid();

    // Merge feature-a (both commits) into integration.
    test_repo.switch_branch("integration");
    test_repo.commit_merge("Merge branch 'feature-a'", initial_oid, part2_tip);
    assert_eq!(test_repo.head_commit().parent_count(), 2);

    // Push feature-a so the remote has both commit objects.
    let workdir = test_repo.workdir();
    git_commands::run_git(&workdir, &["push", "origin", "feature-a"]).unwrap();

    // Advance remote/main to include only part1 (not part2), simulating
    // partial upstream integration.
    let remote_path = test_repo.remote_path().unwrap();
    {
        let remote_repo = Repository::open_bare(&remote_path).unwrap();
        let sig = Signature::now("Test", "test@test.com").unwrap();
        let part1_commit = remote_repo.find_commit(part1_tip).unwrap();
        let tree = part1_commit.tree().unwrap();
        // Create commit without touching the ref (parent is not the current main tip).
        let new_oid = remote_repo
            .commit(
                None,
                &sig,
                &sig,
                "Upstream integration of feature-a part 1",
                &tree,
                &[&part1_commit],
            )
            .unwrap();
        // Force-update refs/heads/main to the new commit.
        remote_repo
            .reference(
                "refs/heads/main",
                new_oid,
                true,
                "advance main past feature-a part 1",
            )
            .unwrap();
    }

    // origin/main is still at initial here. After update, part1 is in
    // upstream so it is trimmed from the branch section; part2 is not, so
    // the merge is preserved with only part2 left in the branch.
    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    let repo2 = Repository::open(test_repo.workdir()).unwrap();
    let new_head = repo2.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(
        new_head.parent_count(),
        2,
        "integration should still be a merge: part2 was not integrated upstream"
    );
    assert!(
        test_repo.branch_exists("feature-a"),
        "feature-a branch should still exist after partial trim"
    );
}

#[test]
fn update_drops_woven_section_when_commits_cherry_picked_into_upstream() {
    // Regression: when upstream advances by cherry-picking a woven branch
    // commit (same diff, different OID), loom must detect it via git-cherry
    // and not try to re-apply it — which would cause a "cherry-pick is now
    // empty" error during the interactive rebase.
    let test_repo = TestRepo::new_with_remote();
    let initial_oid = test_repo.head_oid();

    // Add a "buffer" commit to remote/main first so that the cherry-picked
    // commit will have a different parent than the original, giving it a
    // different OID but the same diff.
    test_repo.add_remote_commits(&["Upstream buffer"]);

    // Build woven topology: integration merges feature-a (1 commit).
    test_repo.create_branch_at_commit("feature-a", initial_oid);
    test_repo.switch_branch("feature-a");
    test_repo.commit("Feature A work", "feature-a.txt");
    let feature_tip = test_repo.head_oid();

    test_repo.switch_branch("integration");
    test_repo.commit_merge("Merge branch 'feature-a'", initial_oid, feature_tip);
    assert_eq!(test_repo.head_commit().parent_count(), 2);

    // Push the feature-a commit so the bare repo has the object.
    let workdir = test_repo.workdir();
    git_commands::run_git(&workdir, &["push", "origin", "feature-a"]).unwrap();

    // Advance remote/main by cherry-picking feature-a: same tree/message but
    // different parent → different OID, same patch-id.
    let remote_path = test_repo.remote_path().unwrap();
    {
        let remote_repo = Repository::open_bare(&remote_path).unwrap();
        let sig = Signature::now("Test", "test@test.com").unwrap();
        let feature_commit = remote_repo.find_commit(feature_tip).unwrap();
        let main_tip = remote_repo
            .find_branch("main", BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap();
        let main_commit = remote_repo.find_commit(main_tip).unwrap();
        // Cherry-pick: same tree and message but different parent → different OID.
        remote_repo
            .commit(
                Some("refs/heads/main"),
                &sig,
                &sig,
                "Feature A work",
                &feature_commit.tree().unwrap(),
                &[&main_commit],
            )
            .unwrap();
    }

    // After update, the branch section should be detected as fully
    // cherry-picked (via git cherry content comparison), and the merge should
    // be removed from the integration branch.
    let result = test_repo.in_dir(|| super::run(false));
    assert!(result.is_ok(), "update failed: {:?}", result.err());

    let repo2 = Repository::open(test_repo.workdir()).unwrap();
    let new_head = repo2.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(
        new_head.parent_count(),
        1,
        "integration should be flat after feature-a is cherry-picked into upstream"
    );
}
