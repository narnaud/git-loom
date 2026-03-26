use crate::git;
use crate::test_helpers::TestRepo;

/// Unmerging a woven branch removes it from integration but keeps the ref.
#[test]
fn unmerge_removes_branch_from_integration() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    test_repo.commit("A2", "a2.txt");
    let a2_oid = test_repo.head_oid();

    // Create and weave feature-a
    test_repo
        .in_dir(|| crate::branch::new::run(Some("feature-a".to_string()), Some(a2_oid.to_string())))
        .unwrap();

    // Verify it's woven (HEAD is a merge commit)
    assert_eq!(test_repo.head_commit().parent_count(), 2);

    // Unmerge it
    test_repo
        .in_dir(|| super::unmerge::run(Some("feature-a".to_string())))
        .unwrap();

    // Branch ref should still exist
    assert!(test_repo.branch_exists("feature-a"));

    // feature-a should not be in the woven branches list
    let info = git::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        !branch_names.contains(&"feature-a"),
        "feature-a should not be in integration branches after unmerge, got: {:?}",
        branch_names
    );
}

/// When replaying integration commits on top of the merge-base after removing a
/// woven branch, a conflict can occur if those commits depended on the branch's
/// content.  The rebase must be aborted automatically and the error propagated.
///
/// Conflict setup: feature-a creates `shared.txt`; integration commit B modifies
/// it.  Unmerging feature-a requires replaying B on the merge-base (which has no
/// `shared.txt`) → delete/modify conflict.
#[test]
fn unmerge_conflict_aborts() {
    let test_repo = TestRepo::new_with_remote();

    // feature-a creates shared.txt
    let a_oid = test_repo.commit("from-a", "shared.txt");
    test_repo
        .in_dir(|| crate::branch::new::run(Some("feature-a".to_string()), Some(a_oid.to_string())))
        .unwrap();

    // Integration commit B modifies shared.txt — its diff expects "from-a" as
    // context, which will be absent when replayed on the merge-base.
    test_repo.write_file("shared.txt", "from-b");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit B");

    let result = test_repo.in_dir(|| super::unmerge::run(Some("feature-a".to_string())));

    assert!(
        result.is_err(),
        "unmerge should fail when the rebase conflicts"
    );

    // The rebase must be fully aborted — no stale git rebase state.
    assert!(
        !crate::git_commands::rebase_is_in_progress(test_repo.repo.path()),
        "rebase should be aborted, not left paused"
    );
    // No loom state file should be left behind.
    assert!(
        !test_repo
            .repo
            .path()
            .join("loom")
            .join("state.json")
            .exists(),
        "no loom state should be written for a non-resumable command"
    );
}

/// Unmerging a branch that isn't woven should error.
#[test]
fn unmerge_non_woven_branch_errors() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");

    // Create a branch at merge-base (not woven, just a ref)
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-a", &base_oid.to_string());

    let result = test_repo.in_dir(|| super::unmerge::run(Some("feature-a".to_string())));

    assert!(result.is_err());
}
