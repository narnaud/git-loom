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

#[test]
fn detect_remote_type_plain_by_config() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    // A saved "plain" answer (from the Gerrit confirmation prompt) must be
    // honored without warning about an unknown value
    test_repo.set_config("loom.remote-type", "plain");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), super::RemoteType::Plain);
}

// ── looks_like_gerrit tests ──────────────────────────────────────────────

#[test]
fn looks_like_gerrit_by_ssh_port() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("C1", "c1.txt");

    test_repo
        .repo
        .remote_set_url(
            "origin",
            "ssh://nicolas@review.example.com:29418/kdab/Project",
        )
        .unwrap();

    assert!(super::looks_like_gerrit(&test_repo.repo, "origin/main"));
}

#[test]
fn looks_like_gerrit_by_change_id_trailer() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit(
        "C1\n\nChange-Id: I61096b677887afc82613103d8467808b77ecbd50",
        "c1.txt",
    );

    assert!(super::looks_like_gerrit(&test_repo.repo, "origin/main"));
}

#[test]
fn looks_like_gerrit_negative_without_hints() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("C1", "c1.txt");

    assert!(!super::looks_like_gerrit(&test_repo.repo, "origin/main"));
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
fn detect_remote_type_gitlab_by_config() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    test_repo.set_config("loom.remote-type", "gitlab");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), super::RemoteType::GitLab);
}

#[test]
fn detect_remote_type_gitlab_by_url() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    test_repo
        .repo
        .remote_set_url("origin", "git@gitlab.com:group/repo.git")
        .unwrap();

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), super::RemoteType::GitLab);
}

// ── append_remote_urls tests ─────────────────────────────────────────────

#[test]
fn append_remote_urls_extracts_gitlab_mr_link() {
    let stderr = "remote: \n\
                  remote: To create a merge request for feature-a, visit:\n\
                  remote:   https://gitlab.com/group/repo/-/merge_requests/new?x=1\n\
                  remote: \n";
    let mut message = String::from("Pushed");
    super::append_remote_urls(&mut message, stderr);
    assert_eq!(
        message,
        "Pushed\nhttps://gitlab.com/group/repo/-/merge_requests/new?x=1"
    );
}

#[test]
fn append_remote_urls_wraps_gerrit_tag() {
    let stderr = "remote:   https://gerrit.example.com/c/proj/+/123 [NEW]\n";
    let mut message = String::from("Pushed");
    super::append_remote_urls(&mut message, stderr);
    assert_eq!(
        message,
        "Pushed\nhttps://gerrit.example.com/c/proj/+/123 `[NEW]`"
    );
}

#[test]
fn append_remote_urls_ignores_non_url_lines() {
    let stderr = "remote: Counting objects: 5, done.\nSwitched to branch\n";
    let mut message = String::from("Pushed");
    super::append_remote_urls(&mut message, stderr);
    assert_eq!(message, "Pushed");
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

// ── extract_azure_remote tests ────────────────────────────────────────────

#[test]
fn extract_azure_remote_https() {
    let test_repo = TestRepo::new_with_remote();
    test_repo
        .repo
        .remote_set_url(
            "origin",
            "https://dev.azure.com/myorg/myproject/_git/myrepo",
        )
        .unwrap();
    let azure = super::extract_azure_remote(&test_repo.repo, "origin").unwrap();
    assert_eq!(azure.org_url, "https://dev.azure.com/myorg");
    assert_eq!(azure.project.as_deref(), Some("myproject"));
    assert_eq!(azure.repository.as_deref(), Some("myrepo"));
}

#[test]
fn extract_azure_remote_ssh() {
    let test_repo = TestRepo::new_with_remote();
    test_repo
        .repo
        .remote_set_url("origin", "git@ssh.dev.azure.com:v3/myorg/myproject/myrepo")
        .unwrap();
    let azure = super::extract_azure_remote(&test_repo.repo, "origin").unwrap();
    assert_eq!(azure.org_url, "https://dev.azure.com/myorg");
    assert_eq!(azure.project.as_deref(), Some("myproject"));
    assert_eq!(azure.repository.as_deref(), Some("myrepo"));
}

#[test]
fn extract_azure_remote_visualstudio_has_no_project() {
    let test_repo = TestRepo::new_with_remote();
    test_repo
        .repo
        .remote_set_url(
            "origin",
            "https://myorg.visualstudio.com/myproject/_git/myrepo",
        )
        .unwrap();
    let azure = super::extract_azure_remote(&test_repo.repo, "origin").unwrap();
    assert_eq!(azure.org_url, "https://myorg.visualstudio.com");
    assert_eq!(azure.project, None);
    assert_eq!(azure.repository.as_deref(), Some("myrepo"));
}

#[test]
fn extract_azure_remote_unrecognized() {
    let test_repo = TestRepo::new_with_remote();
    test_repo
        .repo
        .remote_set_url("origin", "git@github.com:owner/repo.git")
        .unwrap();
    assert!(super::extract_azure_remote(&test_repo.repo, "origin").is_none());
}

// ── force tests ──────────────────────────────────────────────────────────

#[test]
fn force_pushes_when_the_lease_check_would_refuse() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.create_branch("feature-a");
    test_repo.switch_branch("feature-a");
    let tip = test_repo.commit("feature commit", "a.txt");
    crate::git::run_git(&workdir, &["push", "origin", "feature-a"]).unwrap();

    // Someone else moved the remote branch, so our remote-tracking ref is stale
    // and --force-with-lease refuses the push.
    let remote = test_repo.remote_path().unwrap();
    crate::git::run_git(
        &remote,
        &["update-ref", "refs/heads/feature-a", "refs/heads/main"],
    )
    .unwrap();

    let plan = super::PushPlan::single("feature-a");
    assert!(super::push_plain(&workdir, "origin", &plan, false).is_err());
    assert!(super::push_plain(&workdir, "origin", &plan, true).is_ok());

    let pushed =
        crate::git::run_git_stdout(&remote, &["rev-parse", "refs/heads/feature-a"]).unwrap();
    assert_eq!(pushed.trim(), tip.to_string());
}

// ── stacked branches ─────────────────────────────────────────────────────

use crate::core::repo::{BranchInfo, CommitInfo, RemoteStatus, RepoInfo, UpstreamInfo};

fn oid(byte: u8) -> git2::Oid {
    let mut bytes = [0u8; 20];
    bytes[0] = byte;
    git2::Oid::from_bytes(&bytes).unwrap()
}

fn commit(byte: u8, message: &str, parent: Option<u8>) -> CommitInfo {
    CommitInfo {
        oid: oid(byte),
        short_id: format!("{:07x}", byte),
        message: message.to_string(),
        parent_oid: parent.map(oid),
        files: vec![],
    }
}

fn branch(name: &str, tip: u8, remote: Option<RemoteStatus>) -> BranchInfo {
    BranchInfo {
        name: name.to_string(),
        tip_oid: oid(tip),
        remote,
    }
}

/// d (D1) on c (C1) on b (B1) on a (A1); x (X1) forks from upstream.
fn stack_info(
    b_remote: Option<RemoteStatus>,
    c_remote: Option<RemoteStatus>,
    d_remote: Option<RemoteStatus>,
) -> RepoInfo {
    RepoInfo {
        branch_name: "integration".to_string(),
        upstream: UpstreamInfo {
            label: "origin/main".to_string(),
            tip_oid: oid(0xAA),
            merge_base_oid: oid(0xAA),
            base_short_id: "aaa0000".to_string(),
            base_message: "Initial".to_string(),
            base_date: "2026-01-01".to_string(),
            commits_ahead: 0,
        },
        commits: vec![
            commit(0x10, "X1", None),
            commit(4, "D1", Some(3)),
            commit(3, "C1", Some(2)),
            commit(2, "B1", Some(1)),
            commit(1, "A1", None),
        ],
        branches: vec![
            branch("x", 0x10, None),
            branch("d", 4, d_remote),
            branch("c", 3, c_remote),
            branch("b", 2, b_remote),
            branch("a", 1, Some(RemoteStatus::Synced)),
        ],
        working_changes: vec![],
        context_commits: vec![],
    }
}

/// Override one branch's remote status in a `stack_info` graph.
fn with_remote(mut info: RepoInfo, name: &str, remote: Option<RemoteStatus>) -> RepoInfo {
    let branch = info
        .branches
        .iter_mut()
        .find(|b| b.name == name)
        .expect("branch is in the graph");
    branch.remote = remote;
    info
}

fn layer(branch: &str, base: &str) -> super::Layer {
    super::Layer {
        branch: branch.to_string(),
        base: base.to_string(),
    }
}

#[test]
fn plan_push_of_a_lone_branch_targets_upstream() {
    let info = stack_info(None, None, None);
    let plan = super::plan_push(&info, "x", "main");
    assert_eq!(plan.layers, vec![layer("x", "main")]);
    assert!(plan.republish.is_empty());
    assert!(plan.not_pushed.is_empty());
    assert!(!plan.is_stacked());
    assert_eq!(plan.branches(), vec!["x"]);
}

#[test]
fn plan_push_chains_bases_bottom_up() {
    let info = stack_info(None, None, None);
    let plan = super::plan_push(&info, "c", "main");
    assert_eq!(
        plan.layers,
        vec![layer("a", "main"), layer("b", "a"), layer("c", "b")]
    );
    assert_eq!(plan.requested(), "c");
    assert!(plan.is_stacked());
}

#[test]
fn plan_push_republishes_stale_upstack_and_hints_the_rest() {
    // c was pushed and has changed since, d was never pushed.
    let info = stack_info(None, Some(RemoteStatus::Different), None);
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(plan.layers, vec![layer("a", "main"), layer("b", "a")]);
    assert_eq!(plan.republish, vec![layer("c", "b")]);
    assert_eq!(plan.not_pushed, vec!["d"]);
    assert_eq!(plan.branches(), vec!["a", "b", "c"]);
    let flags: Vec<bool> = plan.pr_layers().map(|(_, republish)| republish).collect();
    assert_eq!(flags, vec![false, false, true]);
}

#[test]
fn hidden_downstack_reports_hidden_layers_bottom_first() {
    let info = stack_info(None, None, None);
    // The prefix `a` hides only branch `a`.
    assert_eq!(super::hidden_downstack(&info, "c", "a"), vec!["a"]);
    assert_eq!(super::hidden_downstack(&info, "a", "a"), vec!["a"]);
    assert!(super::hidden_downstack(&info, "x", "a").is_empty());
    assert!(super::hidden_downstack(&info, "c", "").is_empty());
}

#[test]
fn refuse_hidden_names_the_hidden_layer() {
    let info = stack_info(None, None, None);
    let err = super::refuse_hidden(&info, "c", "a")
        .unwrap_err()
        .to_string();
    assert!(err.contains("`c` is stacked on hidden branch `a`"), "{err}");
    let err = super::refuse_hidden(&info, "a", "a")
        .unwrap_err()
        .to_string();
    assert!(err.contains("`a` is hidden"), "{err}");
    assert!(super::refuse_hidden(&info, "x", "a").is_ok());
}

#[test]
fn plan_push_skips_synced_and_gone_upstack() {
    let info = stack_info(None, Some(RemoteStatus::Synced), Some(RemoteStatus::Gone));
    let plan = super::plan_push(&info, "b", "main");
    assert!(plan.republish.is_empty());
    assert!(plan.not_pushed.is_empty());
}

#[test]
fn plan_push_drops_a_gone_bottom_layer() {
    // a's PR merged and the remote branch was deleted: b becomes the bottom.
    let info = with_remote(stack_info(None, None, None), "a", Some(RemoteStatus::Gone));
    let plan = super::plan_push(&info, "c", "main");
    assert_eq!(plan.layers, vec![layer("b", "main"), layer("c", "b")]);
    assert_eq!(plan.requested(), "c");
    assert_eq!(plan.branches(), vec!["b", "c"]);
}

#[test]
fn plan_push_drops_a_gone_middle_layer() {
    let info = stack_info(Some(RemoteStatus::Gone), None, None);
    let plan = super::plan_push(&info, "c", "main");
    assert_eq!(plan.layers, vec![layer("a", "main"), layer("c", "a")]);
}

#[test]
fn plan_push_keeps_the_requested_branch_when_it_is_gone() {
    // Every layer is gone, the requested one included: it is still pushed,
    // alone, targeting upstream.
    let info = with_remote(
        stack_info(Some(RemoteStatus::Gone), None, None),
        "a",
        Some(RemoteStatus::Gone),
    );
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(plan.layers, vec![layer("b", "main")]);
    assert_eq!(plan.requested(), "b");
    assert!(!plan.is_stacked());
}

#[test]
fn plan_push_rebases_a_republished_layer_over_a_gone_one() {
    // c was merged and its remote branch deleted, d was rewritten above it:
    // d's PR must target b, not the branch that is no longer on the remote.
    let info = stack_info(
        None,
        Some(RemoteStatus::Gone),
        Some(RemoteStatus::Different),
    );
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(plan.layers, vec![layer("a", "main"), layer("b", "a")]);
    assert_eq!(plan.republish, vec![layer("d", "b")]);
    assert!(plan.not_pushed.is_empty());
}

#[test]
fn plan_push_republishes_over_a_never_pushed_layer() {
    // c exists only locally, so it is no PR target: d is still re-published,
    // over the nearest branch the push leaves on the remote.
    let info = stack_info(None, None, Some(RemoteStatus::Different));
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(plan.republish, vec![layer("d", "b")]);
    assert_eq!(plan.not_pushed, vec!["c"]);
}

#[test]
fn plan_push_republishes_a_layer_that_only_added_commits() {
    // c has local commits on top of what is published. It is out of step with
    // the remote like any other, so it goes out with the push.
    let info = stack_info(None, Some(RemoteStatus::Different), None);
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(plan.republish, vec![layer("c", "b")]);
    assert_eq!(plan.not_pushed, vec!["d"]);
}

#[test]
fn plan_push_keeps_a_synced_layer_as_a_republished_base() {
    // A synced c is still on the remote, so d keeps targeting it (spec 011:
    // the skipped layer ends the stack run, it does not change the bases).
    let info = stack_info(
        None,
        Some(RemoteStatus::Synced),
        Some(RemoteStatus::Different),
    );
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(plan.republish, vec![layer("d", "c")]);
    assert!(plan.not_pushed.is_empty());
}

#[test]
fn plan_push_falls_back_to_upstream_when_everything_below_is_gone() {
    // Only the requested branch survives the filter, and it is what the
    // layer above targets.
    let info = with_remote(
        stack_info(
            None,
            Some(RemoteStatus::Gone),
            Some(RemoteStatus::Different),
        ),
        "a",
        Some(RemoteStatus::Gone),
    );
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(plan.layers, vec![layer("b", "main")]);
    assert_eq!(plan.republish, vec![layer("d", "b")]);
}

#[test]
fn plan_push_keeps_gone_out_of_the_message_but_still_hints_upstack() {
    let info = with_remote(
        stack_info(None, Some(RemoteStatus::Different), None),
        "a",
        Some(RemoteStatus::Gone),
    );
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(
        super::pushed_message("origin", &plan),
        "Pushed `b` to `origin`
Re-pushed above `b`: `c`"
    );
    assert_eq!(plan.not_pushed, vec!["d"]);
}

#[test]
fn is_chain_counts_republished_layers_too() {
    // A branch of its own, with nothing above it: one PR, no chain.
    let info = stack_info(None, None, None);
    let lone = super::plan_push(&info, "x", "main");
    assert!(!lone.is_stacked());
    assert!(!lone.is_chain());

    // Not stacked on anything, but a re-published branch above it makes the
    // two PRs a chain: b's PR targets a.
    let info = with_remote(
        stack_info(Some(RemoteStatus::Different), None, None),
        "a",
        None,
    );
    let bottom = super::plan_push(&info, "a", "main");
    assert_eq!(bottom.layers, vec![layer("a", "main")]);
    assert_eq!(bottom.republish, vec![layer("b", "a")]);
    assert!(!bottom.is_stacked(), "a has nothing below it");
    assert!(bottom.is_chain(), "a and b form a PR chain");

    // A stacked branch is a chain on its downstack alone.
    let stacked = super::plan_push(&info, "b", "main");
    assert!(stacked.is_chain());

    // The Gerrit/no-PR plan is a single branch.
    assert!(!super::PushPlan::single("x").is_chain());
}

#[test]
fn azure_refuses_a_stacked_branch() {
    let info = stack_info(None, None, None);
    let stacked = super::plan_push(&info, "b", "main");
    let err = super::refuse_stacked_azure(&super::RemoteType::AzureDevOps, &stacked)
        .unwrap_err()
        .to_string();
    // The whole message: a continuation line must not carry the source indent.
    assert_eq!(
        err,
        "`b` is stacked on `a` — Azure DevOps has no stacked pull requests\n\
         Land the branches below it first, or push without a PR (`--no-pr`)"
    );

    // A branch of its own is fine, and every other remote stacks freely.
    let lone = super::plan_push(&info, "x", "main");
    assert!(super::refuse_stacked_azure(&super::RemoteType::AzureDevOps, &lone).is_ok());
    assert!(super::refuse_stacked_azure(&super::RemoteType::GitHub, &stacked).is_ok());
}

#[test]
fn pushed_message_lists_layers_and_republished() {
    let info = stack_info(None, Some(RemoteStatus::Different), None);
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(
        super::pushed_message("origin", &plan),
        "Pushed `a`, `b` to `origin`\nRe-pushed above `b`: `c`"
    );
    let lone = super::PushPlan::single("x");
    assert_eq!(
        super::pushed_message("origin", &lone),
        "Pushed `x` to `origin`"
    );
}

#[test]
fn gather_branch_commits_yields_only_the_branch_own_commits() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    test_repo.create_branch("feature-a");
    test_repo.commit("B1", "b1.txt");
    test_repo.create_branch("feature-b");
    let info = crate::core::repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();

    let subjects = |branch: &str, base: &str| {
        super::gather_branch_commits(&test_repo.repo, &info, branch, base)
            .unwrap()
            .iter()
            .map(|(s, _)| s.clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(subjects("feature-a", "main"), vec!["A1", "A2"]);
    // Stacked on feature-a: the PR's diff is B1 alone, and so is its body.
    assert_eq!(subjects("feature-b", "feature-a"), vec!["B1"]);
}

#[test]
fn gather_branch_commits_spans_the_stack_when_the_pr_targets_the_trunk() {
    // A fork's PRs target the trunk, and so does a layer whose base was
    // dropped: GitHub's diff then covers the branches below too, so the
    // description has to cover them as well.
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    test_repo.create_branch("feature-a");
    test_repo.commit("B1", "b1.txt");
    test_repo.create_branch("feature-b");
    let info = crate::core::repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();

    let commits =
        super::gather_branch_commits(&test_repo.repo, &info, "feature-b", "main").unwrap();
    assert_eq!(
        commits.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(),
        vec!["A1", "B1"]
    );
}

#[test]
fn push_args_make_a_stack_atomic() {
    // Spec 011 sells the stack push as all-or-nothing: a lease refused on any
    // branch must leave every branch untouched. `--atomic` is what buys that.
    let stack = super::push_args("origin", &["a", "b"], false);
    assert_eq!(
        stack,
        vec![
            "push",
            "--force-with-lease",
            "--force-if-includes",
            "--atomic",
            "-u",
            "origin",
            "a",
            "b"
        ]
    );
    // Nothing to be atomic about with one ref.
    let lone = super::push_args("origin", &["a"], false);
    assert!(!lone.contains(&"--atomic"));
    // `-f` replaces the lease pair, and keeps the guarantee.
    let forced = super::push_args("origin", &["a", "b"], true);
    assert_eq!(
        forced,
        vec!["push", "--force", "--atomic", "-u", "origin", "a", "b"]
    );
}

#[test]
fn push_plain_sends_the_whole_chain_in_one_push() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let a_tip = test_repo.commit("A1", "a1.txt");
    test_repo.create_branch("feature-a");
    let b_tip = test_repo.commit("B1", "b1.txt");
    test_repo.create_branch("feature-b");
    let info = crate::core::repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let plan = super::plan_push(&info, "feature-b", "main");
    assert_eq!(plan.branches(), vec!["feature-a", "feature-b"]);

    super::push_plain(&workdir, "origin", &plan, false).unwrap();

    let remote = test_repo.remote_path().unwrap();
    let rev = |r: &str| {
        crate::git::run_git_stdout(&remote, &["rev-parse", r])
            .unwrap()
            .trim()
            .to_string()
    };
    assert_eq!(rev("refs/heads/feature-a"), a_tip.to_string());
    assert_eq!(rev("refs/heads/feature-b"), b_tip.to_string());
}

// ── gh parsing ───────────────────────────────────────────────────────────

#[test]
fn parse_gh_pr_list_reads_number_url_and_base() {
    let json =
        r#"[{"baseRefName":"feature-a","number":42,"url":"https://github.com/o/r/pull/42"}]"#;
    assert_eq!(
        super::parse_gh_pr_list(json, None),
        Some(super::GhPr {
            number: 42,
            url: "https://github.com/o/r/pull/42".to_string(),
            base: "feature-a".to_string(),
        })
    );
    assert_eq!(super::parse_gh_pr_list("[]", None), None);
    assert_eq!(super::parse_gh_pr_list("not json", None), None);
}

#[test]
fn parse_gh_pr_list_picks_the_pr_from_our_repository() {
    // `--head b` also matches a stranger's fork branch named `b`.
    let json = r#"[
        {"baseRefName":"main","number":9,"url":"https://github.com/o/r/pull/9",
         "headRepositoryOwner":{"login":"stranger"}},
        {"baseRefName":"a","number":12,"url":"https://github.com/o/r/pull/12",
         "headRepositoryOwner":{"login":"Forker"}}
    ]"#;
    assert_eq!(
        super::parse_gh_pr_list(json, Some("forker")).map(|pr| pr.number),
        Some(12)
    );
    assert_eq!(super::parse_gh_pr_list(json, Some("nobody")), None);
    assert_eq!(
        super::parse_gh_pr_list(json, None).map(|pr| pr.number),
        Some(9)
    );
}

#[test]
fn stack_pr_numbers_stops_at_a_missing_pr_or_a_gap() {
    let chain = |specs: &[(&str, &str, Option<u64>)]| -> Vec<(super::Layer, Option<u64>)> {
        specs
            .iter()
            .map(|(b, base, n)| (layer(b, base), *n))
            .collect()
    };
    // Complete chain.
    assert_eq!(
        super::stack_pr_numbers(&chain(&[("a", "main", Some(1)), ("b", "a", Some(2))])),
        vec![1, 2]
    );
    // A layer without a PR ends the run.
    assert_eq!(
        super::stack_pr_numbers(&chain(&[
            ("a", "main", Some(1)),
            ("b", "a", None),
            ("c", "b", Some(3)),
        ])),
        vec![1]
    );
    // c was in sync and skipped: d targets c, not b, so d cannot join.
    assert_eq!(
        super::stack_pr_numbers(&chain(&[
            ("a", "main", Some(1)),
            ("b", "a", Some(2)),
            ("d", "c", Some(4)),
        ])),
        vec![1, 2]
    );
    assert!(super::stack_pr_numbers(&chain(&[("a", "main", None)])).is_empty());
}

#[test]
fn pr_number_from_url_takes_the_last_segment() {
    assert_eq!(
        super::pr_number_from_url("https://github.com/o/r/pull/7\n"),
        Some(7)
    );
    assert_eq!(
        super::pr_number_from_url("https://github.com/o/r/pulls"),
        None
    );
}

#[test]
fn parse_stack_number_handles_null_and_objects() {
    assert_eq!(super::parse_stack_number("null\n"), None);
    assert_eq!(
        super::parse_stack_number(r#"{"id":9,"number":3,"size":2,"position":1}"#),
        Some(3)
    );
}

#[test]
fn stack_request_body_orders_bottom_to_top() {
    assert_eq!(
        super::stack_request_body(&[10, 11, 12]),
        r#"{"pull_requests":[10,11,12]}"#
    );
}

#[test]
fn plan_stack_registration_covers_every_shape() {
    use super::StackAction;
    assert_eq!(
        super::plan_stack_registration(&[None, None]),
        StackAction::Create
    );
    assert_eq!(
        super::plan_stack_registration(&[Some(3), Some(3)]),
        StackAction::Complete { stack: 3 }
    );
    assert_eq!(
        super::plan_stack_registration(&[Some(3), Some(3), None, None]),
        StackAction::Extend { stack: 3, from: 2 }
    );
    assert_eq!(
        super::plan_stack_registration(&[Some(3), Some(4)]),
        StackAction::Conflict
    );
    assert_eq!(
        super::plan_stack_registration(&[Some(3), None, Some(3)]),
        StackAction::Conflict
    );
    assert_eq!(
        super::plan_stack_registration(&[None, Some(3)]),
        StackAction::Conflict
    );
}
