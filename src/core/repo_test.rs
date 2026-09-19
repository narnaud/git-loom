use crate::core::repo::{
    self, RemoteStatus, Target, TargetKind, gather_repo_info, get_working_changes,
    get_working_changes_recurse,
};
use crate::core::test_helpers::TestRepo;

// ── Tests ──────────────────────────────────────────────────────────────

/// Point `refs/remotes/origin/<name>` at `oid` and make local `name` track it.
fn publish_branch_at(test_repo: &TestRepo, name: &str, oid: git2::Oid) {
    test_repo
        .repo
        .reference(&format!("refs/remotes/origin/{}", name), oid, true, "test")
        .unwrap();
    test_repo
        .repo
        .find_branch(name, git2::BranchType::Local)
        .unwrap()
        .set_upstream(Some(&format!("origin/{}", name)))
        .unwrap();
}

fn remote_status_of(test_repo: &TestRepo, name: &str) -> Option<RemoteStatus> {
    let branch = test_repo
        .repo
        .find_branch(name, git2::BranchType::Local)
        .unwrap();
    let tip = branch.get().target().unwrap();
    repo::detect_remote_status(&test_repo.repo, &branch, name, tip)
}

#[test]
fn remote_status_is_synced_when_the_tips_match() {
    let test_repo = TestRepo::new_with_remote();
    let tip = test_repo.commit("A1", "a.txt");
    test_repo.create_branch("feat");
    publish_branch_at(&test_repo, "feat", tip);
    assert!(matches!(
        remote_status_of(&test_repo, "feat"),
        Some(RemoteStatus::Synced)
    ));
}

#[test]
fn remote_status_is_different_when_only_new_commits_were_added() {
    let test_repo = TestRepo::new_with_remote();
    let published = test_repo.commit("A1", "a.txt");
    test_repo.commit("A2", "a2.txt");
    test_repo.create_branch("feat");
    // The published tip is still in the branch's history, and it still counts
    // as different: the remote does not have what the branch has.
    publish_branch_at(&test_repo, "feat", published);
    assert!(matches!(
        remote_status_of(&test_repo, "feat"),
        Some(RemoteStatus::Different)
    ));
}

#[test]
fn remote_status_is_different_when_the_published_tip_was_rewritten() {
    let test_repo = TestRepo::new_with_remote();
    let rewritten = test_repo.commit("A1", "a.txt");
    test_repo.reset_hard(test_repo.get_oid(1));
    let local = test_repo.commit("A1 amended", "a.txt");
    test_repo.create_branch("feat");
    assert_ne!(local, rewritten);
    // The published tip is not even an ancestor any more.
    publish_branch_at(&test_repo, "feat", rewritten);
    assert!(matches!(
        remote_status_of(&test_repo, "feat"),
        Some(RemoteStatus::Different)
    ));
}

#[test]
fn remote_status_is_gone_when_the_remote_ref_was_deleted() {
    let test_repo = TestRepo::new_with_remote();
    let tip = test_repo.commit("A1", "a.txt");
    test_repo.create_branch("feat");
    publish_branch_at(&test_repo, "feat", tip);
    test_repo
        .repo
        .find_reference("refs/remotes/origin/feat")
        .unwrap()
        .delete()
        .unwrap();
    assert!(matches!(
        remote_status_of(&test_repo, "feat"),
        Some(RemoteStatus::Gone)
    ));
}

#[test]
fn remote_status_is_none_when_the_branch_was_never_pushed() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a.txt");
    test_repo.create_branch("feat");
    assert!(remote_status_of(&test_repo, "feat").is_none());
}

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
fn recurse_untracked_subdirs() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("base", "tracked.txt");

    // Create untracked files in a subdirectory.
    let subdir = test_repo.workdir().join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(subdir.join("a.txt"), "aaa").unwrap();
    std::fs::write(subdir.join("b.txt"), "bbb").unwrap();

    // Non-recursive: directory appears as a single entry.
    let flat = get_working_changes(&test_repo.repo).unwrap();
    let flat_paths: Vec<&str> = flat.iter().map(|c| c.path.as_str()).collect();
    assert!(
        flat_paths.contains(&"subdir/") || flat_paths.len() == 1,
        "expected collapsed dir, got: {:?}",
        flat_paths,
    );

    // Recursive: individual files appear.
    let deep = get_working_changes_recurse(&test_repo.repo).unwrap();
    let deep_paths: Vec<&str> = deep.iter().map(|c| c.path.as_str()).collect();
    assert!(
        deep_paths.contains(&"subdir/a.txt"),
        "expected subdir/a.txt, got: {:?}",
        deep_paths,
    );
    assert!(
        deep_paths.contains(&"subdir/b.txt"),
        "expected subdir/b.txt, got: {:?}",
        deep_paths,
    );
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
        crate::core::repo::resolve_arg(
            &test_repo.repo,
            &head_oid.to_string(),
            &[crate::core::repo::TargetKind::Commit],
        )
    });

    assert!(result.is_ok());
    match result.unwrap() {
        crate::core::repo::Target::Commit(hash) => assert_eq!(hash, head_oid.to_string()),
        crate::core::repo::Target::Branch(_) => panic!("Expected Commit, got Branch"),
        crate::core::repo::Target::File(_) => panic!("Expected Commit, got File"),
        crate::core::repo::Target::Unstaged => panic!("Expected Commit, got Unstaged"),
        crate::core::repo::Target::CommitFile { .. } => panic!("Expected Commit, got CommitFile"),
    }
}

#[test]
fn resolve_partial_commit_hash() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("Second commit");

    let head_oid = test_repo.head_oid();
    let partial_hash = &head_oid.to_string()[..7];
    let result = test_repo.in_dir(|| {
        crate::core::repo::resolve_arg(
            &test_repo.repo,
            partial_hash,
            &[crate::core::repo::TargetKind::Commit],
        )
    });

    assert!(result.is_ok());
    match result.unwrap() {
        crate::core::repo::Target::Commit(hash) => assert_eq!(hash, head_oid.to_string()),
        crate::core::repo::Target::Branch(_) => panic!("Expected Commit, got Branch"),
        crate::core::repo::Target::File(_) => panic!("Expected Commit, got File"),
        crate::core::repo::Target::Unstaged => panic!("Expected Commit, got Unstaged"),
        crate::core::repo::Target::CommitFile { .. } => panic!("Expected Commit, got CommitFile"),
    }
}

#[test]
fn resolve_invalid_target_fails() {
    let test_repo = TestRepo::new_with_remote();

    let result = test_repo.in_dir(|| {
        crate::core::repo::resolve_arg(
            &test_repo.repo,
            "nonexistent",
            &[
                crate::core::repo::TargetKind::Commit,
                crate::core::repo::TargetKind::Branch,
                crate::core::repo::TargetKind::File,
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
        let result = repo::resolve_arg(&test_repo.repo, "hello.txt", &[TargetKind::File]).unwrap();
        assert_eq!(result, Target::File("hello.txt".to_string()));
    });
}

#[test]
fn resolve_arg_file_normalizes_the_spelling() {
    let test_repo = TestRepo::new_with_remote();
    std::fs::create_dir_all(test_repo.workdir().join("dir")).unwrap();
    test_repo.write_file("dir/hello.txt", "content");
    test_repo.in_dir(|| {
        for (arg, expected) in [
            ("./dir/hello.txt", "dir/hello.txt"),
            ("dir/", "dir"),
            ("./dir", "dir"),
            ("dir/../dir/hello.txt", "dir/hello.txt"),
            (".", "."),
            ("./", "."),
        ] {
            let result = repo::resolve_arg(&test_repo.repo, arg, &[TargetKind::File]).unwrap();
            assert_eq!(result, Target::File(expected.to_string()), "{arg}");
        }
    });
    let sub_dir = test_repo.workdir().join("dir");
    test_repo.in_dir_path(&sub_dir, || {
        for (arg, expected) in [
            (".", "dir"),
            ("..", "."),
            ("../dir/hello.txt", "dir/hello.txt"),
        ] {
            let result = repo::resolve_arg(&test_repo.repo, arg, &[TargetKind::File]).unwrap();
            assert_eq!(result, Target::File(expected.to_string()), "{arg}");
        }
    });
}

#[test]
fn resolve_arg_file_cwd_relative() {
    let test_repo = TestRepo::new_with_remote();
    let sub_dir = test_repo.workdir().join("sub");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(sub_dir.join("deep.txt"), "content").unwrap();
    test_repo.in_dir_path(&sub_dir, || {
        let result = repo::resolve_arg(&test_repo.repo, "deep.txt", &[TargetKind::File]).unwrap();
        assert_eq!(result, Target::File("sub/deep.txt".to_string()));
    });
}

#[test]
fn resolve_arg_file_absolute_path() {
    let test_repo = TestRepo::new_with_remote();
    let sub_dir = test_repo.workdir().join("sub");
    std::fs::create_dir_all(&sub_dir).unwrap();
    let abs = sub_dir.join("deep.txt");
    std::fs::write(&abs, "content").unwrap();
    let arg = abs.to_string_lossy().into_owned();
    test_repo.in_dir(|| {
        let result = repo::resolve_arg(&test_repo.repo, &arg, &[TargetKind::File]).unwrap();
        assert_eq!(result, Target::File("sub/deep.txt".to_string()));
    });
}

#[test]
fn resolve_arg_file_absolute_path_from_subdir() {
    let test_repo = TestRepo::new_with_remote();
    let sub_dir = test_repo.workdir().join("sub");
    std::fs::create_dir_all(&sub_dir).unwrap();
    test_repo.write_file("top.txt", "content");
    let arg = test_repo
        .workdir()
        .join("top.txt")
        .to_string_lossy()
        .into_owned();
    // CWD is a subdir, so the prefix must not be prepended to an absolute path.
    test_repo.in_dir_path(&sub_dir, || {
        let result = repo::resolve_arg(&test_repo.repo, &arg, &[TargetKind::File]).unwrap();
        assert_eq!(result, Target::File("top.txt".to_string()));
    });
}

#[test]
fn resolve_arg_file_absolute_outside_repo_errors() {
    let test_repo = TestRepo::new_with_remote();
    let outside = tempfile::tempdir().unwrap();
    let abs = outside.path().join("stranger.txt");
    std::fs::write(&abs, "content").unwrap();
    let arg = abs.to_string_lossy().into_owned();
    test_repo.in_dir(|| {
        let result = repo::resolve_arg(&test_repo.repo, &arg, &[TargetKind::File]);
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("outside repository"),
            "error should say the path is outside the repo: {msg}"
        );
    });
}

#[test]
fn resolve_arg_file_not_found_errors() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.in_dir(|| {
        let result = repo::resolve_arg(&test_repo.repo, "nope.txt", &[TargetKind::File]);
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
        let result =
            repo::resolve_arg(&test_repo.repo, "feature-a", &[TargetKind::Branch]).unwrap();
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
        let result = repo::resolve_arg(&test_repo.repo, "feature-a", &[TargetKind::File]);
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
        let result = repo::resolve_arg(
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
        let result = repo::resolve_arg(
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
            repo::resolve_arg(&test_repo.repo, &oid.to_string(), &[TargetKind::Commit]).unwrap();
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
        let result = repo::resolve_arg(
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
        let result = repo::resolve_arg(&test_repo.repo, &oid.to_string(), &[TargetKind::File]);
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
        let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
        let entities = info.collect_entities();
        let alloc = crate::core::shortid::IdAllocator::new(entities);
        let sid = alloc.get_branch("feature-a");
        let result = repo::resolve_arg(&test_repo.repo, sid, &[TargetKind::Branch]).unwrap();
        assert_eq!(result, Target::Branch("feature-a".to_string()));
    });
}

#[test]
fn resolve_arg_commit_by_shortid() {
    let test_repo = TestRepo::new_with_remote();
    let oid = test_repo.commit_empty("A1");
    test_repo.in_dir(|| {
        let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
        let entities = info.collect_entities();
        let alloc = crate::core::shortid::IdAllocator::new(entities);
        let sid = alloc.get_commit(oid);
        let result = repo::resolve_arg(&test_repo.repo, sid, &[TargetKind::Commit]).unwrap();
        assert!(matches!(result, Target::Commit(_)));
    });
}

#[test]
fn resolve_arg_unstaged() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    test_repo.in_dir(|| {
        let result = repo::resolve_arg(&test_repo.repo, "zz", &[TargetKind::Unstaged]).unwrap();
        assert_eq!(result, Target::Unstaged);
    });
}

#[test]
fn resolve_arg_unstaged_not_accepted() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    test_repo.in_dir(|| {
        let result = repo::resolve_arg(&test_repo.repo, "zz", &[TargetKind::Commit]);
        assert!(result.is_err());
    });
}

// ── Persistent commit IDs (Spec 002) ─────────────────────────────────────

/// `3ac7…` encodes to `wpns…`, `3ac8…` to `wpnr…`.
const ID_A: &str = "I3ac7000000000000000000000000000000000000";
const ID_B: &str = "I3ac8000000000000000000000000000000000000";

fn resolve(test_repo: &TestRepo, arg: &str, accept: &[TargetKind]) -> anyhow::Result<Target> {
    test_repo.in_dir(|| repo::resolve_arg(&test_repo.repo, arg, accept))
}

#[test]
fn resolve_arg_persistent_id_and_any_longer_prefix() {
    let test_repo = TestRepo::new_with_remote();
    let a = test_repo.commit(&format!("A\n\nChange-Id: {ID_A}\n"), "a.txt");

    for arg in ["wpn", "wpns", "wpnszzzz"] {
        let target = resolve(&test_repo, arg, &[TargetKind::Commit]).unwrap();
        assert_eq!(target, Target::Commit(a.to_string()), "{arg}");
    }
    let err = resolve(&test_repo, "wp", &[TargetKind::Commit]).unwrap_err();
    assert!(err.to_string().contains("did not resolve"), "{err}");
}

#[test]
fn resolve_arg_persistent_prefix_shared_by_two_commits_lists_them() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit(&format!("Older\n\nChange-Id: {ID_A}\n"), "a.txt");
    test_repo.commit(&format!("Newer\n\nChange-Id: {ID_B}\n"), "b.txt");

    let err = resolve(&test_repo, "wpn", &[TargetKind::Commit]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("'wpn' matches several commits"), "{msg}");
    assert!(msg.contains("wpnr ") && msg.contains(" Newer"), "{msg}");
    assert!(msg.contains("wpns ") && msg.contains(" Older"), "{msg}");
}

#[test]
fn resolve_arg_change_id_literal_in_any_case() {
    let test_repo = TestRepo::new_with_remote();
    let a = test_repo.commit(&format!("A\n\nChange-Id: {ID_A}\n"), "a.txt");

    for arg in [ID_A.to_string(), ID_A.to_uppercase()] {
        let target = resolve(&test_repo, &arg, &[TargetKind::Commit]).unwrap();
        assert_eq!(target, Target::Commit(a.to_string()), "{arg}");
    }
}

#[test]
fn resolve_arg_twins_are_ambiguous_by_change_id_but_not_by_hash_id() {
    let test_repo = TestRepo::new_with_remote();
    let first = test_repo.commit(&format!("First\n\nChange-Id: {ID_A}\n"), "a.txt");
    test_repo.commit(&format!("Twin\n\nChange-Id: {ID_A}\n"), "b.txt");

    for arg in [ID_A, "wpn"] {
        let err = resolve(&test_repo, arg, &[TargetKind::Commit]).unwrap_err();
        assert!(
            err.to_string().contains("matches several commits"),
            "{arg}: {err}"
        );
    }
    let hash_id = &first.to_string()[..2];
    let target = resolve(&test_repo, hash_id, &[TargetKind::Commit]).unwrap();
    assert_eq!(target, Target::Commit(first.to_string()));
}

#[test]
fn resolve_arg_persistent_commit_file() {
    let test_repo = TestRepo::new_with_remote();
    let a = test_repo.commit(&format!("A\n\nChange-Id: {ID_A}\n"), "a.txt");

    let target = resolve(&test_repo, "wpn:0", &[TargetKind::CommitFile]).unwrap();
    assert_eq!(
        target,
        Target::CommitFile {
            commit: a.to_string(),
            path: "a.txt".to_string()
        }
    );
    let err = resolve(&test_repo, "wpn:3", &[TargetKind::CommitFile]).unwrap_err();
    assert!(err.to_string().contains("no file at index 3"), "{err}");
}

/// The persistent pass honors the accepted kinds like the exact one.
#[test]
fn resolve_arg_persistent_id_respects_the_accepted_kinds() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit(&format!("A\n\nChange-Id: {ID_A}\n"), "a.txt");

    let err = resolve(&test_repo, "wpn:0", &[TargetKind::Commit]).unwrap_err();
    assert!(err.to_string().contains("did not resolve"), "{err}");
    let err = resolve(&test_repo, "wpn", &[TargetKind::CommitFile]).unwrap_err();
    assert!(err.to_string().contains("did not resolve"), "{err}");
}

/// The prefix pass runs after every exact match: a branch whose exact ID is
/// `wpn` wins over the commits whose letters merely start with it.
#[test]
fn resolve_arg_exact_branch_id_beats_a_commit_prefix() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit(&format!("Older\n\nChange-Id: {ID_A}\n"), "a.txt");
    let tip = test_repo.commit(&format!("Newer\n\nChange-Id: {ID_B}\n"), "b.txt");
    // `wpn` has candidates wp, wn, pn, then wpn; the first three go to
    // their own branches.
    for name in ["wp", "wn", "pn", "wpn"] {
        test_repo.create_branch_at_commit(name, tip);
    }

    let target = resolve(&test_repo, "wpn", &[TargetKind::Commit, TargetKind::Branch]).unwrap();
    assert_eq!(target, Target::Branch("wpn".to_string()));
}

#[test]
fn describe_commit_names_persistent_ids_and_falls_back_to_the_hash() {
    let test_repo = TestRepo::new_with_remote();
    let plain = test_repo.commit("Plain", "p.txt");
    let with_id = test_repo.commit(&format!("Identified\n\nChange-Id: {ID_A}\n"), "a.txt");
    let short = |oid: git2::Oid| oid.to_string()[..7].to_string();

    assert_eq!(
        repo::describe_commit(&test_repo.workdir(), &with_id.to_string()),
        format!("`wpn` ({})", short(with_id))
    );
    assert_eq!(
        repo::describe_commit(&test_repo.workdir(), &plain.to_string()),
        format!("`{}`", short(plain))
    );
    // Outside the graph (no upstream), the hash alone is still reported.
    let bare = TestRepo::new();
    let oid = bare.commit("Alone", "x.txt");
    assert_eq!(
        repo::describe_commit(&bare.workdir(), &oid.to_string()),
        format!("`{}`", short(oid))
    );
}
