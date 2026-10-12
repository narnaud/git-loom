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
        .in_dir(|| super::run(Some("feature-a".to_string()), false))
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

/// A local branch whose name contains '/' must be merged as-is, not mistaken
/// for a remote branch and stripped down to a duplicate ref.
#[test]
fn merge_local_branch_with_slash_keeps_full_name() {
    let test_repo = TestRepo::new_with_remote();

    test_repo.commit("B1", "b1.txt");

    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("wip/dfaure/feature-a", &base_oid.to_string());

    test_repo.switch_branch("wip/dfaure/feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    test_repo
        .in_dir(|| super::run(Some("wip/dfaure/feature-a".to_string()), false))
        .unwrap();

    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let branch_names: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        branch_names.contains(&"wip/dfaure/feature-a"),
        "the branch passed in must be the one woven, got: {:?}",
        branch_names
    );
    assert!(
        !test_repo.branch_exists("dfaure/feature-a"),
        "no duplicate ref with the first path component stripped may be created"
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
        .in_dir(|| crate::branch::run(Some("feature-a".to_string()), Some(a2_oid.to_string())))
        .unwrap();

    // Try to merge it again — should error
    let result = test_repo.in_dir(|| super::run(Some("feature-a".to_string()), false));
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
        .in_dir(|| super::run(Some("feature-rt".to_string()), false))
        .unwrap();

    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let woven: Vec<&str> = info.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        woven.contains(&"feature-rt"),
        "branch should be woven after merge"
    );

    // Unmerge
    test_repo
        .in_dir(|| crate::unmerge::run(Some("feature-rt".to_string())))
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
    assert_eq!(tip_commit.summary().ok().flatten().unwrap_or(""), "RT2");
}

/// Merging a nonexistent branch should error.
#[test]
fn merge_nonexistent_branch_errors() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");

    let result = test_repo.in_dir(|| super::run(Some("nonexistent-branch".to_string()), false));
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
    let result = test_repo.in_dir(|| super::run(Some("feature-conflict".to_string()), false));
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

    let result = test_repo.in_dir(|| super::run(Some("feature-abort".to_string()), false));
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

/// `feature-a` (one commit on origin/main) woven into `integration-foo`, the
/// integration branch of a `work-foo` worktree.
fn woven_in_other_worktree() -> (TestRepo, std::path::PathBuf) {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");

    let path = test_repo.add_worktree("work-foo", &["-b", "integration-foo", "origin/main"]);
    crate::git::run_git(&path, &["branch", "-q", "-u", "origin/main"]).unwrap();
    test_repo
        .in_dir_path(&path, || super::run(Some("feature-a".to_string()), false))
        .unwrap();
    (test_repo, path)
}

#[test]
fn merge_refuses_branch_woven_in_other_worktree() {
    let (test_repo, _path) = woven_in_other_worktree();
    let head = test_repo.head_oid();

    let result = test_repo.in_dir(|| super::run(Some("feature-a".to_string()), false));

    let err = result.unwrap_err().to_string();
    assert!(err.contains("integration-foo"), "got: {err}");
    assert!(err.contains("work-foo"), "got: {err}");
    assert!(
        err.contains("`\nUnmerge it there first: `loom unmerge feature-a`"),
        "got: {err}"
    );
    assert_eq!(test_repo.head_oid(), head);
}

#[test]
fn merge_refuses_other_worktree_integration_branch() {
    let (test_repo, _path) = woven_in_other_worktree();
    let head = test_repo.head_oid();

    let result = test_repo.in_dir(|| super::run(Some("integration-foo".to_string()), false));

    let err = result.unwrap_err().to_string();
    assert!(err.contains("checked out in the worktree"), "got: {err}");
    assert!(err.contains("work-foo"), "got: {err}");
    assert_eq!(test_repo.head_oid(), head);
}

/// `origin/<name>` one commit past origin/main writing `file`, with no local
/// branch of that name.
fn remote_only_branch(test_repo: &TestRepo, name: &str, file: &str, content: &str) {
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("scratch", &base_oid.to_string());
    test_repo.switch_branch("scratch");
    test_repo.write_file(file, content);
    test_repo.stage_files(&[file]);
    test_repo.commit_staged(name);
    let tip = test_repo.head_oid();
    test_repo.switch_branch("integration");
    test_repo
        .repo
        .reference(&format!("refs/remotes/origin/{name}"), tip, true, "test")
        .unwrap();
    test_repo
        .repo
        .find_branch("scratch", git2::BranchType::Local)
        .unwrap()
        .delete()
        .unwrap();
}

#[test]
fn merge_of_a_remote_branch_creates_its_local_branch() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");

    test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap();

    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    assert!(info.branches.iter().any(|b| b.name == "feature-r"));
}

/// The tracking branch a remote pick creates must not outlive a merge git
/// refused: an untracked file the merge would overwrite stops it here.
#[test]
fn a_refused_merge_of_a_remote_branch_deletes_its_local_branch() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    test_repo.write_file("r.txt", "untracked");
    let head = test_repo.head_oid();

    let result = test_repo.in_dir(|| super::run(Some("origin/feature-r".to_string()), true));

    assert!(result.is_err());
    assert!(!test_repo.branch_exists("feature-r"));
    assert_eq!(test_repo.head_oid(), head);
    assert_eq!(test_repo.read_file("r.txt"), "untracked");
}

#[test]
fn aborting_a_paused_merge_of_a_remote_branch_deletes_its_local_branch() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "shared.txt", "remote");
    test_repo.write_file("shared.txt", "integration");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Integration change");
    let head = test_repo.head_oid();

    test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap();
    assert!(crate::git::merge_is_in_progress(test_repo.repo.path()));

    crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();

    assert!(!test_repo.branch_exists("feature-r"));
    assert_eq!(test_repo.head_oid(), head);
}

/// `git merge` refuses a staged change wherever it is, with nothing loom
/// could point at; loom says why before it creates anything.
#[test]
fn merge_refuses_staged_changes() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    test_repo.write_file("other.txt", "staged");
    test_repo.stage_files(&["other.txt"]);

    let err = test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap_err();

    assert!(err.to_string().contains("staged changes"), "{err}");
    assert!(!test_repo.branch_exists("feature-r"));
}

/// `git merge` of a branch HEAD already holds, merged upstream say, makes no
/// commit and succeeds, so loom would report a merge that did nothing.
#[test]
fn merge_refuses_a_branch_head_already_holds() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo
        .repo
        .reference("refs/remotes/origin/old", base_oid, true, "test")
        .unwrap();
    let head = test_repo.head_oid();

    let err = test_repo
        .in_dir(|| super::run(Some("origin/old".to_string()), true))
        .unwrap_err();

    assert!(err.to_string().contains("already in"), "{err}");
    assert_eq!(test_repo.head_oid(), head);
    assert!(!test_repo.branch_exists("old"));
}

/// The picker reads which branches HEAD holds from one `for-each-ref`, which
/// must match the full ref names it lists.
#[test]
fn the_picker_leaves_out_a_remote_branch_head_already_holds() {
    use crate::core::ui::{self, Answer, PromptKind, Request};

    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo
        .repo
        .reference("refs/remotes/origin/old", base_oid, true, "test")
        .unwrap();

    let kinds = test_repo.in_dir(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            ui::install(tx);
            let result = super::run(None, true);
            ui::uninstall();
            result
        });
        let mut kinds = Vec::new();
        for request in rx {
            if let Request::Prompt { kind, reply, .. } = request {
                kinds.push(kind);
                reply
                    .send(Some(Answer::Text("origin/feature-r".into())))
                    .unwrap();
            }
        }
        worker.join().unwrap().unwrap();
        kinds
    });

    let [PromptKind::GroupedSelect { groups }] = kinds.as_slice() else {
        panic!("{kinds:?}");
    };
    let items: Vec<&str> = groups
        .iter()
        .flat_map(|(_, items)| items.iter().map(String::as_str))
        .collect();
    assert_eq!(items, ["origin/feature-r"]);
    assert!(test_repo.branch_exists("feature-r"));
}

#[test]
fn merge_accepts_branch_already_upstream_in_other_worktree() {
    let (test_repo, _path) = woven_in_other_worktree();
    test_repo.push_branch_to_remote_main("feature-a");

    let result = test_repo.in_dir(|| super::run(Some("feature-a".to_string()), false));
    assert!(result.is_ok(), "merge failed: {:?}", result.err());
}
