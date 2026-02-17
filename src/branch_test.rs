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

    let hash = test_repo.get_branch_target("feature-a").to_string();
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

    let feature_a = info
        .branches
        .iter()
        .find(|b| b.name == "feature-a")
        .unwrap();
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
    let a1_oid = test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");

    let result =
        test_repo.in_dir(|| super::run(Some("feature-a".to_string()), Some(a1_oid.to_string())));

    assert!(result.is_ok(), "branch::run failed: {:?}", result.err());
    assert!(test_repo.branch_exists("feature-a"));
}

#[test]
fn run_duplicate_name_rejected_early() {
    let test_repo = TestRepo::new_with_remote();
    let a1_oid = test_repo.commit_empty("A1");
    test_repo.create_branch_at_commit("feature-a", a1_oid);

    let result =
        test_repo.in_dir(|| super::run(Some("feature-a".to_string()), Some(a1_oid.to_string())));

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

    let result = test_repo.in_dir(|| super::run(Some("feature-a".to_string()), None));

    assert!(result.is_ok(), "branch::run failed: {:?}", result.err());
    assert_eq!(test_repo.get_branch_target("feature-a"), base_oid);
}

#[test]
fn branch_weave_creates_merge_topology() {
    // origin/main → A1 → A2 → A3 (HEAD)
    // Branch at A2 should produce:
    //   origin/main → A1 → A2 (feature-a)
    //              ↘              ↘
    //               A3' --------→ merge (HEAD)
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    let a2_oid = test_repo.head_oid();
    test_repo.commit("A3", "a3.txt");

    let result =
        test_repo.in_dir(|| super::run(Some("feature-a".to_string()), Some(a2_oid.to_string())));

    assert!(result.is_ok(), "branch::run failed: {:?}", result.err());
    assert!(test_repo.branch_exists("feature-a"));

    // HEAD should be a merge commit (2 parents)
    let head = test_repo.head_commit();
    assert_eq!(
        head.parent_count(),
        2,
        "HEAD should be a merge commit with 2 parents"
    );

    // One parent should be the feature-a branch tip
    let parent_oids: Vec<git2::Oid> = (0..head.parent_count())
        .map(|i| head.parent_id(i).unwrap())
        .collect();
    let feature_a_oid = test_repo.get_branch_target("feature-a");
    assert!(
        parent_oids.contains(&feature_a_oid),
        "merge commit should have feature-a as a parent"
    );
}

#[test]
fn branch_at_head_no_weave() {
    // Branch at HEAD should NOT create a merge
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    let head_before = test_repo.head_oid();

    let result = test_repo
        .in_dir(|| super::run(Some("feature-a".to_string()), Some(head_before.to_string())));

    assert!(result.is_ok(), "branch::run failed: {:?}", result.err());

    // HEAD should be unchanged (no merge commit)
    assert_eq!(test_repo.head_oid(), head_before);
    let head = test_repo.head_commit();
    assert_eq!(
        head.parent_count(),
        1,
        "HEAD should NOT be a merge commit when branching at HEAD"
    );
}

#[test]
fn branch_at_merge_base_no_weave() {
    // Branch at merge-base should NOT create a merge
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    let head_before = test_repo.head_oid();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    let result =
        test_repo.in_dir(|| super::run(Some("feature-a".to_string()), Some(base_oid.to_string())));

    assert!(result.is_ok(), "branch::run failed: {:?}", result.err());

    // HEAD should be unchanged (no merge commit)
    assert_eq!(test_repo.head_oid(), head_before);
    let head = test_repo.head_commit();
    assert_eq!(
        head.parent_count(),
        1,
        "HEAD should NOT be a merge commit when branching at merge-base"
    );
}

#[test]
fn branch_inside_existing_branch_no_weave() {
    // When a branch already exists via merge topology, creating a new branch
    // inside it should NOT trigger weaving — the topology is already correct.
    //
    // Setup: origin/main → A1 → A2 (feature-a) merged into integration
    //        with B1 on the integration line
    // Then create feature-b at A1 (inside feature-a's side branch)
    use crate::git_commands::git_merge;

    let test_repo = TestRepo::new_with_remote();

    // Build a side branch: A1 → A2
    test_repo.commit("A1", "a1.txt");
    let a1_oid = test_repo.head_oid();
    test_repo.commit("A2", "a2.txt");
    let a2_oid = test_repo.head_oid();

    // Create feature-a branch at A2
    git_branch::create(
        test_repo.workdir().as_path(),
        "feature-a",
        &a2_oid.to_string(),
    )
    .unwrap();

    // Reset integration to merge-base, add a commit on integration line, then merge feature-a
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.reset_hard(base_oid);

    // Add a commit on the integration line
    test_repo.commit("B1", "b1.txt");

    // Merge feature-a into integration
    git_merge::merge(test_repo.workdir().as_path(), "feature-a").unwrap();
    let head_before = test_repo.head_oid();

    // Now create feature-b at A1, which is inside the feature-a side branch
    let result =
        test_repo.in_dir(|| super::run(Some("feature-b".to_string()), Some(a1_oid.to_string())));

    assert!(result.is_ok(), "branch::run failed: {:?}", result.err());
    assert!(test_repo.branch_exists("feature-b"));
    // HEAD should be unchanged — no new merge commit
    assert_eq!(
        test_repo.head_oid(),
        head_before,
        "HEAD should be unchanged when branching inside an existing side branch"
    );
}
