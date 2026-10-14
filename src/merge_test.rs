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

/// Stages a deletion, a new file and a modification, and leaves another
/// modification unstaged: the shapes a set-aside-and-replay must all bring back.
/// Returns the porcelain status to expect back.
fn dirty_work(test_repo: &TestRepo) -> String {
    test_repo.write_file("d.txt", "d\n");
    test_repo.write_file("s.txt", "s\n");
    test_repo.write_file("w.txt", "w\n");
    test_repo.stage_files(&["d.txt", "s.txt", "w.txt"]);
    test_repo.commit_staged("Tracked files");
    std::fs::remove_file(test_repo.workdir().join("d.txt")).unwrap();
    test_repo.write_file("n.txt", "n\n");
    test_repo.write_file("s.txt", "s\nstaged\n");
    test_repo.stage_files(&["d.txt", "n.txt", "s.txt"]);
    test_repo.write_file("w.txt", "w\nunstaged\n");
    let status = test_repo.status_porcelain();
    assert_eq!(status, "D  d.txt\nA  n.txt\nM  s.txt\n M w.txt\n");
    status
}

/// `feature-c` and integration each change `shared.txt`, so merging conflicts.
fn conflicting_branch(test_repo: &TestRepo) {
    let base_oid = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-c", &base_oid.to_string());
    test_repo.write_file("shared.txt", "integration");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Integration change");
    test_repo.switch_branch("feature-c");
    test_repo.write_file("shared.txt", "feature");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Feature change");
    test_repo.switch_branch("integration");
}

/// `git merge` refuses a staged change wherever it is; the merge sets the work
/// aside, and the replay over the merge would leave it unstaged (#421).
#[test]
fn merge_keeps_staged_and_unstaged_changes() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    let status = dirty_work(&test_repo);

    test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap();

    assert_eq!(test_repo.head_commit().parent_count(), 2);
    assert_eq!(test_repo.status_porcelain(), status);
}

/// An unstaged change to a file the merge also changes, in other lines: git
/// alone refuses to overwrite it.
#[test]
fn merge_keeps_unstaged_change_to_a_merged_file() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("m.txt", "1\n2\n3\n4\n5\n");
    test_repo.stage_files(&["m.txt"]);
    test_repo.commit_staged("Add m");
    let base = test_repo.head_oid();
    test_repo.create_branch_at("feature-m", &base.to_string());
    test_repo.switch_branch("feature-m");
    test_repo.write_file("m.txt", "1 feature\n2\n3\n4\n5\n");
    test_repo.stage_files(&["m.txt"]);
    test_repo.commit_staged("Change m");
    test_repo.switch_branch("integration");
    test_repo.write_file("m.txt", "1\n2\n3\n4\n5 local\n");

    test_repo
        .in_dir(|| super::run(Some("feature-m".to_string()), false))
        .unwrap();

    assert_eq!(test_repo.head_commit().parent_count(), 2);
    assert_eq!(
        test_repo.read_file("m.txt"),
        "1 feature\n2\n3\n4\n5 local\n"
    );
    assert_eq!(test_repo.status_porcelain(), " M m.txt\n");
}

/// `feature-m` changes the first line of `m.txt`; `also_conflicting` makes it
/// change `shared.txt` against integration as well, so the merge itself stops.
fn overlapping_branch(test_repo: &TestRepo, also_conflicting: bool) {
    test_repo.write_file("m.txt", "1\n");
    test_repo.stage_files(&["m.txt"]);
    test_repo.commit_staged("Add m");
    let base = test_repo.head_oid();
    test_repo.create_branch_at("feature-m", &base.to_string());
    test_repo.switch_branch("feature-m");
    test_repo.write_file("m.txt", "1 feature\n");
    test_repo.stage_files(&["m.txt"]);
    test_repo.commit_staged("Change m");
    if also_conflicting {
        test_repo.write_file("shared.txt", "feature");
        test_repo.stage_files(&["shared.txt"]);
        test_repo.commit_staged("Feature shared");
        test_repo.switch_branch("integration");
        test_repo.write_file("shared.txt", "integration");
        test_repo.stage_files(&["shared.txt"]);
        test_repo.commit_staged("Integration shared");
    } else {
        test_repo.switch_branch("integration");
    }
}

/// [`dirty_work`] plus an unstaged change overlapping `feature-m`'s.
fn dirty_work_overlapping(test_repo: &TestRepo) -> String {
    dirty_work(test_repo);
    test_repo.write_file("m.txt", "1 local\n");
    test_repo.status_porcelain()
}

fn merge_feature_m(test_repo: &TestRepo) {
    test_repo
        .in_dir(|| super::run(Some("feature-m".to_string()), false))
        .unwrap();
}

fn continue_merge(test_repo: &TestRepo) {
    crate::core::transaction::continue_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();
}

fn state_exists(test_repo: &TestRepo) -> bool {
    test_repo.repo.path().join("loom/state.json").exists()
}

fn stash_list(test_repo: &TestRepo) -> String {
    crate::git::run_git_stdout(&test_repo.workdir(), &["stash", "list"]).unwrap()
}

/// The merge lands, then replaying the set-aside work conflicts with it:
/// paused, and an abort takes the merge back and returns that work exactly,
/// staging included.
#[test]
fn merge_with_overlapping_change_pauses_and_aborts_to_the_exact_state() {
    let test_repo = TestRepo::new_with_remote();
    overlapping_branch(&test_repo, false);
    let status = dirty_work_overlapping(&test_repo);
    let head = test_repo.head_oid();

    merge_feature_m(&test_repo);

    assert!(state_exists(&test_repo));
    assert_eq!(test_repo.head_commit().parent_count(), 2);
    assert!(crate::git::has_unmerged_paths(&test_repo.workdir()));

    crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();

    assert!(!state_exists(&test_repo));
    assert_eq!(test_repo.head_oid(), head);
    assert_eq!(test_repo.status_porcelain(), status);
    assert_eq!(test_repo.read_file("m.txt"), "1 local\n");
}

#[test]
fn merge_with_overlapping_change_continues_once_resolved() {
    let test_repo = TestRepo::new_with_remote();
    overlapping_branch(&test_repo, false);
    // A stash of the user's own, which the merge must leave alone.
    test_repo.write_file("m.txt", "mine\n");
    crate::git::run_git(&test_repo.workdir(), &["stash", "push", "-q"]).unwrap();
    let status = dirty_work_overlapping(&test_repo);
    merge_feature_m(&test_repo);

    continue_merge(&test_repo);
    assert!(state_exists(&test_repo), "nothing resolved yet");
    assert!(crate::git::has_unmerged_paths(&test_repo.workdir()));

    test_repo.write_file("m.txt", "1 both\n");
    test_repo.stage_files(&["m.txt"]);
    continue_merge(&test_repo);

    assert!(!state_exists(&test_repo));
    assert_eq!(test_repo.head_commit().parent_count(), 2);
    assert_eq!(test_repo.read_file("m.txt"), "1 both\n");
    assert_eq!(test_repo.read_file("w.txt"), "w\nunstaged\n");
    // A conflicted replay stages what it merged cleanly; staging is the
    // user's again, `m.txt` unstaged as it was.
    assert_eq!(test_repo.status_porcelain(), status);
    assert_eq!(
        stash_list(&test_repo).lines().count(),
        1,
        "the user's stash"
    );
}

/// A submodule checked out at another commit is not set aside — the reset would
/// not move it, and replaying its pointer would fail — so it simply stays.
#[test]
fn merge_leaves_a_moved_submodule_alone() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    let (_, second) = test_repo.add_submodule("sub");
    test_repo.commit_staged("Add sub");
    test_repo.checkout_submodule("sub", second);
    test_repo.write_file("w.txt", "w\n");
    test_repo.stage_files(&["w.txt"]);
    let status = test_repo.status_porcelain();
    assert!(status.contains(" M sub"), "{status}");

    test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap();

    assert_eq!(test_repo.head_commit().parent_count(), 2);
    assert_eq!(test_repo.status_porcelain(), status);
}

/// A binary file both sides change cannot carry markers: git leaves the merge's
/// version in the tree, which history holds. The user's, held nowhere else, is
/// put there instead, so staging it and continuing keeps it.
#[test]
fn merge_keeps_your_version_of_a_conflicted_binary() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("b.bin", "a\0base\n");
    test_repo.stage_files(&["b.bin"]);
    test_repo.commit_staged("Add b");
    let base = test_repo.head_oid();
    test_repo.create_branch_at("feature-b", &base.to_string());
    test_repo.switch_branch("feature-b");
    test_repo.write_file("b.bin", "a\0theirs\n");
    test_repo.stage_files(&["b.bin"]);
    test_repo.commit_staged("Change b");
    test_repo.switch_branch("integration");
    test_repo.write_file("b.bin", "a\0mine\n");

    test_repo
        .in_dir(|| super::run(Some("feature-b".to_string()), false))
        .unwrap();

    assert!(state_exists(&test_repo));
    assert!(crate::git::has_unmerged_paths(&test_repo.workdir()));
    assert_eq!(test_repo.read_file("b.bin"), "a\0mine\n");

    test_repo.stage_files(&["b.bin"]);
    continue_merge(&test_repo);
    assert!(!state_exists(&test_repo));
    assert_eq!(test_repo.read_file("b.bin"), "a\0mine\n");
    assert_eq!(test_repo.status_porcelain(), " M b.bin\n");
}

/// `submodule.recurse` must not carry the reset into a submodule, wiping the
/// work inside it, which the set-aside does not hold.
#[test]
fn merge_keeps_work_inside_a_submodule_with_submodule_recurse() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    // A real submodule, its git directory under `.git/modules`: the recursion
    // passes an embedded repository like `add_submodule`'s by.
    let source = TestRepo::new();
    source.commit("Sub", "s.txt");
    crate::git::run_git(
        &test_repo.workdir(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &source.workdir().to_string_lossy(),
            "sub",
        ],
    )
    .unwrap();
    test_repo.commit_staged("Add sub");
    test_repo.set_config("submodule.recurse", "true");
    test_repo.write_file("sub/s.txt", "edited inside");
    test_repo.write_file("w.txt", "w\n");
    test_repo.stage_files(&["w.txt"]);

    test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap();

    assert_eq!(test_repo.head_commit().parent_count(), 2);
    assert_eq!(test_repo.read_file("sub/s.txt"), "edited inside");
}

/// The reset would end the cherry-pick or revert the user is in.
#[test]
fn merge_refuses_during_a_cherry_pick() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    let pick = test_repo.repo.path().join("CHERRY_PICK_HEAD");
    std::fs::write(&pick, format!("{}\n", test_repo.head_oid())).unwrap();
    test_repo.write_file("w.txt", "w\n");
    test_repo.stage_files(&["w.txt"]);

    let err = test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap_err();

    assert!(err.to_string().contains("cherry-pick"), "{err}");
    assert!(pick.exists());
    assert_eq!(test_repo.status_porcelain(), "A  w.txt\n");
}

/// A clean replay that landed before its state write was lost: the patch is
/// already in the tree, and redoing it would fail on its staged deletion.
#[test]
fn merge_continue_does_not_redo_a_replay_already_in_the_tree() {
    let test_repo = TestRepo::new_with_remote();
    conflicting_branch(&test_repo);
    let status = dirty_work(&test_repo);
    test_repo
        .in_dir(|| super::run(Some("feature-c".to_string()), false))
        .unwrap();
    test_repo.write_file("shared.txt", "resolved");
    test_repo.stage_files(&["shared.txt"]);
    crate::git::continue_merge(&test_repo.workdir(), test_repo.repo.path()).unwrap();
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(test_repo.repo.path().join("loom/state.json")).unwrap(),
    )
    .unwrap();
    let patch = state["rollback"]["saved_worktree_patch"].as_str().unwrap();
    assert!(crate::git::replay_patch_three_way(&test_repo.workdir(), patch).unwrap());

    continue_merge(&test_repo);

    assert!(!state_exists(&test_repo));
    assert_eq!(test_repo.head_commit().parent_count(), 2);
    assert_eq!(test_repo.status_porcelain(), status);
}

/// A staged submodule pointer is the one change the set-aside cannot bring
/// back, the reset unstaging it: refused up front.
#[test]
fn merge_refuses_a_staged_submodule_change() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    let (_, second) = test_repo.add_submodule("sub");
    test_repo.commit_staged("Add sub");
    test_repo.checkout_submodule("sub", second);
    test_repo.stage_files(&["sub"]);
    let status = test_repo.status_porcelain();
    let head = test_repo.head_oid();

    let err = test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap_err();

    assert!(err.to_string().contains("`sub`"), "{err}");
    assert_eq!(test_repo.head_oid(), head);
    assert_eq!(test_repo.status_porcelain(), status);
    assert!(!state_exists(&test_repo));
}

/// Resolving, a file staged and then edited again: `git merge --abort` refuses
/// that, so the abort must not depend on it.
#[test]
fn merge_abort_gets_past_a_file_staged_then_edited_while_paused() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Unrelated", "u.txt");
    conflicting_branch(&test_repo);
    let status = dirty_work(&test_repo);
    let head = test_repo.head_oid();
    test_repo
        .in_dir(|| super::run(Some("feature-c".to_string()), false))
        .unwrap();
    test_repo.write_file("u.txt", "staged");
    test_repo.stage_files(&["u.txt"]);
    test_repo.write_file("u.txt", "edited again");

    crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();

    assert!(!state_exists(&test_repo));
    assert!(!crate::git::merge_is_in_progress(test_repo.repo.path()));
    assert_eq!(test_repo.head_oid(), head);
    assert_eq!(test_repo.status_porcelain(), status);
}

/// `loom abort` returns to where the transaction started, whatever was done
/// meanwhile: here an edit to a file the merge leaves alone.
#[test]
fn merge_abort_returns_to_the_start_of_the_transaction() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("Unrelated", "u.txt");
    conflicting_branch(&test_repo);
    let status = dirty_work(&test_repo);
    let head = test_repo.head_oid();

    test_repo
        .in_dir(|| super::run(Some("feature-c".to_string()), false))
        .unwrap();
    test_repo.write_file("u.txt", "edited while paused");
    crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();

    assert_eq!(test_repo.head_oid(), head);
    assert_eq!(test_repo.status_porcelain(), status);
}

/// An edit during the conflict pause to a file whose own change was set aside:
/// replaying that change over it on `loom continue` conflicts, and the merge
/// stays paused until it is resolved.
#[test]
fn merge_continue_replays_over_an_edit_made_while_paused() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("t.txt", "t\n");
    test_repo.stage_files(&["t.txt"]);
    test_repo.commit_staged("Tracked");
    conflicting_branch(&test_repo);
    test_repo.write_file("t.txt", "local\n");

    test_repo
        .in_dir(|| super::run(Some("feature-c".to_string()), false))
        .unwrap();
    assert_eq!(test_repo.read_file("t.txt"), "t\n", "set aside");
    test_repo.write_file("shared.txt", "resolved");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.write_file("t.txt", "edited while paused\n");
    continue_merge(&test_repo);

    assert!(state_exists(&test_repo));
    assert!(crate::git::has_unmerged_paths(&test_repo.workdir()));
    let content = test_repo.read_file("t.txt");
    assert!(
        content.contains("local") && content.contains("edited while paused"),
        "{content}"
    );

    test_repo.write_file("t.txt", "both\n");
    test_repo.stage_files(&["t.txt"]);
    continue_merge(&test_repo);
    assert!(!state_exists(&test_repo));
    assert_eq!(test_repo.head_commit().parent_count(), 2);
    assert_eq!(test_repo.status_porcelain(), " M t.txt\n");
}

/// The user finishes the merge's own conflict with raw git instead of
/// `loom continue`: the set-aside work still comes back on `loom continue`,
/// and `loom abort` still returns to the start.
#[test]
fn merge_finished_with_raw_git_still_replays_or_aborts() {
    for abort in [false, true] {
        let test_repo = TestRepo::new_with_remote();
        conflicting_branch(&test_repo);
        let status = dirty_work(&test_repo);
        let head = test_repo.head_oid();

        test_repo
            .in_dir(|| super::run(Some("feature-c".to_string()), false))
            .unwrap();
        test_repo.write_file("shared.txt", "resolved");
        test_repo.stage_files(&["shared.txt"]);
        crate::git::continue_merge(&test_repo.workdir(), test_repo.repo.path()).unwrap();
        assert!(state_exists(&test_repo), "abort: {abort}");

        if abort {
            crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path())
                .unwrap();
            assert_eq!(test_repo.head_oid(), head);
        } else {
            continue_merge(&test_repo);
            assert_eq!(test_repo.head_commit().parent_count(), 2);
        }
        assert_eq!(test_repo.status_porcelain(), status, "abort: {abort}");
        assert!(!state_exists(&test_repo), "abort: {abort}");
    }
}

/// The merge stops on its own conflict first; `loom continue` then lands it,
/// and replaying the set-aside work conflicts: still paused, and either way
/// out works.
#[test]
fn merge_conflict_then_overlapping_change_stays_paused() {
    for abort in [false, true] {
        let test_repo = TestRepo::new_with_remote();
        overlapping_branch(&test_repo, true);
        let status = dirty_work_overlapping(&test_repo);
        let head = test_repo.head_oid();
        merge_feature_m(&test_repo);
        assert!(crate::git::merge_is_in_progress(test_repo.repo.path()));

        test_repo.write_file("shared.txt", "resolved");
        test_repo.stage_files(&["shared.txt"]);
        continue_merge(&test_repo);

        assert!(state_exists(&test_repo), "abort: {abort}");
        assert!(!crate::git::merge_is_in_progress(test_repo.repo.path()));
        assert!(crate::git::has_unmerged_paths(&test_repo.workdir()));

        if abort {
            crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path())
                .unwrap();
            assert_eq!(test_repo.head_oid(), head);
            assert_eq!(test_repo.status_porcelain(), status);
        } else {
            test_repo.write_file("m.txt", "1 both\n");
            test_repo.stage_files(&["m.txt"]);
            continue_merge(&test_repo);
            assert_eq!(test_repo.head_commit().parent_count(), 2);
        }
        assert!(!state_exists(&test_repo), "abort: {abort}");
    }
}

#[test]
fn merge_conflict_continue_restores_staging() {
    let test_repo = TestRepo::new_with_remote();
    conflicting_branch(&test_repo);
    let status = dirty_work(&test_repo);

    test_repo
        .in_dir(|| super::run(Some("feature-c".to_string()), false))
        .unwrap();
    test_repo.write_file("shared.txt", "resolved");
    test_repo.stage_files(&["shared.txt"]);
    crate::core::transaction::continue_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();

    assert_eq!(test_repo.head_commit().parent_count(), 2);
    assert_eq!(test_repo.status_porcelain(), status);
}

#[test]
fn merge_conflict_abort_restores_staging() {
    let test_repo = TestRepo::new_with_remote();
    conflicting_branch(&test_repo);
    let status = dirty_work(&test_repo);
    let head = test_repo.head_oid();

    test_repo
        .in_dir(|| super::run(Some("feature-c".to_string()), false))
        .unwrap();
    crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();

    assert_eq!(test_repo.head_oid(), head);
    assert_eq!(test_repo.status_porcelain(), status);
}

/// git refuses the merge after the work was set aside: an untracked file is in
/// the way of one the branch adds, and setting aside never takes untracked files.
#[test]
fn merge_refused_by_git_restores_staging() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    let status = dirty_work(&test_repo);
    test_repo.write_file("r.txt", "in the way");
    let head = test_repo.head_oid();

    test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap_err();

    assert_eq!(test_repo.head_oid(), head);
    assert_eq!(test_repo.status_porcelain(), format!("{status}?? r.txt\n"));
    assert!(!test_repo.branch_exists("feature-r"));
}

/// The index lock taken, setting the work aside fails before touching it:
/// the work stays as it was, and so does nothing to undo.
#[test]
fn merge_refused_before_setting_aside_leaves_work_alone() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    let status = dirty_work(&test_repo);
    let lock = test_repo.repo.path().join("index.lock");
    std::fs::write(&lock, "").unwrap();

    let result = test_repo.in_dir(|| super::run(Some("origin/feature-r".to_string()), true));
    std::fs::remove_file(&lock).unwrap();

    result.unwrap_err();
    assert_eq!(test_repo.status_porcelain(), status);
    let parked = std::fs::read_dir(test_repo.repo.path().join("loom"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(parked, 0, "nothing parked");
}

/// Staged, then undone in the tree: only the index differs from HEAD, which
/// `git merge` refuses just the same, so it is set aside too.
#[test]
fn merge_keeps_a_change_staged_then_undone_in_the_tree() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    test_repo.write_file("s.txt", "s\n");
    test_repo.stage_files(&["s.txt"]);
    test_repo.commit_staged("Add s");
    test_repo.write_file("s.txt", "staged\n");
    test_repo.stage_files(&["s.txt"]);
    test_repo.write_file("s.txt", "s\n");
    assert_eq!(test_repo.status_porcelain(), "MM s.txt\n");

    test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap();

    assert_eq!(test_repo.head_commit().parent_count(), 2);
    assert_eq!(test_repo.status_porcelain(), "MM s.txt\n");
}

/// The user aborts the merge with raw git: `loom continue` must not report a
/// merge that is not there, and `loom abort` still returns to the start.
#[test]
fn merge_aborted_with_raw_git_is_not_reported_done() {
    let test_repo = TestRepo::new_with_remote();
    conflicting_branch(&test_repo);
    let status = dirty_work(&test_repo);
    let head = test_repo.head_oid();
    test_repo
        .in_dir(|| super::run(Some("feature-c".to_string()), false))
        .unwrap();
    crate::git::merge_abort(&test_repo.workdir()).unwrap();

    let err = crate::core::transaction::continue_cmd(&test_repo.workdir(), test_repo.repo.path())
        .unwrap_err();
    assert!(err.to_string().contains("no longer at HEAD"), "{err}");
    assert!(state_exists(&test_repo));

    crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();
    assert_eq!(test_repo.head_oid(), head);
    assert_eq!(test_repo.status_porcelain(), status);
}

/// A file held open without write sharing makes `git reset --hard` fail
/// part-way, after resetting others: the state, the only copy of the work,
/// stays for `loom abort` to put it back.
#[cfg(windows)]
#[test]
fn merge_keeps_the_state_when_setting_aside_fails_part_way() {
    use std::os::windows::fs::OpenOptionsExt;

    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    test_repo.write_file("a.txt", "a\n");
    test_repo.write_file("z.txt", "z\n");
    test_repo.stage_files(&["a.txt", "z.txt"]);
    test_repo.commit_staged("Add a and z");
    test_repo.write_file("a.txt", "a edited\n");
    test_repo.write_file("z.txt", "z edited\n");
    let status = test_repo.status_porcelain();

    const FILE_SHARE_READ: u32 = 1;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(test_repo.workdir().join("z.txt"))
        .unwrap();
    let result = test_repo.in_dir(|| super::run(Some("origin/feature-r".to_string()), true));
    drop(lock);

    result.unwrap_err();
    if test_repo.read_file("a.txt") == "a edited\n" {
        // Nothing was reset after all: the state went with nothing to undo.
        assert!(!state_exists(&test_repo));
    } else {
        assert!(state_exists(&test_repo));
        crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();
    }
    assert_eq!(test_repo.status_porcelain(), status);
    assert_eq!(test_repo.read_file("a.txt"), "a edited\n");
}

/// The replay's conflicts landed but the state write recording them was lost:
/// `loom continue` must see them as the replay's, not stage the markers as
/// resolved and finish.
#[test]
fn merge_continue_after_a_lost_replay_write_stays_paused() {
    let test_repo = TestRepo::new_with_remote();
    overlapping_branch(&test_repo, false);
    dirty_work_overlapping(&test_repo);
    merge_feature_m(&test_repo);
    set_replayed(&test_repo, false);

    continue_merge(&test_repo);

    assert!(state_exists(&test_repo));
    assert!(crate::git::has_unmerged_paths(&test_repo.workdir()));
}

/// Rewrite the saved `replayed` flag, as a lost write leaves it.
fn set_replayed(test_repo: &TestRepo, replayed: bool) {
    let path = test_repo.repo.path().join("loom/state.json");
    let state = std::fs::read_to_string(&path).unwrap();
    let from = format!("\"replayed\": {}", !replayed);
    assert!(state.contains(&from), "{state}");
    std::fs::write(
        &path,
        state.replace(&from, &format!("\"replayed\": {replayed}")),
    )
    .unwrap();
}

/// Mark `path` unmerged in the index, the way a conflicted `git stash pop`
/// or `git checkout -m` leaves it.
fn make_unmerged(test_repo: &TestRepo, path: &str) {
    let workdir = test_repo.workdir();
    let blob = |content: &str| {
        test_repo.write_file(path, content);
        crate::git::run_git_stdout(&workdir, &["hash-object", "-w", path])
            .unwrap()
            .trim()
            .to_string()
    };
    let (base, ours, theirs) = (blob("base\n"), blob("ours\n"), blob("theirs\n"));
    test_repo.write_file(
        path,
        "<<<<<<< ours\nours\n=======\ntheirs\n>>>>>>> theirs\n",
    );
    let entries = format!(
        "0 0000000000000000000000000000000000000000\t{path}\n\
         100644 {base} 1\t{path}\n100644 {ours} 2\t{path}\n100644 {theirs} 3\t{path}\n"
    );
    let mut child = std::process::Command::new("git")
        .current_dir(&workdir)
        .args(["update-index", "--index-info"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(entries.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(crate::git::has_unmerged_paths(&workdir));
}

#[test]
fn merge_refuses_unresolved_conflicts() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "r.txt", "r");
    test_repo.commit("Q", "q.txt");
    make_unmerged(&test_repo, "q.txt");

    let err = test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap_err();

    assert!(err.to_string().contains("unresolved conflicts"), "{err}");
    assert!(!state_exists(&test_repo));
    assert!(crate::git::has_unmerged_paths(&test_repo.workdir()));
}

/// `git apply --3way` cannot replay a change to a file the merge deletes, nor a
/// deletion of one it changes: refused up front, with nothing touched.
#[test]
fn merge_refuses_a_change_to_a_file_the_branch_deletes() {
    for local_deletes in [false, true] {
        let test_repo = TestRepo::new_with_remote();
        test_repo.write_file("d.txt", "d\n");
        test_repo.stage_files(&["d.txt"]);
        test_repo.commit_staged("Add d");
        let base = test_repo.head_oid();
        test_repo.create_branch_at("feature-d", &base.to_string());
        test_repo.switch_branch("feature-d");
        if local_deletes {
            test_repo.write_file("d.txt", "d changed\n");
            test_repo.stage_files(&["d.txt"]);
        } else {
            crate::git::run_git(&test_repo.workdir(), &["rm", "-q", "d.txt"]).unwrap();
        }
        test_repo.commit_staged("Touch d");
        test_repo.switch_branch("integration");
        if local_deletes {
            std::fs::remove_file(test_repo.workdir().join("d.txt")).unwrap();
        } else {
            test_repo.write_file("d.txt", "d local\n");
        }
        let status = test_repo.status_porcelain();
        let head = test_repo.head_oid();

        let err = test_repo
            .in_dir(|| super::run(Some("feature-d".to_string()), false))
            .unwrap_err();

        assert!(err.to_string().contains("`d.txt`"), "{err}");
        assert_eq!(test_repo.head_oid(), head);
        assert_eq!(test_repo.status_porcelain(), status);
        assert!(!state_exists(&test_repo));
    }
}

/// A state written by a loom that left the work to `git merge`: no pre-merge
/// HEAD to reset to, and nothing set aside to replay.
#[test]
fn merge_continue_finishes_a_state_from_an_older_loom() {
    let test_repo = TestRepo::new_with_remote();
    conflicting_branch(&test_repo);
    test_repo
        .in_dir(|| super::run(Some("feature-c".to_string()), false))
        .unwrap();
    let path = test_repo.repo.path().join("loom/state.json");
    let mut state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    state["rollback"]["reset_hard_to"] = serde_json::Value::String(String::new());
    state["context"] = serde_json::json!({ "branch_name": "feature-c" });
    std::fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    test_repo.write_file("shared.txt", "resolved");
    test_repo.stage_files(&["shared.txt"]);

    continue_merge(&test_repo);

    assert!(!state_exists(&test_repo));
    assert_eq!(test_repo.head_commit().parent_count(), 2);
}

/// Both sides add `n.txt`: replaying the set-aside new file over the merged one
/// is an add/add conflict, paused like any other, and `loom abort` returns to
/// the start, tracking branch included.
#[test]
fn merge_with_both_sides_adding_a_file_pauses_and_aborts() {
    let test_repo = TestRepo::new_with_remote();
    remote_only_branch(&test_repo, "feature-r", "n.txt", "theirs");
    test_repo.write_file("n.txt", "mine");
    test_repo.stage_files(&["n.txt"]);
    let head = test_repo.head_oid();

    test_repo
        .in_dir(|| super::run(Some("origin/feature-r".to_string()), true))
        .unwrap();

    assert!(state_exists(&test_repo));
    assert!(crate::git::has_unmerged_paths(&test_repo.workdir()));
    crate::core::transaction::abort_cmd(&test_repo.workdir(), test_repo.repo.path()).unwrap();
    assert_eq!(test_repo.head_oid(), head);
    assert_eq!(test_repo.status_porcelain(), "A  n.txt\n");
    assert_eq!(test_repo.read_file("n.txt"), "mine");
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
