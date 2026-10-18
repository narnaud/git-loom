use crate::core::test_helpers::TestRepo;

// ── detect_remote_type tests ─────────────────────────────────────────────

#[test]
fn detect_remote_type_plain_by_default() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
    assert_eq!(result, super::RemoteType::Plain);
}

#[test]
fn detect_remote_type_gerrit_by_config() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    test_repo.set_config("loom.remoteType", "gerrit");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
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

    test_repo.set_config("loom.remoteType", "github");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
    assert_eq!(result, super::RemoteType::GitHub);
}

#[test]
fn detect_remote_type_config_overrides_url() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    // Even though remote URL is a local path (not github.com),
    // explicit config should take priority
    test_repo.set_config("loom.remoteType", "gerrit");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
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

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
    assert_eq!(
        result,
        super::RemoteType::Gerrit {
            target_branch: "main".to_string()
        }
    );
}

#[test]
fn detect_remote_type_gerrit_by_ssh_port() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    for (url, gerrit) in [
        ("ssh://jane@review.example.com:29418/proj", true),
        ("ssh://review.example.com:29418/proj", true),
        ("ssh://git@example.com:22/proj", false),
        ("git@example.com:29418/proj", false),
    ] {
        test_repo.repo.remote_set_url("origin", url).unwrap();
        let detected = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
        assert_eq!(
            matches!(detected, super::RemoteType::Gerrit { .. }),
            gerrit,
            "{}",
            url
        );
    }
}

#[test]
fn detect_remote_type_plain_by_config() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    // A saved "plain" answer (from the remote type menu) must be honored
    // without warning about an unknown value
    test_repo.set_config("loom.remoteType", "plain");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
    assert_eq!(result, super::RemoteType::Plain);
}

#[test]
fn remote_types_round_trip_through_their_config_value() {
    for (value, _) in super::REMOTE_TYPES {
        let remote_type = super::parse_remote_type(value, "main").unwrap();
        assert_eq!(remote_type.config_value(), value);
    }
}

// ── resolve_remote_type tests ────────────────────────────────────────────

fn remote_type_config(test_repo: &TestRepo) -> Option<String> {
    crate::git::run_git_stdout(
        &test_repo.workdir(),
        &["config", "--get", "loom.remoteType"],
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
            let result = super::resolve_remote_type(&repo, &workdir, "origin", "main");
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
    test_repo.set_config("loom.remoteType", "plain");

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
            "Which kind of remote is `origin`? (saved as `loom.remoteType`)".to_string(),
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
            let result = super::resolve_remote_type(&repo, &workdir, "origin", "main");
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
        "upstream",
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
        "origin",
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
        "upstream",
        &super::RemoteType::Plain,
    );
    assert_eq!(result, "upstream");
}

/// A local upstream's `.` was taken for a remote and handed to `git push`
/// (#442).
#[test]
fn a_local_upstream_without_a_push_remote_is_refused() {
    let test_repo = TestRepo::new_with_remote();
    let err = super::integration_remote(&test_repo.repo, &test_repo.workdir(), ".").unwrap_err();
    assert!(err.to_string().contains("loom.pushRemote"), "{err}");
}

#[test]
fn a_push_remote_naming_no_remote_is_named_in_the_refusal() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.set_config("loom.pushRemote", "missing");
    let err = super::integration_remote(&test_repo.repo, &test_repo.workdir(), ".").unwrap_err();
    assert!(
        err.to_string().contains("`missing`, which names no remote"),
        "{err}"
    );
}

#[test]
fn the_push_remote_stands_for_a_local_upstream() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.set_config("loom.pushRemote", "origin");

    assert_eq!(
        super::integration_remote(&test_repo.repo, &workdir, ".").unwrap(),
        "origin"
    );
    assert_eq!(
        super::fork_push_remote(&test_repo.repo, &workdir, ".", "main").as_deref(),
        Some("origin")
    );
    assert_eq!(
        super::fork_push_remote(&test_repo.repo, &workdir, "origin", "main"),
        None
    );
}

/// The dashed spellings predate the camelCase ones and stay readable.
#[test]
fn the_dashed_spellings_are_still_read() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.set_config("loom.remote-type", "gerrit");
    test_repo.set_config("loom.push-remote", "missing");

    assert!(matches!(
        super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main"),
        super::RemoteType::Gerrit { .. }
    ));
    let err = super::integration_remote(&test_repo.repo, &workdir, ".").unwrap_err();
    assert!(
        err.to_string().contains("`missing`, which names no remote"),
        "{err}"
    );
}

#[test]
fn the_camel_case_spellings_win_over_the_dashed_ones() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.set_config("loom.remote-type", "gerrit");
    test_repo.set_config("loom.remoteType", "github");
    test_repo.set_config("loom.push-remote", "missing");
    test_repo.set_config("loom.pushRemote", "origin");

    assert_eq!(
        super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main"),
        super::RemoteType::GitHub
    );
    assert_eq!(
        super::integration_remote(&test_repo.repo, &workdir, ".").unwrap(),
        "origin"
    );
}

#[test]
fn detect_remote_type_azure_by_config() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    test_repo.commit("C1", "c1.txt");

    test_repo.set_config("loom.remoteType", "azure");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
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
    let result = super::detect_remote_type(&wt_repo, &wt_path, "origin", "main");
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

    test_repo.set_config("loom.remoteType", "gitlab");

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
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

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
    assert_eq!(result, super::RemoteType::GitLab);
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

    let result = super::detect_remote_type(&test_repo.repo, &workdir, "origin", "main");
    assert_eq!(result, super::RemoteType::AzureDevOps);
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
fn extract_gh_repo_any_scheme_user_or_port_on_github_com() {
    let test_repo = TestRepo::new_with_remote();
    for (url, expected) in [
        ("ssh://git@github.com/owner/repo.git", Some("owner/repo")),
        ("ssh://git@GitHub.com:22/owner/repo", Some("owner/repo")),
        (
            "https://user:token@github.com/owner/repo.git",
            Some("owner/repo"),
        ),
        ("https://ghe.corp/owner/repo.git", None),
    ] {
        test_repo.repo.remote_set_url("origin", url).unwrap();
        assert_eq!(
            super::extract_gh_repo(&test_repo.repo, "origin").as_deref(),
            expected,
            "{}",
            url
        );
    }
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
