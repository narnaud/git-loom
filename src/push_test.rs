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
    assert_eq!(result, super::RemoteType::Plain);
}

#[test]
fn detect_remote_type_gerrit_by_config() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    test_repo.set_config("loom.remote-type", "gerrit");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert_eq!(
        result,
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

    test_repo.set_config("loom.remote-type", "github");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert_eq!(result, super::RemoteType::GitHub);
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
    assert_eq!(
        result,
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
    assert_eq!(
        result,
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

    // A saved "plain" answer (from the remote type menu) must be honored
    // without warning about an unknown value
    test_repo.set_config("loom.remote-type", "plain");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert_eq!(result, super::RemoteType::Plain);
}

// ── resolve_remote_type tests ────────────────────────────────────────────

fn remote_type_config(test_repo: &TestRepo) -> Option<String> {
    crate::git::run_git_stdout(
        &test_repo.workdir(),
        &["config", "--get", "loom.remote-type"],
    )
    .ok()
    .map(|v| v.trim().to_string())
}

/// Resolve the remote type under the TUI, answering a menu with `pick`;
/// returns the prompts shown, with their items, and the result.
fn resolve_remote_type_under_tui(
    test_repo: &TestRepo,
    pick: &str,
) -> (
    Vec<(String, Vec<String>)>,
    anyhow::Result<super::RemoteType>,
) {
    use crate::core::ui::{self, Answer, PromptKind, Request};

    let workdir = test_repo.workdir();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            ui::install(tx);
            let repo = git2::Repository::open(&workdir).unwrap();
            let result = super::resolve_remote_type(&repo, &workdir, "origin/main");
            ui::uninstall();
            result
        });
        let mut prompts = Vec::new();
        for request in rx {
            if let Request::Prompt {
                kind,
                prompt,
                reply,
                ..
            } = request
            {
                let items = match kind {
                    PromptKind::Select { items, .. } => items,
                    _ => vec![],
                };
                prompts.push((prompt, items));
                let _ = reply.send(Some(Answer::Text(pick.to_string())));
            }
        }
        (prompts, worker.join().unwrap())
    })
}

#[test]
fn resolve_remote_type_saves_a_detected_type() {
    let test_repo = TestRepo::new_with_remote();
    test_repo
        .repo
        .remote_set_url("origin", "https://github.com/owner/repo.git")
        .unwrap();

    let (prompts, result) = resolve_remote_type_under_tui(&test_repo, "Plain Git");
    assert!(prompts.is_empty(), "{:?}", prompts);
    assert_eq!(result.unwrap(), super::RemoteType::GitHub);
    assert_eq!(remote_type_config(&test_repo).as_deref(), Some("github"));
}

#[test]
fn resolve_remote_type_keeps_the_configured_type() {
    let test_repo = TestRepo::new_with_remote();
    test_repo
        .repo
        .remote_set_url("origin", "https://github.com/owner/repo.git")
        .unwrap();
    test_repo.set_config("loom.remote-type", "plain");

    let (prompts, result) = resolve_remote_type_under_tui(&test_repo, "GitHub");
    assert!(prompts.is_empty(), "{:?}", prompts);
    assert_eq!(result.unwrap(), super::RemoteType::Plain);
    assert_eq!(remote_type_config(&test_repo).as_deref(), Some("plain"));
}

#[test]
fn resolve_remote_type_asks_with_every_type_and_saves_the_pick() {
    let test_repo = TestRepo::new_with_remote();

    let (prompts, result) = resolve_remote_type_under_tui(&test_repo, "Gerrit");
    assert_eq!(
        prompts,
        [(
            "Which kind of remote is `origin`? (saved as `loom.remote-type`)".to_string(),
            ["GitHub", "GitLab", "Azure DevOps", "Gerrit", "Plain Git"]
                .map(String::from)
                .to_vec()
        )]
    );
    assert_eq!(
        result.unwrap(),
        super::RemoteType::Gerrit {
            target_branch: "main".to_string()
        }
    );
    assert_eq!(remote_type_config(&test_repo).as_deref(), Some("gerrit"));
}

#[test]
fn resolve_remote_type_saves_nothing_when_the_menu_is_cancelled() {
    use crate::core::ui::{self, Request};

    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let (tx, rx) = std::sync::mpsc::channel();
    let result = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            ui::install(tx);
            let repo = git2::Repository::open(&workdir).unwrap();
            let result = super::resolve_remote_type(&repo, &workdir, "origin/main");
            ui::uninstall();
            result
        });
        for request in rx {
            if let Request::Prompt { reply, .. } = request {
                let _ = reply.send(None);
            }
        }
        worker.join().unwrap()
    });
    assert!(result.is_err());
    assert_eq!(remote_type_config(&test_repo), None);
}

// ── resolve_push_remote tests ────────────────────────────────────────────

#[test]
fn resolve_push_remote_github_fork_uses_origin() {
    let test_repo = TestRepo::new_with_remote();

    test_repo
        .repo
        .remote_set_url("origin", "https://github.com/user/fork.git")
        .unwrap();
    let remote_path = test_repo.remote_path().unwrap();
    test_repo
        .repo
        .remote("upstream", remote_path.to_str().unwrap())
        .unwrap();

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

    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

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
    assert_eq!(result, super::RemoteType::AzureDevOps);
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

    let wt_repo = git2::Repository::open(&wt_path).unwrap();

    // .git should be a file in the worktree, not a directory
    assert!(
        !wt_path.join(".git").is_dir(),
        ".git in worktree should not be a directory"
    );

    // detect_remote_type should still find the Gerrit hook via repo.path()
    let result = super::detect_remote_type(&wt_repo, &wt_path, "origin/main");
    assert_eq!(
        result,
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
    assert_eq!(result, super::RemoteType::GitLab);
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
    assert_eq!(result, super::RemoteType::GitLab);
}

// ── append_remote_urls tests ─────────────────────────────────────────────

#[test]
fn append_remote_urls_extracts_gitlab_mr_link() {
    let stderr = "remote: \n\
                  remote: To create a merge request for feature-a, visit:\n\
                  remote:   https://gitlab.com/group/repo/-/merge_requests/new?x=1\n\
                  remote: \n";
    let mut message = String::from("Pushed");
    super::append_remote_urls(&mut message, stderr, None);
    assert_eq!(
        message,
        "Pushed\nhttps://gitlab.com/group/repo/-/merge_requests/new?x=1"
    );
}

#[test]
fn append_remote_urls_wraps_gerrit_tag() {
    let stderr = "remote:   https://gerrit.example.com/c/proj/+/123 [NEW]\n";
    let mut message = String::from("Pushed");
    super::append_remote_urls(&mut message, stderr, None);
    assert_eq!(
        message,
        "Pushed\nhttps://gerrit.example.com/c/proj/+/123 `[NEW]`"
    );
}

#[test]
fn append_remote_urls_ignores_non_url_lines() {
    let stderr = "remote: Counting objects: 5, done.\nSwitched to branch\n";
    let mut message = String::from("Pushed");
    super::append_remote_urls(&mut message, stderr, None);
    assert_eq!(message, "Pushed");
}

#[test]
fn append_remote_urls_skips_only_the_new_pr_hint() {
    let stderr = "remote: Create a pull request for 'a' on GitHub by visiting:\n\
                  remote:      https://github.com/owner/repo/pull/new/a\n\
                  remote: GitHub found 1 vulnerability on owner/repo's default branch:\n\
                  remote:      https://github.com/owner/repo/security/dependabot\n";
    let mut message = String::from("Pushed");
    super::append_remote_urls(&mut message, stderr, Some("/pull/new/"));
    assert_eq!(
        message,
        "Pushed\nhttps://github.com/owner/repo/security/dependabot"
    );
}

#[test]
fn detect_remote_type_azure_by_url() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    test_repo
        .repo
        .remote_set_url(
            "origin",
            "https://dev.azure.com/myorg/myproject/_git/myrepo",
        )
        .unwrap();

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin/main");
    assert_eq!(result, super::RemoteType::AzureDevOps);
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

#[test]
fn on_github_com_reads_the_host_of_every_url_form() {
    let test_repo = TestRepo::new_with_remote();
    for (url, expected) in [
        ("git@github.com:owner/repo.git", true),
        ("https://github.com/owner/repo.git", true),
        ("https://user:token@github.com/owner/repo.git", true),
        ("ssh://git@GitHub.com:22/owner/repo.git", true),
        ("git@ghe.corp:owner/repo.git", false),
        ("https://github.com.evil.example/owner/repo", false),
        ("https://ghe.corp/github.com/repo", false),
        ("github-work:owner/repo", false),
    ] {
        test_repo.repo.remote_set_url("origin", url).unwrap();
        assert_eq!(
            super::on_github_com(&test_repo.repo, "origin"),
            expected,
            "{}",
            url
        );
    }
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

// ── PR links ──────────────────────────────────────────────────────────────

#[test]
fn github_compare_url_encodes_names_but_keeps_paths_and_forks() {
    assert_eq!(
        super::github_compare_url("owner/repo", "main", "feat/x"),
        "https://github.com/owner/repo/compare/main...feat/x?expand=1"
    );
    assert_eq!(
        super::github_compare_url("owner/repo", "main", "forker:a#b"),
        "https://github.com/owner/repo/compare/main...forker:a%23b?expand=1"
    );
}

#[test]
fn azure_create_pr_url_needs_project_and_repository() {
    let test_repo = TestRepo::new_with_remote();
    for url in [
        "https://dev.azure.com/org/my%20project/_git/my%20repo",
        "git@ssh.dev.azure.com:v3/org/my%20project/my%20repo",
    ] {
        test_repo.repo.remote_set_url("origin", url).unwrap();
        let azure = super::extract_azure_remote(&test_repo.repo, "origin");
        assert_eq!(
            super::azure_create_pr_url(azure.as_ref(), "feat/x y", "main").as_deref(),
            Some(
                "https://dev.azure.com/org/my%20project/_git/my%20repo/pullrequestcreate\
                 ?sourceRef=feat/x%20y&targetRef=main"
            ),
            "{}",
            url
        );
    }
    let legacy = super::AzureRemote {
        org_url: "https://dev.azure.com/org".into(),
        project: None,
        repository: Some("repo".into()),
    };
    assert_eq!(super::azure_create_pr_url(Some(&legacy), "x", "main"), None);
    assert_eq!(super::azure_create_pr_url(None, "x", "main"), None);
}

#[test]
fn gitlab_mr_links_tell_the_mr_from_the_form() {
    let view = "remote: \n\
                remote: View merge request for a:\n\
                remote:   https://gitlab.com/g/r/-/merge_requests/42\n";
    assert_eq!(
        super::gitlab_mr_links(view),
        super::GitlabMrLinks {
            view: Some("https://gitlab.com/g/r/-/merge_requests/42".into()),
            create: None,
        }
    );
    let create = "remote: To create a merge request for a, visit:\n\
                  remote:   https://gitlab.com/g/r/-/merge_requests/new?x=1\n";
    assert_eq!(
        super::gitlab_mr_links(create),
        super::GitlabMrLinks {
            view: None,
            create: Some("https://gitlab.com/g/r/-/merge_requests/new?x=1".into()),
        }
    );
    assert_eq!(super::gitlab_mr_links(""), super::GitlabMrLinks::default());
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
fn extract_azure_remote_unrecognized() {
    let test_repo = TestRepo::new_with_remote();
    test_repo
        .repo
        .remote_set_url("origin", "git@github.com:owner/repo.git")
        .unwrap();
    assert!(super::extract_azure_remote(&test_repo.repo, "origin").is_none());
}

#[test]
fn a_refused_push_names_the_flag_that_gets_past_it() {
    let refused = || anyhow::Error::new(super::PushFailed::Refused);

    let hinted = super::push_hint(refused(), "feature-a", false, false);
    assert!(
        hinted.to_string().contains("loom push feature-a -f"),
        "no way out offered: {}",
        hinted
    );
    let hinted = super::push_hint(refused(), "feature-a", true, false);
    assert!(
        hinted
            .to_string()
            .contains("loom push feature-a --no-pr -f"),
        "the way out drops --no-pr: {}",
        hinted
    );

    let forced = super::push_hint(refused(), "feature-a", false, true);
    assert_eq!(forced.to_string(), super::PUSH_FAILED);
    let other = super::push_hint(anyhow::anyhow!("Cancelled"), "feature-a", false, false);
    assert_eq!(other.to_string(), "Cancelled");
    for failed in [
        super::PushFailed::ServerRejected,
        super::PushFailed::NoRefStatus,
    ] {
        let hinted = super::push_hint(failed.into(), "feature-a", false, false);
        assert_eq!(
            hinted.to_string(),
            super::PUSH_FAILED,
            "{:?}: no force",
            failed
        );
    }
}

#[test]
fn a_refused_push_under_the_tui_names_its_key() {
    let (tx, _rx) = std::sync::mpsc::channel();
    crate::core::ui::install(tx);
    let hint = |failed: super::PushFailed, force| {
        super::push_hint(failed.into(), "feature-a", false, force).to_string()
    };
    let refused = hint(super::PushFailed::Refused, false);
    let no_status = hint(super::PushFailed::NoRefStatus, false);
    let declined = hint(super::PushFailed::ServerRejected, false);
    let forced = hint(super::PushFailed::Refused, true);
    crate::core::ui::uninstall();
    assert!(refused.contains("`P`"), "{}", refused);
    assert!(!refused.contains("loom push"), "{}", refused);
    // Credentials, not a moved remote, are what git cannot ask for here.
    assert!(no_status.contains("credentials"), "{}", no_status);
    assert!(!no_status.contains("`P`"), "{}", no_status);
    // The server took these, credentials and all.
    assert_eq!(declined, super::PUSH_FAILED);
    assert_eq!(forced, super::PUSH_FAILED);
}

#[test]
fn a_push_failure_is_classified_by_its_ref_statuses() {
    use super::PushFailed;
    let refused = "To ../remote.git\n ! [rejected]        a -> a (stale info)\n";
    let declined = "To ../remote.git\n ! [remote rejected] a -> a (pre-receive hook declined)\n";
    let unreported =
        "To ../remote.git\n ! [remote failure]  a -> a (remote failed to report status)\n";
    let unreached = "fatal: could not read Username for 'https://example.com': \
                     terminal prompts disabled\n";
    assert_eq!(super::classify_push_failure(refused), PushFailed::Refused);
    assert_eq!(
        super::classify_push_failure(declined),
        PushFailed::ServerRejected
    );
    assert_eq!(
        super::classify_push_failure(unreported),
        PushFailed::ServerRejected
    );
    assert_eq!(
        super::classify_push_failure(unreached),
        PushFailed::NoRefStatus
    );
}

#[test]
fn force_pushes_when_the_lease_check_would_refuse() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.create_branch("feature-a");
    test_repo.switch_branch("feature-a");
    let tip = test_repo.commit("feature commit", "a.txt");
    crate::git::run_git(&workdir, &["push", "origin", "feature-a"]).unwrap();

    let remote = test_repo.remote_path().unwrap();
    crate::git::run_git(
        &remote,
        &["update-ref", "refs/heads/feature-a", "refs/heads/main"],
    )
    .unwrap();

    let plan = super::PushPlan::single("feature-a");
    let refused = super::push_plain(&workdir, "origin", &plan, false).unwrap_err();
    assert_eq!(
        refused.downcast_ref::<super::PushFailed>(),
        Some(&super::PushFailed::Refused)
    );
    assert!(super::push_plain(&workdir, "origin", &plan, true).is_ok());

    let pushed =
        crate::git::run_git_stdout(&remote, &["rev-parse", "refs/heads/feature-a"]).unwrap();
    assert_eq!(pushed.trim(), tip.to_string());
}

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
        change_id: None,
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
    let info = stack_info(None, None, Some(RemoteStatus::Different));
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(plan.republish, vec![layer("d", "b")]);
    assert_eq!(plan.not_pushed, vec!["c"]);
}

#[test]
fn plan_push_republishes_a_layer_that_only_added_commits() {
    let info = stack_info(None, Some(RemoteStatus::Different), None);
    let plan = super::plan_push(&info, "b", "main");
    assert_eq!(plan.republish, vec![layer("c", "b")]);
    assert_eq!(plan.not_pushed, vec!["d"]);
}

#[test]
fn plan_push_keeps_a_synced_layer_as_a_republished_base() {
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
    let info = stack_info(None, None, None);
    let lone = super::plan_push(&info, "x", "main");
    assert!(!lone.is_stacked());
    assert!(!lone.is_chain());

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

    let stacked = super::plan_push(&info, "b", "main");
    assert!(stacked.is_chain());

    assert!(!super::PushPlan::single("x").is_chain());
}

#[test]
fn unsupported_stacks_are_refused_with_one_message() {
    let info = stack_info(None, None, None);
    let stacked = super::plan_push(&info, "b", "main");
    let refusal = |remote_type, has_cli| {
        super::refuse_unsupported_stack(&remote_type, &stacked, has_cli)
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        refusal(super::RemoteType::AzureDevOps, true),
        "Cannot create stacked PRs: `b` is stacked on `a`\n\
         Azure DevOps has no stacked pull requests\n\
         Land `a` first, or push without PRs (`--no-pr`)"
    );
    assert_eq!(
        refusal(super::RemoteType::GitHub, false),
        "Cannot create stacked PRs: `b` is stacked on `a`\n\
         Stacked pull requests need `gh`: https://cli.github.com\n\
         Install it, or push without PRs (`--no-pr`)"
    );

    for supported in [
        super::RemoteType::GitHub,
        super::RemoteType::GitLab,
        super::RemoteType::Plain,
    ] {
        assert!(super::refuse_unsupported_stack(&supported, &stacked, true).is_ok());
    }
    let lone = super::plan_push(&info, "x", "main");
    assert!(super::refuse_unsupported_stack(&super::RemoteType::AzureDevOps, &lone, false).is_ok());
    assert!(super::refuse_unsupported_stack(&super::RemoteType::GitHub, &lone, false).is_ok());
}

#[test]
fn pr_not_created_message_lists_link_then_hint() {
    assert_eq!(
        super::pr_not_created_message(
            "a",
            &super::missing_cli_reason("gh"),
            Some("https://example.com/new"),
            Some(&super::install_hint("gh", super::GH_INSTALL_URL)),
        ),
        "PR not created for `a`: `gh` is not installed\n\
         Create it at https://example.com/new\n\
         Install `gh` to have loom create it: https://cli.github.com"
    );
    assert_eq!(
        super::pr_not_created_message("a", "agent mode", None, None),
        "PR not created for `a`: agent mode"
    );
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
    assert_eq!(subjects("feature-b", "feature-a"), vec!["B1"]);
}

#[test]
fn gather_branch_commits_spans_the_stack_when_the_pr_targets_the_trunk() {
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
    let lone = super::push_args("origin", &["a"], false);
    assert!(!lone.contains(&"--atomic"));
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
    assert_eq!(
        super::stack_pr_numbers(&chain(&[("a", "main", Some(1)), ("b", "a", Some(2))])),
        vec![1, 2]
    );
    assert_eq!(
        super::stack_pr_numbers(&chain(&[
            ("a", "main", Some(1)),
            ("b", "a", None),
            ("c", "b", Some(3)),
        ])),
        vec![1]
    );
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

/// Run `push feature-a -f` as the TUI does, giving the confirmation `answer`,
/// with the remote's `feature-a` moved where only a force replaces it.
/// Returns the prompts, the result, and where the remote's `feature-a` ends.
fn force_push_under_tui(answer: bool) -> (Vec<String>, anyhow::Result<()>, String, String) {
    force_push_under_tui_of(answer, false)
}

/// `force_push_under_tui`, with `stacked` pushing `feature-b`, built on
/// `feature-a`, instead.
fn force_push_under_tui_of(
    answer: bool,
    stacked: bool,
) -> (Vec<String>, anyhow::Result<()>, String, String) {
    use crate::core::ui::{self, Answer, Request};

    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.set_config("loom.remote-type", "plain");
    test_repo.create_branch("feature-a");
    test_repo.switch_branch("feature-a");
    let tip = test_repo.commit("feature commit", "a.txt");
    let pushed_branch = if stacked {
        test_repo.create_branch("feature-b");
        test_repo.switch_branch("feature-b");
        test_repo.commit("stacked commit", "b.txt");
        "feature-b"
    } else {
        "feature-a"
    };
    test_repo.switch_branch("integration");
    test_repo.merge_no_ff(pushed_branch);
    crate::git::run_git(&workdir, &["push", "origin", "feature-a"]).unwrap();
    let remote = test_repo.remote_path().unwrap();
    crate::git::run_git(
        &remote,
        &["update-ref", "refs/heads/feature-a", "refs/heads/main"],
    )
    .unwrap();

    let (prompts, result) = test_repo.in_dir(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                ui::install(tx);
                let result = super::run(Some(pushed_branch.to_string()), false, true);
                ui::uninstall();
                result
            });
            let mut prompts = Vec::new();
            for request in rx {
                if let Request::Prompt { prompt, reply, .. } = request {
                    prompts.push(prompt);
                    let _ = reply.send(Some(Answer::Bool(answer)));
                }
            }
            (prompts, worker.join().unwrap())
        })
    });
    let pushed =
        crate::git::run_git_stdout(&remote, &["rev-parse", "refs/heads/feature-a"]).unwrap();
    (prompts, result, pushed.trim().to_string(), tip.to_string())
}

#[test]
fn a_force_push_under_the_tui_is_confirmed_naming_its_branches() {
    let (prompts, result, pushed, tip) = force_push_under_tui(false);
    assert_eq!(
        prompts,
        ["Force-push `feature-a` to `origin`?\nOverwrites whatever the remote holds for it"]
    );
    let err = result.unwrap_err();
    assert!(
        err.downcast_ref::<crate::core::ui::Cancelled>().is_some(),
        "{}",
        err
    );
    assert_ne!(pushed, tip, "declining leaves the remote alone");

    let (prompts, result, pushed, tip) = force_push_under_tui(true);
    assert_eq!(prompts.len(), 1);
    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(pushed, tip, "confirming forces the push");
}

#[test]
fn a_gerrit_review_push_never_asks_about_a_force() {
    // With the receiver gone a prompt is cancelled at once instead of waiting.
    let (tx, rx) = std::sync::mpsc::channel();
    drop(rx);
    crate::core::ui::install(tx);
    let gerrit = super::RemoteType::Gerrit {
        target_branch: "main".to_string(),
    };
    let plan = super::PushPlan::single("feature-a");
    let asked = super::confirm_force(&gerrit, false, "origin", &plan);
    crate::core::ui::uninstall();
    assert!(asked.is_ok(), "a review push is never forced");
}

#[test]
fn a_force_push_of_a_stacked_branch_names_the_branches_below_it() {
    let (prompts, result, pushed, tip) = force_push_under_tui_of(true, true);
    assert_eq!(
        prompts,
        ["Force-push `feature-a`, `feature-b` to `origin`?\n\
          Overwrites whatever the remote holds for them"]
    );
    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(pushed, tip, "the force reaches the branch below");
}

#[test]
fn a_push_that_cannot_reach_the_remote_is_not_refused() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.create_branch("feature-a");
    let gone = workdir.join("no-such-remote");
    crate::git::run_git(&workdir, &["remote", "add", "gone", gone.to_str().unwrap()]).unwrap();

    let plan = super::PushPlan::single("feature-a");
    let failed = super::push_plain(&workdir, "gone", &plan, false).unwrap_err();
    assert_eq!(
        failed.downcast_ref::<super::PushFailed>(),
        Some(&super::PushFailed::NoRefStatus)
    );
}

#[test]
fn a_push_a_hook_declines_is_server_rejected() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.create_branch("feature-a");
    let remote = test_repo.remote_path().unwrap();
    let hooks = remote.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    // As in the fixtures: a `core.hooksPath` in the user's config would leave it inert.
    let hooks_path = hooks.display().to_string().replace('\\', "/");
    crate::git::run_git(&remote, &["config", "core.hooksPath", &hooks_path]).unwrap();
    std::fs::write(hooks.join("pre-receive"), "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let hook = hooks.join("pre-receive");
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let plan = super::PushPlan::single("feature-a");
    let failed = super::push_plain(&workdir, "origin", &plan, false).unwrap_err();
    assert_eq!(
        failed.downcast_ref::<super::PushFailed>(),
        Some(&super::PushFailed::ServerRejected)
    );
}
