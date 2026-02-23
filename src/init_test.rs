use crate::test_helpers::TestRepo;

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
        err.contains("not a valid branch name"),
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
