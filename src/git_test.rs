use crate::git::gather_repo_info;
use crate::test_helpers::TestRepo;

// ── Tests ──────────────────────────────────────────────────────────────

#[test]
fn no_commits_ahead_of_upstream() {
    let test_repo = TestRepo::new_with_remote();
    let info = gather_repo_info(&test_repo.repo).unwrap();

    assert!(info.commits.is_empty());
    assert!(info.branches.is_empty());
    assert_eq!(info.upstream.label, "origin/main");
    assert_eq!(info.upstream.commits_ahead, 0);
}

#[test]
fn commits_without_branches() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit_empty("First");
    test_repo.commit_empty("Second");
    test_repo.commit_empty("Third");

    let info = gather_repo_info(&test_repo.repo).unwrap();

    assert_eq!(info.commits.len(), 3);
    assert_eq!(info.commits[0].message, "Third");
    assert_eq!(info.commits[1].message, "Second");
    assert_eq!(info.commits[2].message, "First");
    // No feature branches detected (only integration branch exists)
    assert!(info.branches.is_empty());
}

#[test]
fn single_feature_branch() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit_empty("A1");
    let a2_oid = test_repo.commit_empty("A2");

    // Create feature-a branch at current HEAD
    test_repo.create_branch_at_commit("feature-a", a2_oid);

    let info = gather_repo_info(&test_repo.repo).unwrap();

    assert_eq!(info.commits.len(), 2);
    assert_eq!(info.branches.len(), 1);
    assert_eq!(info.branches[0].name, "feature-a");
    assert_eq!(info.branches[0].tip_oid, a2_oid);
}

#[test]
fn multiple_independent_branches() {
    let test_repo = TestRepo::new_with_remote();

    // feature-a: A1 on top of upstream
    test_repo.commit_empty("A1");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", a1_oid);

    // Merge feature-a into integration (creates a merge commit)
    let upstream_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.commit_merge("Merge feature-a", a1_oid, upstream_oid);

    // feature-b: B1 on top of the merge
    test_repo.commit_empty("B1");
    let b1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-b", b1_oid);

    let info = gather_repo_info(&test_repo.repo).unwrap();

    // Merge commit should be filtered out
    let messages: Vec<&str> = info.commits.iter().map(|c| c.message.as_str()).collect();
    assert!(
        !messages.iter().any(|m| m.starts_with("Merge")),
        "merge commits should be filtered out, got: {:?}",
        messages
    );

    // Both branches detected
    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(branch_names.contains(&"feature-a"));
    assert!(branch_names.contains(&"feature-b"));

    // Integration branch should NOT be in the list
    assert!(!branch_names.contains(&"integration"));
}

#[test]
fn stacked_branches() {
    let test_repo = TestRepo::new_with_remote();

    // feature-a: A1, A2
    test_repo.commit_empty("A1");
    let a2_oid = test_repo.commit_empty("A2");
    test_repo.create_branch_at_commit("feature-a", a2_oid);

    // feature-b: B1, B2 on top of feature-a
    test_repo.commit_empty("B1");
    let b2_oid = test_repo.commit_empty("B2");
    test_repo.create_branch_at_commit("feature-b", b2_oid);

    let info = gather_repo_info(&test_repo.repo).unwrap();

    assert_eq!(info.commits.len(), 4);
    assert_eq!(info.commits[0].message, "B2");
    assert_eq!(info.commits[1].message, "B1");
    assert_eq!(info.commits[2].message, "A2");
    assert_eq!(info.commits[3].message, "A1");

    // B1's parent should be A2 (stacked)
    assert_eq!(info.commits[1].parent_oid, Some(a2_oid));

    // Both branches detected
    assert_eq!(info.branches.len(), 2);
}

#[test]
fn merge_commits_are_filtered() {
    let test_repo = TestRepo::new_with_remote();

    let c1_oid = test_repo.commit_empty("C1");

    // Create a side branch from upstream, then merge it
    let upstream_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.commit_merge("Merge side branch", c1_oid, upstream_oid);
    test_repo.commit_empty("C2");

    let info = gather_repo_info(&test_repo.repo).unwrap();

    let messages: Vec<&str> = info.commits.iter().map(|c| c.message.as_str()).collect();
    assert_eq!(messages, vec!["C2", "C1"]);
}

#[test]
fn detached_head_returns_error() {
    let test_repo = TestRepo::new_with_remote();

    let head_oid = test_repo.head_oid();
    test_repo.set_detached_head(head_oid);

    let result = gather_repo_info(&test_repo.repo);
    assert!(result.is_err());
    assert!(result.unwrap_err().message().contains("detached"));
}

#[test]
fn no_upstream_returns_error() {
    let test_repo = TestRepo::new();

    let result = gather_repo_info(&test_repo.repo);
    assert!(result.is_err());
    assert!(result.unwrap_err().message().contains("upstream"));
}

#[test]
fn working_tree_changes_detected() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("base", "tracked.txt");

    // Modify a tracked file
    test_repo.write_file("tracked.txt", "modified");

    // Add an untracked file
    test_repo.write_file("untracked.txt", "new");

    let info = gather_repo_info(&test_repo.repo).unwrap();

    let paths: Vec<&str> = info
        .working_changes
        .iter()
        .map(|c| c.path.as_str())
        .collect();
    assert!(paths.contains(&"tracked.txt"));
    assert!(paths.contains(&"untracked.txt"));

    let tracked = info
        .working_changes
        .iter()
        .find(|c| c.path == "tracked.txt")
        .unwrap();
    assert_eq!(tracked.status, 'M');

    let untracked = info
        .working_changes
        .iter()
        .find(|c| c.path == "untracked.txt")
        .unwrap();
    assert_eq!(untracked.status, 'A');
}

#[test]
fn no_working_changes_when_clean() {
    let test_repo = TestRepo::new_with_remote();

    let info = gather_repo_info(&test_repo.repo).unwrap();
    assert!(info.working_changes.is_empty());
}

#[test]
fn upstream_ahead_of_merge_base() {
    let test_repo = TestRepo::new_with_remote();

    // Make a commit on the integration branch
    test_repo.commit_empty("Local work");

    // Push new commits to origin/main (simulate upstream moving ahead)
    test_repo.add_remote_commits(&["Remote 1", "Remote 2"]);

    // Fetch to update origin/main in the working repo
    test_repo.fetch_remote();

    let info = gather_repo_info(&test_repo.repo).unwrap();

    // Upstream is 2 commits ahead of the merge-base (which is the original "Initial" commit)
    assert_eq!(info.upstream.commits_ahead, 2);
    assert_eq!(info.upstream.base_message, "Initial");
    assert_eq!(info.commits.len(), 1);
    assert_eq!(info.commits[0].message, "Local work");
}

#[test]
fn branch_at_upstream_is_not_detected() {
    let test_repo = TestRepo::new_with_remote();

    // Create a branch pointing at the upstream commit (not ahead)
    let upstream_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at_commit("stale-branch", upstream_oid);

    test_repo.commit_empty("Ahead");

    let info = gather_repo_info(&test_repo.repo).unwrap();

    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        !branch_names.contains(&"stale-branch"),
        "branch at upstream should not be detected, got: {:?}",
        branch_names
    );
}

// ── Tests for target resolution ────────────────────────────────────────

#[test]
fn resolve_full_commit_hash() {
    let test_repo = TestRepo::new();
    test_repo.commit("Second commit", "file.txt");

    let head_oid = test_repo.head_oid();
    let result = crate::git::resolve_target(&test_repo.repo, &head_oid.to_string());

    assert!(result.is_ok());
    match result.unwrap() {
        crate::git::Target::Commit(hash) => assert_eq!(hash, head_oid.to_string()),
        crate::git::Target::Branch(_) => panic!("Expected Commit, got Branch"),
        crate::git::Target::File(_) => panic!("Expected Commit, got File"),
    }
}

#[test]
fn resolve_partial_commit_hash() {
    let test_repo = TestRepo::new();
    test_repo.commit("Second commit", "file.txt");

    let head_oid = test_repo.head_oid();
    let partial_hash = &head_oid.to_string()[..7];
    let result = crate::git::resolve_target(&test_repo.repo, partial_hash);

    assert!(result.is_ok());
    match result.unwrap() {
        crate::git::Target::Commit(hash) => assert_eq!(hash, head_oid.to_string()),
        crate::git::Target::Branch(_) => panic!("Expected Commit, got Branch"),
        crate::git::Target::File(_) => panic!("Expected Commit, got File"),
    }
}

#[test]
fn resolve_invalid_target_fails() {
    let test_repo = TestRepo::new();

    let result = crate::git::resolve_target(&test_repo.repo, "nonexistent");

    // Without upstream, shortid resolution should fail because gather_repo_info fails
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    // Error should mention either "upstream" or "shortid"
    assert!(
        err_msg.contains("upstream") || err_msg.contains("shortid"),
        "Expected error about upstream or shortid, got: {}",
        err_msg
    );
}
