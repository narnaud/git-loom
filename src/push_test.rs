use crate::git_commands::git_branch;
use crate::test_helpers::TestRepo;

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
    crate::git_commands::run_git(&workdir, &["config", "loom.remote-type", "gerrit"]).unwrap();

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
    crate::git_commands::run_git(&workdir, &["config", "loom.remote-type", "github"]).unwrap();

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
    crate::git_commands::run_git(&workdir, &["config", "loom.remote-type", "gerrit"]).unwrap();

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

// ── resolve_branch tests ─────────────────────────────────────────────────

#[test]
fn resolve_branch_accepts_woven_branch() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create feature-a at merge-base
    git_branch::create(workdir.as_path(), "feature-a", &base_oid.to_string()).unwrap();

    // Switch to feature-a, add a commit, switch back
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    // Add integration commit + merge to create woven topology
    test_repo.commit("Int", "int.txt");
    crate::git_commands::git_merge::merge_no_ff(workdir.as_path(), "feature-a").unwrap();

    let result = super::resolve_branch(&test_repo.repo, "feature-a");
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

    let result = super::resolve_branch(&test_repo.repo, "stray-branch");
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

    let result = super::resolve_branch(&test_repo.repo, &c1_oid.to_string());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not a commit"));
}
