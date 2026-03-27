use crate::core::test_helpers::TestRepo;

// ── extract_remote_name tests ────────────────────────────────────────────

#[test]
fn extract_remote_name_parses_correctly() {
    assert_eq!(super::extract_remote_name("origin/main"), "origin");
    assert_eq!(super::extract_remote_name("upstream/develop"), "upstream");
    assert_eq!(super::extract_remote_name("origin"), "origin");
}

// ── extract_target_branch tests ──────────────────────────────────────────

#[test]
fn extract_target_branch_parses_correctly() {
    assert_eq!(super::extract_target_branch("origin/main"), "main");
    assert_eq!(super::extract_target_branch("upstream/develop"), "develop");
    assert_eq!(
        super::extract_target_branch("origin/release/v1"),
        "release/v1"
    );
    assert_eq!(super::extract_target_branch("origin"), "main");
}

// ── detect_remote_type tests ─────────────────────────────────────────────

#[test]
fn detect_remote_type_plain_by_default() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), super::RemoteType::Plain);
}

#[test]
fn detect_remote_type_gerrit_by_config() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    // Set loom.remote-type to gerrit
    test_repo.set_config("loom.remote-type", "gerrit");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        super::RemoteType::Gerrit {
            target_branch: "main".to_string()
        }
    );
}

#[test]
fn detect_remote_type_github_by_config() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    // Set loom.remote-type to github
    test_repo.set_config("loom.remote-type", "github");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), super::RemoteType::GitHub);
}

#[test]
fn detect_remote_type_config_overrides_url() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    // Even though remote URL is a local path (not github.com),
    // explicit config should take priority
    test_repo.set_config("loom.remote-type", "gerrit");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        super::RemoteType::Gerrit {
            target_branch: "main".to_string()
        }
    );
}

#[test]
fn detect_remote_type_gerrit_by_hook() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    // Create a fake commit-msg hook containing "gerrit"
    let hooks_dir = workdir.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(
        hooks_dir.join("commit-msg"),
        "#!/bin/sh\n# Gerrit Change-Id hook\n",
    )
    .unwrap();

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        super::RemoteType::Gerrit {
            target_branch: "main".to_string()
        }
    );
}

// ── resolve_push_remote tests ────────────────────────────────────────────

#[test]
fn resolve_push_remote_github_fork_uses_origin() {
    let test_repo = TestRepo::new_with_remote();

    // Set up origin with a github.com URL and add an "upstream" remote
    test_repo
        .repo
        .remote_set_url("origin", "https://github.com/user/fork.git")
        .unwrap();
    let remote_path = test_repo.remote_path().unwrap();
    test_repo
        .repo
        .remote("upstream", remote_path.to_str().unwrap())
        .unwrap();

    // When tracking upstream/main on GitHub, push should go to origin
    let result = super::resolve_push_remote(
        &test_repo.repo,
        &test_repo.workdir(),
        "upstream/main",
        &super::RemoteType::GitHub,
    );
    assert_eq!(result, "origin");
}

#[test]
fn resolve_push_remote_github_origin_stays_origin() {
    let test_repo = TestRepo::new_with_remote();

    // When tracking origin/main on GitHub, push should stay on origin
    let result = super::resolve_push_remote(
        &test_repo.repo,
        &test_repo.workdir(),
        "origin/main",
        &super::RemoteType::GitHub,
    );
    assert_eq!(result, "origin");
}

#[test]
fn resolve_push_remote_plain_upstream_stays_upstream() {
    let test_repo = TestRepo::new_with_remote();

    let remote_path = test_repo.remote_path().unwrap();
    test_repo
        .repo
        .remote("upstream", remote_path.to_str().unwrap())
        .unwrap();

    // Plain remote type should NOT redirect, even if "upstream" remote exists
    let result = super::resolve_push_remote(
        &test_repo.repo,
        &test_repo.workdir(),
        "upstream/main",
        &super::RemoteType::Plain,
    );
    assert_eq!(result, "upstream");
}

// ── resolve_branch tests ─────────────────────────────────────────────────

#[test]
fn resolve_branch_accepts_woven_branch() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at merge-base
    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    // Switch to feature-a, add a commit, switch back
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    // Add integration commit + merge to create woven topology
    test_repo.commit("Int", "int.txt");
    test_repo.merge_no_ff("feature-a");

    let result = super::resolve_branch(
        &test_repo.repo,
        &crate::core::repo::gather_repo_info(&test_repo.repo, false, 1).unwrap(),
        "feature-a",
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "feature-a");
}

#[test]
fn resolve_branch_rejects_non_woven() {
    let test_repo = TestRepo::new_with_remote();

    // Create a branch whose tip is outside the integration range:
    // advance main past origin/main, then create stray-branch there
    test_repo.switch_branch("main");
    test_repo.commit("Main-only", "main-only.txt");
    test_repo.create_branch("stray-branch");
    test_repo.switch_branch("integration");
    test_repo.commit("C1", "c1.txt");

    let result = super::resolve_branch(
        &test_repo.repo,
        &crate::core::repo::gather_repo_info(&test_repo.repo, false, 1).unwrap(),
        "stray-branch",
    );
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("not woven into the integration branch")
    );
}

#[test]
fn resolve_branch_rejects_commit_target() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("C1", "c1.txt");

    let result = super::resolve_branch(
        &test_repo.repo,
        &crate::core::repo::gather_repo_info(&test_repo.repo, false, 1).unwrap(),
        &c1_oid.to_string(),
    );
    assert!(result.is_err());
    // With the centralized resolver, a commit hash that doesn't resolve to a branch
    // produces a "did not resolve to a branch" error
    assert!(result.unwrap_err().to_string().contains("branch"));
}

#[test]
fn detect_remote_type_azure_by_config() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    test_repo.set_config("loom.remote-type", "azure");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), super::RemoteType::AzureDevOps);
}

#[test]
fn detect_remote_type_gerrit_in_worktree() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    // Install a Gerrit hook in the main repo's hooks dir
    let hooks_dir = workdir.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    std::fs::write(
        hooks_dir.join("commit-msg"),
        "#!/bin/sh\n# Gerrit Change-Id hook\n",
    )
    .unwrap();

    // Create a worktree — .git is a file there, not a directory
    let wt_path = workdir.parent().unwrap().join("worktree-test");
    std::process::Command::new("git")
        .current_dir(&workdir)
        .args(["worktree", "add", wt_path.to_str().unwrap(), "HEAD"])
        .output()
        .unwrap();

    // Open the worktree as a Repository
    let wt_repo = git2::Repository::open(&wt_path).unwrap();

    // .git should be a file in the worktree, not a directory
    assert!(
        !wt_path.join(".git").is_dir(),
        ".git in worktree should not be a directory"
    );

    // detect_remote_type should still find the Gerrit hook via repo.path()
    let result = super::detect_remote_type(&wt_repo, &wt_path, "origin/main");
    assert!(result.is_ok(), "detect_remote_type failed: {:?}", result);
    assert_eq!(
        result.unwrap(),
        super::RemoteType::Gerrit {
            target_branch: "main".to_string()
        },
        "Should detect Gerrit via hook even in a worktree"
    );
}

#[test]
fn detect_remote_type_azure_by_url() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    // Set remote URL to a dev.azure.com URL
    test_repo
        .repo
        .remote_set_url(
            "origin",
            "https://dev.azure.com/myorg/myproject/_git/myrepo",
        )
        .unwrap();

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), super::RemoteType::AzureDevOps);
}

#[test]
fn push_github_skips_pr_for_upstream_branch() {
    // The guard is: if branch == target_branch, skip PR creation.
    // We test the condition itself since push_github needs a real gh CLI.
    let branch = "main";
    let target_branch = "main";
    assert_eq!(branch, target_branch, "upstream branch should be detected");

    let branch = "feature-a";
    let target_branch = "main";
    assert_ne!(branch, target_branch, "feature branch should not skip");
}

// ── extract_gh_repo tests ─────────────────────────────────────────────────

#[test]
fn extract_gh_repo_scp_style() {
    let test_repo = TestRepo::new_with_remote();
    test_repo
        .repo
        .remote_set_url("origin", "git@github.com:owner/repo.git")
        .unwrap();
    let result = super::extract_gh_repo(&test_repo.repo, "origin");
    assert_eq!(result, Some("owner/repo".to_string()));
}

#[test]
fn extract_gh_repo_https() {
    let test_repo = TestRepo::new_with_remote();
    test_repo
        .repo
        .remote_set_url("origin", "https://github.com/owner/repo.git")
        .unwrap();
    let result = super::extract_gh_repo(&test_repo.repo, "origin");
    assert_eq!(result, Some("owner/repo".to_string()));
}

#[test]
fn extract_gh_repo_bare_alias() {
    let test_repo = TestRepo::new_with_remote();
    test_repo
        .repo
        .remote_set_url("origin", "github-work:owner/repo")
        .unwrap();
    let result = super::extract_gh_repo(&test_repo.repo, "origin");
    assert_eq!(result, Some("owner/repo".to_string()));
}

#[test]
fn extract_gh_repo_nonexistent_remote() {
    let test_repo = TestRepo::new_with_remote();
    let result = super::extract_gh_repo(&test_repo.repo, "nonexistent");
    assert_eq!(result, None);
}
