use std::fs;
use std::path::Path;

use git2::{BranchType, Repository, Signature};

use crate::git::gather_repo_info;

/// Create a signature for test commits.
fn sig() -> Signature<'static> {
    Signature::now("Test", "test@test.com").unwrap()
}

/// Create a commit on the current HEAD (or as initial commit if repo is empty).
/// Returns the new commit OID.
fn make_commit(repo: &Repository, message: &str) -> git2::Oid {
    let sig = sig();
    let tree_id = {
        let mut index = repo.index().unwrap();
        index.write_tree().unwrap()
    };
    let tree = repo.find_tree(tree_id).unwrap();

    if let Ok(head) = repo.head() {
        let parent = repo.find_commit(head.target().unwrap()).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
            .unwrap()
    } else {
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
            .unwrap()
    }
}

/// Create a commit that touches a file (so working tree changes can be tested).
fn make_commit_with_file(repo: &Repository, message: &str, filename: &str) -> git2::Oid {
    let path = repo.workdir().unwrap().join(filename);
    fs::write(&path, message).unwrap();

    let mut index = repo.index().unwrap();
    index.add_path(Path::new(filename)).unwrap();
    index.write().unwrap();

    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = sig();

    if let Ok(head) = repo.head() {
        let parent = repo.find_commit(head.target().unwrap()).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
            .unwrap()
    } else {
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
            .unwrap()
    }
}

/// Create a merge commit combining two parent commits.
fn make_merge_commit(
    repo: &Repository,
    message: &str,
    parent1_oid: git2::Oid,
    parent2_oid: git2::Oid,
) -> git2::Oid {
    let sig = sig();
    let p1 = repo.find_commit(parent1_oid).unwrap();
    let p2 = repo.find_commit(parent2_oid).unwrap();
    let tree = repo.find_tree(p1.tree_id()).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&p1, &p2])
        .unwrap()
}

/// Set up a test repo with a "remote" (bare repo) and a working clone.
/// The working clone has one initial commit pushed to origin/main, and an
/// "integration" branch tracking origin/main.
/// Returns (working repo, path to temp dir).
fn setup_repo() -> (Repository, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();

    // Create a bare "remote"
    let remote_path = dir.path().join("remote.git");
    let remote_repo = Repository::init_bare(&remote_path).unwrap();

    // Create initial commit in the bare repo so it has a main branch
    {
        let sig = sig();
        let tree_id = {
            let mut index = remote_repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = remote_repo.find_tree(tree_id).unwrap();
        remote_repo
            .commit(Some("refs/heads/main"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();
    }

    // Clone it
    let work_path = dir.path().join("work");
    let repo = Repository::clone(remote_path.to_str().unwrap(), &work_path).unwrap();

    // Create integration branch pointing at main, tracking origin/main
    {
        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("integration", &head_commit, false).unwrap();
        repo.set_head("refs/heads/integration").unwrap();

        // Set upstream tracking
        let mut integration = repo.find_branch("integration", BranchType::Local).unwrap();
        integration.set_upstream(Some("origin/main")).unwrap();
    }

    (repo, dir)
}

// ── Tests ──────────────────────────────────────────────────────────────

#[test]
fn no_commits_ahead_of_upstream() {
    let (repo, _dir) = setup_repo();
    let info = gather_repo_info(&repo).unwrap();

    assert!(info.commits.is_empty());
    assert!(info.branches.is_empty());
    assert_eq!(info.upstream.label, "origin/main");
    assert_eq!(info.upstream.commits_ahead, 0);
}

#[test]
fn commits_without_branches() {
    let (repo, _dir) = setup_repo();

    make_commit(&repo, "First");
    make_commit(&repo, "Second");
    make_commit(&repo, "Third");

    let info = gather_repo_info(&repo).unwrap();

    assert_eq!(info.commits.len(), 3);
    assert_eq!(info.commits[0].message, "Third");
    assert_eq!(info.commits[1].message, "Second");
    assert_eq!(info.commits[2].message, "First");
    // No feature branches detected (only integration branch exists)
    assert!(info.branches.is_empty());
}

#[test]
fn single_feature_branch() {
    let (repo, _dir) = setup_repo();

    make_commit(&repo, "A1");
    let a2_oid = make_commit(&repo, "A2");

    // Create feature-a branch at current HEAD
    let commit = repo.find_commit(a2_oid).unwrap();
    repo.branch("feature-a", &commit, false).unwrap();

    let info = gather_repo_info(&repo).unwrap();

    assert_eq!(info.commits.len(), 2);
    assert_eq!(info.branches.len(), 1);
    assert_eq!(info.branches[0].name, "feature-a");
    assert_eq!(info.branches[0].tip_oid, a2_oid);
}

#[test]
fn multiple_independent_branches() {
    let (repo, _dir) = setup_repo();

    // feature-a: A1 on top of upstream
    make_commit(&repo, "A1");
    let a1_oid = repo.head().unwrap().target().unwrap();
    let a1_commit = repo.find_commit(a1_oid).unwrap();
    repo.branch("feature-a", &a1_commit, false).unwrap();

    // Merge feature-a into integration (creates a merge commit)
    let upstream_oid = repo
        .find_branch("origin/main", BranchType::Remote)
        .unwrap()
        .get()
        .target()
        .unwrap();
    make_merge_commit(&repo, "Merge feature-a", a1_oid, upstream_oid);

    // feature-b: B1 on top of the merge
    make_commit(&repo, "B1");
    let b1_oid = repo.head().unwrap().target().unwrap();
    let b1_commit = repo.find_commit(b1_oid).unwrap();
    repo.branch("feature-b", &b1_commit, false).unwrap();

    let info = gather_repo_info(&repo).unwrap();

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
    let (repo, _dir) = setup_repo();

    // feature-a: A1, A2
    make_commit(&repo, "A1");
    let a2_oid = make_commit(&repo, "A2");
    let a2_commit = repo.find_commit(a2_oid).unwrap();
    repo.branch("feature-a", &a2_commit, false).unwrap();

    // feature-b: B1, B2 on top of feature-a
    make_commit(&repo, "B1");
    let b2_oid = make_commit(&repo, "B2");
    let b2_commit = repo.find_commit(b2_oid).unwrap();
    repo.branch("feature-b", &b2_commit, false).unwrap();

    let info = gather_repo_info(&repo).unwrap();

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
    let (repo, _dir) = setup_repo();

    let c1_oid = make_commit(&repo, "C1");

    // Create a side branch from upstream, then merge it
    let upstream_oid = repo
        .find_branch("origin/main", BranchType::Remote)
        .unwrap()
        .get()
        .target()
        .unwrap();

    make_merge_commit(&repo, "Merge side branch", c1_oid, upstream_oid);
    make_commit(&repo, "C2");

    let info = gather_repo_info(&repo).unwrap();

    let messages: Vec<&str> = info.commits.iter().map(|c| c.message.as_str()).collect();
    assert_eq!(messages, vec!["C2", "C1"]);
}

#[test]
fn detached_head_returns_error() {
    let (repo, _dir) = setup_repo();

    let head_oid = repo.head().unwrap().target().unwrap();
    repo.set_head_detached(head_oid).unwrap();

    let result = gather_repo_info(&repo);
    assert!(result.is_err());
    assert!(result.unwrap_err().message().contains("detached"));
}

#[test]
fn no_upstream_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();

    // Create an initial commit so HEAD exists
    let sig = sig();
    let tree_id = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "Init", &tree, &[])
        .unwrap();

    let result = gather_repo_info(&repo);
    assert!(result.is_err());
    assert!(result.unwrap_err().message().contains("upstream"));
}

#[test]
fn working_tree_changes_detected() {
    let (repo, _dir) = setup_repo();

    make_commit_with_file(&repo, "base", "tracked.txt");

    // Modify a tracked file
    let workdir = repo.workdir().unwrap();
    fs::write(workdir.join("tracked.txt"), "modified").unwrap();

    // Add an untracked file
    fs::write(workdir.join("untracked.txt"), "new").unwrap();

    let info = gather_repo_info(&repo).unwrap();

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
    let (repo, _dir) = setup_repo();

    let info = gather_repo_info(&repo).unwrap();
    assert!(info.working_changes.is_empty());
}

#[test]
fn upstream_ahead_of_merge_base() {
    let (repo, _dir) = setup_repo();

    // Make a commit on the integration branch
    make_commit(&repo, "Local work");

    // Push new commits to origin/main (simulate upstream moving ahead)
    // We do this by adding commits directly to the remote's main branch
    let remote_path = _dir.path().join("remote.git");
    let remote_repo = Repository::open_bare(&remote_path).unwrap();
    {
        let sig = sig();
        let main_oid = remote_repo
            .find_branch("main", BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap();
        let parent = remote_repo.find_commit(main_oid).unwrap();
        let tree = parent.tree().unwrap();
        let c1 = remote_repo
            .commit(Some("refs/heads/main"), &sig, &sig, "Remote 1", &tree, &[&parent])
            .unwrap();
        let c1_commit = remote_repo.find_commit(c1).unwrap();
        remote_repo
            .commit(Some("refs/heads/main"), &sig, &sig, "Remote 2", &tree, &[&c1_commit])
            .unwrap();
    }

    // Fetch to update origin/main in the working repo
    repo.find_remote("origin")
        .unwrap()
        .fetch(&["main"], None, None)
        .unwrap();

    let info = gather_repo_info(&repo).unwrap();

    // Upstream is 2 commits ahead of the merge-base (which is the original "Initial" commit)
    assert_eq!(info.upstream.commits_ahead, 2);
    assert_eq!(info.upstream.base_message, "Initial");
    assert_eq!(info.commits.len(), 1);
    assert_eq!(info.commits[0].message, "Local work");
}

#[test]
fn branch_at_upstream_is_not_detected() {
    let (repo, _dir) = setup_repo();

    // Create a branch pointing at the upstream commit (not ahead)
    let upstream_oid = repo
        .find_branch("origin/main", BranchType::Remote)
        .unwrap()
        .get()
        .target()
        .unwrap();
    let upstream_commit = repo.find_commit(upstream_oid).unwrap();
    repo.branch("stale-branch", &upstream_commit, false)
        .unwrap();

    make_commit(&repo, "Ahead");

    let info = gather_repo_info(&repo).unwrap();

    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        !branch_names.contains(&"stale-branch"),
        "branch at upstream should not be detected, got: {:?}",
        branch_names
    );
}

// ── Tests for target resolution ────────────────────────────────────────

/// Create a simple test repo without upstream (for resolve_target tests).
fn setup_simple_repo() -> (Repository, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();

    // Create an initial commit
    {
        let sig = sig();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }

    (repo, dir)
}

#[test]
fn resolve_full_commit_hash() {
    let (repo, _dir) = setup_simple_repo();
    make_commit_with_file(&repo, "Second commit", "file.txt");

    let head_oid = repo.head().unwrap().target().unwrap();
    let result = crate::git::resolve_target(&repo, &head_oid.to_string());

    assert!(result.is_ok());
    match result.unwrap() {
        crate::git::Target::Commit(hash) => assert_eq!(hash, head_oid.to_string()),
        crate::git::Target::Branch(_) => panic!("Expected Commit, got Branch"),
        crate::git::Target::File(_) => panic!("Expected Commit, got File"),
    }
}

#[test]
fn resolve_partial_commit_hash() {
    let (repo, _dir) = setup_simple_repo();
    make_commit_with_file(&repo, "Second commit", "file.txt");

    let head_oid = repo.head().unwrap().target().unwrap();
    let partial_hash = &head_oid.to_string()[..7];
    let result = crate::git::resolve_target(&repo, partial_hash);

    assert!(result.is_ok());
    match result.unwrap() {
        crate::git::Target::Commit(hash) => assert_eq!(hash, head_oid.to_string()),
        crate::git::Target::Branch(_) => panic!("Expected Commit, got Branch"),
        crate::git::Target::File(_) => panic!("Expected Commit, got File"),
    }
}

#[test]
fn resolve_invalid_target_fails() {
    let (repo, _dir) = setup_simple_repo();

    let result = crate::git::resolve_target(&repo, "nonexistent");

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
