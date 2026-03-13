use crate::git::{self, Target, TargetKind, gather_repo_info};
use crate::test_helpers::TestRepo;

// ── Tests ──────────────────────────────────────────────────────────────

#[test]
fn no_commits_ahead_of_upstream() {
    let test_repo = TestRepo::new_with_remote();
    let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();

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

    let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();

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

    let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();

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

    let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();

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

    let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();

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

    let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();

    let messages: Vec<&str> = info.commits.iter().map(|c| c.message.as_str()).collect();
    assert_eq!(messages, vec!["C2", "C1"]);
}

#[test]
fn detached_head_returns_error() {
    let test_repo = TestRepo::new_with_remote();

    let head_oid = test_repo.head_oid();
    test_repo.set_detached_head(head_oid);

    let result = gather_repo_info(&test_repo.repo, false, 1);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("detached"));
}

#[test]
fn no_upstream_returns_error() {
    let test_repo = TestRepo::new();

    let result = gather_repo_info(&test_repo.repo, false, 1);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("upstream"));
}

#[test]
fn working_tree_changes_detected() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("base", "tracked.txt");

    // Modify a tracked file
    test_repo.write_file("tracked.txt", "modified");

    // Add an untracked file
    test_repo.write_file("untracked.txt", "new");

    let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();

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
    assert_eq!(tracked.index, ' ');
    assert_eq!(tracked.worktree, 'M');

    let untracked = info
        .working_changes
        .iter()
        .find(|c| c.path == "untracked.txt")
        .unwrap();
    assert_eq!(untracked.index, '?');
    assert_eq!(untracked.worktree, '?');
}

#[test]
fn no_working_changes_when_clean() {
    let test_repo = TestRepo::new_with_remote();

    let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();
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

    let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();

    // Upstream is 2 commits ahead of the merge-base (which is the original "Initial" commit)
    assert_eq!(info.upstream.commits_ahead, 2);
    assert_eq!(info.upstream.base_message, "Initial");
    assert_eq!(info.commits.len(), 1);
    assert_eq!(info.commits[0].message, "Local work");
}

#[test]
fn branch_at_upstream_is_detected() {
    let test_repo = TestRepo::new_with_remote();

    // Create a branch pointing at the upstream commit (not ahead)
    let upstream_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at_commit("stale-branch", upstream_oid);

    test_repo.commit_empty("Ahead");

    let info = gather_repo_info(&test_repo.repo, false, 1).unwrap();

    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        branch_names.contains(&"stale-branch"),
        "branch at upstream should be detected, got: {:?}",
        branch_names
    );
}

// ── Tests for target resolution ────────────────────────────────────────

#[test]
fn resolve_full_commit_hash() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("Second commit");

    let head_oid = test_repo.head_oid();
    let result = test_repo.in_dir(|| {
        crate::git::resolve_arg(
            &test_repo.repo,
            &head_oid.to_string(),
            &[crate::git::TargetKind::Commit],
        )
    });

    assert!(result.is_ok());
    match result.unwrap() {
        crate::git::Target::Commit(hash) => assert_eq!(hash, head_oid.to_string()),
        crate::git::Target::Branch(_) => panic!("Expected Commit, got Branch"),
        crate::git::Target::File(_) => panic!("Expected Commit, got File"),
        crate::git::Target::Unstaged => panic!("Expected Commit, got Unstaged"),
        crate::git::Target::CommitFile { .. } => panic!("Expected Commit, got CommitFile"),
    }
}

#[test]
fn resolve_partial_commit_hash() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("Second commit");

    let head_oid = test_repo.head_oid();
    let partial_hash = &head_oid.to_string()[..7];
    let result = test_repo.in_dir(|| {
        crate::git::resolve_arg(
            &test_repo.repo,
            partial_hash,
            &[crate::git::TargetKind::Commit],
        )
    });

    assert!(result.is_ok());
    match result.unwrap() {
        crate::git::Target::Commit(hash) => assert_eq!(hash, head_oid.to_string()),
        crate::git::Target::Branch(_) => panic!("Expected Commit, got Branch"),
        crate::git::Target::File(_) => panic!("Expected Commit, got File"),
        crate::git::Target::Unstaged => panic!("Expected Commit, got Unstaged"),
        crate::git::Target::CommitFile { .. } => panic!("Expected Commit, got CommitFile"),
    }
}

#[test]
fn resolve_invalid_target_fails() {
    let test_repo = TestRepo::new_with_remote();

    let result = test_repo.in_dir(|| {
        crate::git::resolve_arg(
            &test_repo.repo,
            "nonexistent",
            &[
                crate::git::TargetKind::Commit,
                crate::git::TargetKind::Branch,
                crate::git::TargetKind::File,
            ],
        )
    });

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        !err_msg.is_empty(),
        "Expected non-empty error message, got: {}",
        err_msg
    );
}

// ── Tests for resolve_arg ───────────────────────────────────────────────

#[test]
fn resolve_arg_file_on_disk() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("hello.txt", "content");
    test_repo.in_dir(|| {
        let result = git::resolve_arg(&test_repo.repo, "hello.txt", &[TargetKind::File]).unwrap();
        assert_eq!(result, Target::File("hello.txt".to_string()));
    });
}

#[test]
fn resolve_arg_file_cwd_relative() {
    let test_repo = TestRepo::new_with_remote();
    let sub_dir = test_repo.workdir().join("sub");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(sub_dir.join("deep.txt"), "content").unwrap();
    test_repo.in_dir_path(&sub_dir, || {
        let result = git::resolve_arg(&test_repo.repo, "deep.txt", &[TargetKind::File]).unwrap();
        assert_eq!(result, Target::File("sub/deep.txt".to_string()));
    });
}

#[test]
fn resolve_arg_file_not_found_errors() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.in_dir(|| {
        let result = git::resolve_arg(&test_repo.repo, "nope.txt", &[TargetKind::File]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("file"),
            "error should mention accepted types: {msg}"
        );
    });
}

#[test]
fn resolve_arg_branch() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", a1_oid);
    test_repo.in_dir(|| {
        let result = git::resolve_arg(&test_repo.repo, "feature-a", &[TargetKind::Branch]).unwrap();
        assert_eq!(result, Target::Branch("feature-a".to_string()));
    });
}

#[test]
fn resolve_arg_branch_not_accepted_skips() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", a1_oid);
    test_repo.in_dir(|| {
        // Only accept File — branch should not match
        let result = git::resolve_arg(&test_repo.repo, "feature-a", &[TargetKind::File]);
        assert!(result.is_err());
    });
}

#[test]
fn resolve_arg_file_before_branch_wins() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("collision", a1_oid);
    test_repo.write_file("collision", "data");
    test_repo.in_dir(|| {
        // File first → file wins
        let result = git::resolve_arg(
            &test_repo.repo,
            "collision",
            &[TargetKind::File, TargetKind::Branch],
        )
        .unwrap();
        assert_eq!(result, Target::File("collision".to_string()));
    });
}

#[test]
fn resolve_arg_branch_before_file_wins() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("collision", a1_oid);
    test_repo.write_file("collision", "data");
    test_repo.in_dir(|| {
        // Branch first → branch wins
        let result = git::resolve_arg(
            &test_repo.repo,
            "collision",
            &[TargetKind::Branch, TargetKind::File],
        )
        .unwrap();
        assert_eq!(result, Target::Branch("collision".to_string()));
    });
}

#[test]
fn resolve_arg_commit_by_hash() {
    let test_repo = TestRepo::new_with_remote();
    let oid = test_repo.commit_empty("A1");
    test_repo.in_dir(|| {
        let result =
            git::resolve_arg(&test_repo.repo, &oid.to_string(), &[TargetKind::Commit]).unwrap();
        assert!(matches!(result, Target::Commit(_)));
    });
}

#[test]
fn resolve_arg_commit_rejects_merge() {
    let test_repo = TestRepo::new_with_remote();
    let upstream_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.commit_empty("A1");
    let a1_oid = test_repo.head_oid();
    // Create a merge commit
    test_repo.commit_merge("Merge side", a1_oid, upstream_oid);
    let merge_oid = test_repo.head_oid();
    test_repo.in_dir(|| {
        let result = git::resolve_arg(
            &test_repo.repo,
            &merge_oid.to_string(),
            &[TargetKind::Commit],
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("merge commit"));
    });
}

#[test]
fn resolve_arg_commit_not_accepted_skips() {
    let test_repo = TestRepo::new_with_remote();
    let oid = test_repo.commit_empty("A1");
    test_repo.in_dir(|| {
        // Only accept File — commit hash should not match
        let result = git::resolve_arg(&test_repo.repo, &oid.to_string(), &[TargetKind::File]);
        assert!(result.is_err());
    });
}

#[test]
fn resolve_arg_branch_by_shortid() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    let a1_oid = test_repo.head_oid();
    test_repo.create_branch_at_commit("feature-a", a1_oid);
    test_repo.in_dir(|| {
        let info = git::gather_repo_info(&test_repo.repo, false, 1).unwrap();
        let entities = info.collect_entities();
        let alloc = crate::shortid::IdAllocator::new(entities);
        let sid = alloc.get_branch("feature-a");
        let result = git::resolve_arg(&test_repo.repo, &sid, &[TargetKind::Branch]).unwrap();
        assert_eq!(result, Target::Branch("feature-a".to_string()));
    });
}

#[test]
fn resolve_arg_commit_by_shortid() {
    let test_repo = TestRepo::new_with_remote();
    let oid = test_repo.commit_empty("A1");
    test_repo.in_dir(|| {
        let info = git::gather_repo_info(&test_repo.repo, false, 1).unwrap();
        let entities = info.collect_entities();
        let alloc = crate::shortid::IdAllocator::new(entities);
        let sid = alloc.get_commit(oid);
        let result = git::resolve_arg(&test_repo.repo, &sid, &[TargetKind::Commit]).unwrap();
        assert!(matches!(result, Target::Commit(_)));
    });
}

#[test]
fn resolve_arg_unstaged() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    test_repo.in_dir(|| {
        let result = git::resolve_arg(&test_repo.repo, "zz", &[TargetKind::Unstaged]).unwrap();
        assert_eq!(result, Target::Unstaged);
    });
}

#[test]
fn resolve_arg_unstaged_not_accepted() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    test_repo.in_dir(|| {
        let result = git::resolve_arg(&test_repo.repo, "zz", &[TargetKind::Commit]);
        assert!(result.is_err());
    });
}
