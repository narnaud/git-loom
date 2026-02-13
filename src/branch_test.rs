use crate::git;
use crate::git_commands::git_branch;
use crate::test_helpers::TestRepo;

#[test]
fn branch_at_merge_base_default() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");
    test_repo.commit_empty("A2");

    let info = git::gather_repo_info(&test_repo.repo).unwrap();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    // Create branch at merge-base (default, no target)
    let base = test_repo
        .repo
        .revparse_single(&info.upstream.base_short_id)
        .unwrap();
    git_branch::create(
        test_repo.workdir().as_path(),
        "feature-a",
        &base.id().to_string(),
    )
    .unwrap();

    assert!(test_repo.branch_exists("feature-a"));
    assert_eq!(test_repo.get_branch_target("feature-a"), base_oid);
}

#[test]
fn branch_at_specific_commit() {
    let test_repo = TestRepo::new_with_remote();
    let a1_oid = test_repo.commit_empty("A1");
    test_repo.commit_empty("A2");

    git_branch::create(
        test_repo.workdir().as_path(),
        "feature-a",
        &a1_oid.to_string(),
    )
    .unwrap();

    assert!(test_repo.branch_exists("feature-a"));
    assert_eq!(test_repo.get_branch_target("feature-a"), a1_oid);
}

#[test]
fn branch_at_branch_tip() {
    let test_repo = TestRepo::new_with_remote();
    let a1_oid = test_repo.commit_empty("A1");
    test_repo.create_branch_at_commit("feature-a", a1_oid);
    test_repo.commit_empty("A2");

    // Resolve feature-a to its tip commit and create new branch there
    let resolved = git::resolve_target(&test_repo.repo, "feature-a").unwrap();
    let hash = match resolved {
        git::Target::Branch(name) => {
            let branch = test_repo
                .repo
                .find_branch(&name, git2::BranchType::Local)
                .unwrap();
            branch.get().target().unwrap().to_string()
        }
        _ => panic!("expected branch target"),
    };

    git_branch::create(test_repo.workdir().as_path(), "feature-b", &hash).unwrap();

    assert!(test_repo.branch_exists("feature-b"));
    assert_eq!(test_repo.get_branch_target("feature-b"), a1_oid);
}

#[test]
fn branch_duplicate_name_fails() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");

    let head_oid = test_repo.head_oid().to_string();
    git_branch::create(test_repo.workdir().as_path(), "feature-a", &head_oid).unwrap();

    // Creating a branch with the same name should fail
    let result = git_branch::create(test_repo.workdir().as_path(), "feature-a", &head_oid);
    assert!(result.is_err());
}

#[test]
fn branch_shows_in_status() {
    let test_repo = TestRepo::new_with_remote();
    let a1_oid = test_repo.commit_empty("A1");
    test_repo.commit_empty("A2");

    git_branch::create(
        test_repo.workdir().as_path(),
        "feature-a",
        &a1_oid.to_string(),
    )
    .unwrap();

    let info = git::gather_repo_info(&test_repo.repo).unwrap();
    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        branch_names.contains(&"feature-a"),
        "feature-a should appear in status branches, got: {:?}",
        branch_names
    );

    let feature_a = info.branches.iter().find(|b| b.name == "feature-a").unwrap();
    assert_eq!(feature_a.tip_oid, a1_oid);
}

#[test]
fn branch_ownership_splits_commits() {
    let test_repo = TestRepo::new_with_remote();
    let a1_oid = test_repo.commit_empty("A1");
    test_repo.commit_empty("A2");
    let b1_oid = test_repo.commit_empty("B1");
    test_repo.commit_empty("B2");

    git_branch::create(
        test_repo.workdir().as_path(),
        "feature-a",
        &a1_oid.to_string(),
    )
    .unwrap();
    git_branch::create(
        test_repo.workdir().as_path(),
        "feature-b",
        &b1_oid.to_string(),
    )
    .unwrap();

    let info = git::gather_repo_info(&test_repo.repo).unwrap();
    assert_eq!(info.branches.len(), 2);
}

#[test]
fn branch_invalid_name_fails() {
    let result = git_branch::validate_name("my..branch");
    assert!(result.is_err(), "double dots should be invalid");

    let result = git_branch::validate_name("has space");
    assert!(result.is_err(), "spaces should be invalid");

    let result = git_branch::validate_name("valid-name");
    assert!(result.is_ok(), "valid name should pass");
}

#[test]
fn run_with_name_and_target() {
    let test_repo = TestRepo::new_with_remote();
    let a1_oid = test_repo.commit_empty("A1");
    test_repo.commit_empty("A2");

    // Use std::env to set the working directory for the run() function
    let project_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::env::set_current_dir(test_repo.workdir()).unwrap();

    let result = super::run(
        Some("feature-a".to_string()),
        Some(a1_oid.to_string()),
    );

    std::env::set_current_dir(&project_dir).unwrap();

    assert!(result.is_ok(), "branch::run failed: {:?}", result.err());
    assert!(test_repo.branch_exists("feature-a"));
    assert_eq!(test_repo.get_branch_target("feature-a"), a1_oid);
}

#[test]
fn run_duplicate_name_rejected_early() {
    let test_repo = TestRepo::new_with_remote();
    let a1_oid = test_repo.commit_empty("A1");
    test_repo.create_branch_at_commit("feature-a", a1_oid);

    let project_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::env::set_current_dir(test_repo.workdir()).unwrap();

    let result = super::run(
        Some("feature-a".to_string()),
        Some(a1_oid.to_string()),
    );

    std::env::set_current_dir(&project_dir).unwrap();

    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("already exists"),
        "expected 'already exists' error"
    );
}

#[test]
fn run_default_target_is_merge_base() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit_empty("A1");

    let base_oid = test_repo.find_remote_branch_target("origin/main");

    let project_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::env::set_current_dir(test_repo.workdir()).unwrap();

    let result = super::run(Some("feature-a".to_string()), None);

    std::env::set_current_dir(&project_dir).unwrap();

    assert!(result.is_ok(), "branch::run failed: {:?}", result.err());
    assert_eq!(test_repo.get_branch_target("feature-a"), base_oid);
}
