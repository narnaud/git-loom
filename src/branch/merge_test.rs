use crate::core::repo;
use crate::core::test_helpers::TestRepo;

/// Merging a non-woven local branch weaves it into integration.
#[test]
fn merge_weaves_existing_branch() {
    let test_repo = TestRepo::new_with_remote();

    // Create commits on integration
    test_repo.commit("B1", "b1.txt");

    // Create feature-a at merge-base with its own commits
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    // Add a commit to feature-a
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    // Merge feature-a into integration
    test_repo
        .in_dir(|| super::merge::run(Some("feature-a".to_string()), false))
        .unwrap();

    // HEAD should be a merge commit
    assert_eq!(
        test_repo.head_commit().parent_count(),
        2,
        "HEAD should be a merge commit after merging a branch"
    );

    // feature-a should now be in the woven branches
    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        branch_names.contains(&"feature-a"),
        "feature-a should be in integration branches after merge, got: {:?}",
        branch_names
    );
}

/// Merging an already-woven branch should error.
#[test]
fn merge_already_woven_errors() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    let a2_oid = test_repo.head_oid();

    // Create and weave feature-a via branch::new
    test_repo
        .in_dir(|| crate::branch::new::run(Some("feature-a".to_string()), Some(a2_oid.to_string())))
        .unwrap();

    // Try to merge it again — should error
    let result = test_repo.in_dir(|| super::merge::run(Some("feature-a".to_string()), false));
    assert!(result.is_err(), "merging already-woven branch should error");
}

/// Merging then unmerging a branch is a round-trip: branch still exists with its commits.
#[test]
fn merge_then_unmerge_round_trip() {
    let test_repo = TestRepo::new_with_remote();

    // Create and populate a non-woven branch
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-rt", &base_oid.to_string());
    test_repo.switch_branch("feature-rt");
    test_repo.commit("RT1", "rt1.txt");
    test_repo.commit("RT2", "rt2.txt");
    test_repo.switch_branch("integration");

    // Merge (weave)
    test_repo
        .in_dir(|| super::merge::run(Some("feature-rt".to_string()), false))
        .unwrap();

    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let woven: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        woven.contains(&"feature-rt"),
        "branch should be woven after merge"
    );

    // Unmerge
    test_repo
        .in_dir(|| super::unmerge::run(Some("feature-rt".to_string())))
        .unwrap();

    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let woven: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        !woven.contains(&"feature-rt"),
        "branch should not be woven after unmerge"
    );

    // Branch ref should still exist
    assert!(test_repo.branch_exists("feature-rt"));

    // The tip commit of feature-rt should have message "RT2"
    let tip = test_repo.get_branch_target("feature-rt");
    let tip_commit = test_repo.find_commit(tip);
    assert_eq!(tip_commit.summary().unwrap_or(""), "RT2");
}

/// Merging a nonexistent branch should error.
#[test]
fn merge_nonexistent_branch_errors() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");

    let result =
        test_repo.in_dir(|| super::merge::run(Some("nonexistent-branch".to_string()), false));
    assert!(result.is_err(), "merging nonexistent branch should error");
}

/// When a merge conflicts, the operation pauses: state file saved, HEAD unchanged.
/// Resolving and running continue completes the merge and weaves the branch.
#[test]
fn merge_conflict_continue_weaves_branch() {
    let test_repo = TestRepo::new_with_remote();

    // Both branches modify the same file differently → merge conflict.
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-conflict", &base_oid.to_string());

    test_repo.write_file("shared.txt", "integration");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Integration change");

    test_repo.switch_branch("feature-conflict");
    test_repo.write_file("shared.txt", "feature");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Feature change");
    test_repo.switch_branch("integration");

    let pre_merge_head = test_repo.head_oid();

    // Merge pauses — must not return Err.
    let result =
        test_repo.in_dir(|| super::merge::run(Some("feature-conflict".to_string()), false));
    assert!(
        result.is_ok(),
        "merge should return Ok on conflict: {:?}",
        result
    );

    let state_path = test_repo.repo.path().join("loom/state.json");
    assert!(
        state_path.exists(),
        "state file must exist when merge is paused"
    );
    assert_eq!(
        test_repo.head_oid(),
        pre_merge_head,
        "HEAD must not move on conflict"
    );

    // Resolve conflict and continue.
    test_repo.write_file("shared.txt", "resolved");
    test_repo.stage_files(&["shared.txt"]);
    crate::core::transaction::continue_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();

    assert!(
        !state_path.exists(),
        "state file must be deleted after continue"
    );
    assert_eq!(
        test_repo.head_commit().parent_count(),
        2,
        "HEAD must be a merge commit after continue"
    );

    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        branch_names.contains(&"feature-conflict"),
        "branch must be woven after continue, got: {:?}",
        branch_names
    );
}

/// Aborting a paused merge restores HEAD to its pre-merge state and leaves the
/// branch unwoven.
#[test]
fn merge_conflict_abort_restores_state() {
    let test_repo = TestRepo::new_with_remote();

    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-abort", &base_oid.to_string());

    test_repo.write_file("shared.txt", "integration");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Integration change");

    test_repo.switch_branch("feature-abort");
    test_repo.write_file("shared.txt", "feature");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Feature change");
    test_repo.switch_branch("integration");

    let pre_merge_head = test_repo.head_oid();

    let result = test_repo.in_dir(|| super::merge::run(Some("feature-abort".to_string()), false));
    assert!(
        result.is_ok(),
        "merge should return Ok on conflict: {:?}",
        result
    );

    let state_path = test_repo.repo.path().join("loom/state.json");
    assert!(
        state_path.exists(),
        "state file must exist when merge is paused"
    );

    crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();

    assert!(
        !state_path.exists(),
        "state file must be deleted after abort"
    );
    assert_eq!(
        test_repo.head_oid(),
        pre_merge_head,
        "HEAD must be restored after abort"
    );
    assert_eq!(
        test_repo.head_commit().parent_count(),
        1,
        "HEAD must not be a merge commit after abort"
    );

    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        !branch_names.contains(&"feature-abort"),
        "branch must NOT be woven after abort, got: {:?}",
        branch_names
    );
}
