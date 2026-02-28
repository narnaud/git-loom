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

    let result = test_repo.in_dir(|| super::run());
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

    let result = test_repo.in_dir(|| super::run());
    assert!(result.is_ok(), "update failed: {:?}", result.err());
}

#[test]
fn update_fails_on_detached_head() {
    let test_repo = TestRepo::new_with_remote();
    let oid = test_repo.head_oid();
    test_repo.set_detached_head(oid);

    let result = test_repo.in_dir(|| super::run());
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

    let result = test_repo.in_dir(|| super::run());
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

    let result = test_repo.in_dir(|| super::run());
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

    let result = test_repo.in_dir(|| super::run());
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

    let result = test_repo.in_dir(|| super::run());
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
    let result = test_repo.in_dir(|| super::run());
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
