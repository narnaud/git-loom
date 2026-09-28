use crate::core::test_helpers::TestRepo;
use git2::BranchType;

#[test]
fn init_creates_tracking_branch_with_default_name() {
    let test_repo = TestRepo::new_with_remote();
    // Switch back to main so init can detect its upstream
    test_repo.switch_branch("main");
    // Delete the pre-existing "integration" branch so the default name is available
    test_repo.delete_branch("integration");

    let result = test_repo.in_dir(|| super::run(None));
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    // Should have switched to the new branch
    assert_eq!(test_repo.current_branch_name(), "integration");

    // Branch should exist and track origin/main
    let branch = test_repo
        .repo
        .find_branch("integration", git2::BranchType::Local)
        .expect("integration branch should exist");
    let upstream = branch.upstream().expect("should have upstream");
    let upstream_name = upstream.name().unwrap().unwrap();
    assert_eq!(upstream_name, "origin/main");
}

#[test]
fn init_creates_tracking_branch_with_custom_name() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.switch_branch("main");

    let result = test_repo.in_dir(|| super::run(Some("my-integration".to_string())));
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    assert_eq!(test_repo.current_branch_name(), "my-integration");
    assert!(test_repo.branch_exists("my-integration"));
}

#[test]
fn init_fails_if_branch_already_exists() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.switch_branch("main");

    // "integration" already exists from new_with_remote()
    let result = test_repo.in_dir(|| super::run(Some("integration".to_string())));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("already exists"),
        "Expected 'already exists' error, got: {}",
        err
    );
}

#[test]
fn init_fails_with_empty_name() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.switch_branch("main");

    let result = test_repo.in_dir(|| super::run(Some("  ".to_string())));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("empty"),
        "Expected empty name error, got: {}",
        err
    );
}

#[test]
fn init_fails_with_invalid_name() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.switch_branch("main");

    let result = test_repo.in_dir(|| super::run(Some("my..branch".to_string())));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("is not valid"),
        "Expected invalid name error, got: {}",
        err
    );
}

#[test]
fn init_points_branch_at_upstream_tip() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.switch_branch("main");
    // Delete the pre-existing "integration" branch so the default name is available
    test_repo.delete_branch("integration");

    // The upstream tip should be origin/main
    let origin_main_oid = test_repo.find_remote_branch_target("origin/main");

    let result = test_repo.in_dir(|| super::run(None));
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    // The new branch should point at the same commit as origin/main
    let integration_oid = test_repo.get_branch_target("integration");
    assert_eq!(integration_oid, origin_main_oid);
}

#[test]
fn init_detects_upstream_from_current_branch() {
    let test_repo = TestRepo::new_with_remote();
    // The "integration" branch tracks origin/main, switch to it
    test_repo.switch_branch("integration");

    let result = test_repo.in_dir(|| super::run(Some("my-loom".to_string())));
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let branch = test_repo
        .repo
        .find_branch("my-loom", git2::BranchType::Local)
        .expect("my-loom branch should exist");
    let upstream = branch.upstream().expect("should have upstream");
    let upstream_name = upstream.name().unwrap().unwrap();
    assert_eq!(upstream_name, "origin/main");
}

#[test]
fn init_prefers_upstream_remote_on_github() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.switch_branch("main");
    test_repo.delete_branch("integration");

    // Set origin URL to github.com to simulate a GitHub fork
    test_repo
        .repo
        .remote_set_url("origin", "https://github.com/user/fork.git")
        .unwrap();

    // Add "upstream" remote pointing to the same bare repo
    let remote_path = test_repo.remote_path().unwrap();
    test_repo
        .repo
        .remote("upstream", remote_path.to_str().unwrap())
        .unwrap();

    // Fetch from upstream to get upstream/main
    test_repo
        .repo
        .find_remote("upstream")
        .unwrap()
        .fetch(&["main"], None, None)
        .unwrap();

    let result = test_repo.in_dir(|| super::run(None));
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    // Should track upstream/main, not origin/main
    let branch = test_repo
        .repo
        .find_branch("integration", BranchType::Local)
        .expect("integration branch should exist");
    let upstream = branch.upstream().expect("should have upstream");
    let upstream_name = upstream.name().unwrap().unwrap();
    assert_eq!(upstream_name, "upstream/main");
}

#[test]
fn init_no_upstream_remote_uses_origin_on_github() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.switch_branch("main");
    test_repo.delete_branch("integration");

    // Set origin URL to github.com but don't add an "upstream" remote
    test_repo
        .repo
        .remote_set_url("origin", "https://github.com/user/repo.git")
        .unwrap();

    let result = test_repo.in_dir(|| super::run(None));
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    // Should still track origin/main since there's no "upstream" remote
    let branch = test_repo
        .repo
        .find_branch("integration", BranchType::Local)
        .expect("integration branch should exist");
    let upstream = branch.upstream().expect("should have upstream");
    let upstream_name = upstream.name().unwrap().unwrap();
    assert_eq!(upstream_name, "origin/main");
}

fn worktree_head_and_upstream(path: &std::path::Path) -> (String, String) {
    let repo = git2::Repository::open(path).unwrap();
    let head = repo.head().unwrap();
    let name = head.shorthand().unwrap().to_string();
    let branch = repo.find_branch(&name, BranchType::Local).unwrap();
    let upstream = branch
        .upstream()
        .unwrap()
        .name()
        .unwrap()
        .unwrap()
        .to_string();
    (name, upstream)
}

#[test]
fn init_in_worktree_names_branch_after_dir_suffix() {
    let test_repo = TestRepo::new_with_remote();
    let path = test_repo.add_worktree("work-foo", &["--detach"]);

    let result = test_repo.in_dir_path(&path, || super::run(None));
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    assert_eq!(
        worktree_head_and_upstream(&path),
        ("integration-foo".to_string(), "origin/main".to_string())
    );
    assert_eq!(test_repo.current_branch_name(), "integration");
}

#[test]
fn init_in_worktree_outside_naming_scheme_uses_whole_dir_name() {
    let test_repo = TestRepo::new_with_remote();
    let path = test_repo.add_worktree("other", &["--detach"]);

    let result = test_repo.in_dir_path(&path, || super::run(None));
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    assert_eq!(worktree_head_and_upstream(&path).0, "integration-other");
}

#[test]
fn init_in_worktree_keeps_explicit_name() {
    let test_repo = TestRepo::new_with_remote();
    let path = test_repo.add_worktree("work-foo", &["--detach"]);

    let result = test_repo.in_dir_path(&path, || super::run(Some("mine".to_string())));
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    assert_eq!(worktree_head_and_upstream(&path).0, "mine");
}

#[test]
fn init_in_worktree_tracks_main_worktree_upstream() {
    // origin/release is not a default-branch candidate, so only the main
    // worktree's `integration` can lead init to it.
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    crate::git::run_git(
        &workdir,
        &["push", "-q", "origin", "HEAD:refs/heads/release"],
    )
    .unwrap();
    crate::git::run_git(&workdir, &["fetch", "-q", "origin"]).unwrap();
    crate::git::run_git(&workdir, &["branch", "-q", "-u", "origin/release"]).unwrap();
    let path = test_repo.add_worktree("work-foo", &["-b", "scratch"]);

    let result = test_repo.in_dir_path(&path, || super::run(None));
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    assert_eq!(
        worktree_head_and_upstream(&path),
        ("integration-foo".to_string(), "origin/release".to_string())
    );
}
