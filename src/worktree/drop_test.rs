use std::path::PathBuf;

use crate::core::test_helpers::TestRepo;
use crate::core::transaction::{self, LoomState, Rollback};

fn with_foo_worktree() -> (TestRepo, PathBuf) {
    let test_repo = TestRepo::new_with_remote();
    let path = test_repo.add_worktree("work-foo", &["-b", "integration-foo", "origin/main"]);
    (test_repo, path)
}

#[test]
fn drop_removes_worktree_and_branch() {
    let (test_repo, path) = with_foo_worktree();

    let result = test_repo.in_dir(|| super::run("foo".to_string()));
    assert!(result.is_ok(), "drop failed: {:?}", result.err());

    assert!(!path.exists());
    assert!(!test_repo.branch_exists("integration-foo"));
}

#[test]
fn drop_keeps_branch_holding_unique_commits() {
    let (test_repo, path) = with_foo_worktree();
    std::fs::write(path.join("loose.txt"), "x").unwrap();
    crate::git::run_git(&path, &["add", "loose.txt"]).unwrap();
    crate::git::run_git(&path, &["commit", "-q", "-m", "Loose"]).unwrap();

    let result = test_repo.in_dir(|| super::run("foo".to_string()));
    assert!(result.is_ok(), "drop failed: {:?}", result.err());

    assert!(!path.exists());
    assert!(test_repo.branch_exists("integration-foo"));
}

#[test]
fn drop_deletes_branch_whose_commits_live_on_feature_branches() {
    // integration-foo holds only the weave merge of feature-a.
    let (test_repo, path) = with_foo_worktree();
    let base = test_repo.find_remote_branch_target("origin/main");
    test_repo.create_branch_at("feature-a", &base.to_string());
    test_repo.switch_branch("feature-a");
    test_repo.commit("A1", "a1.txt");
    test_repo.switch_branch("integration");
    test_repo
        .in_dir_path(&path, || {
            crate::branch::merge::run(Some("feature-a".to_string()), false)
        })
        .unwrap();

    test_repo.in_dir(|| super::run("foo".to_string())).unwrap();

    assert!(!test_repo.branch_exists("integration-foo"));
    assert!(test_repo.branch_exists("feature-a"));
}

#[test]
fn drop_refuses_untracked_changes() {
    let (test_repo, path) = with_foo_worktree();
    std::fs::write(path.join("new.txt"), "x").unwrap();

    let err = test_repo
        .in_dir(|| super::run("foo".to_string()))
        .unwrap_err();

    assert!(err.to_string().contains("changes"), "got: {err}");
    assert!(path.join("new.txt").exists());
}

#[test]
fn drop_refuses_paused_operation() {
    let (test_repo, path) = with_foo_worktree();
    let git_dir = git2::Repository::open(&path).unwrap().path().to_path_buf();
    let state = LoomState {
        command: "merge".to_string(),
        rollback: Rollback::default(),
        protect: Vec::new(),
        targets: Vec::new(),
        context: serde_json::Value::Null,
    };
    transaction::save(&git_dir, &state).unwrap();

    let err = test_repo
        .in_dir(|| super::run("foo".to_string()))
        .unwrap_err();

    assert!(err.to_string().contains("in progress"), "got: {err}");
    assert!(path.exists());
}

#[test]
fn drop_refuses_current_and_main_worktrees() {
    let (test_repo, path) = with_foo_worktree();

    let err = test_repo
        .in_dir_path(&path, || super::run("foo".to_string()))
        .unwrap_err();
    assert!(err.to_string().contains("you are in"), "got: {err}");

    let err = test_repo
        .in_dir_path(&path, || super::run("work".to_string()))
        .unwrap_err();
    assert!(err.to_string().contains("main worktree"), "got: {err}");
    assert!(path.exists());
}

#[test]
fn drop_keeps_a_checked_out_branch_it_did_not_create() {
    // feature-a is woven into integration, so no commit is unique to it: the
    // commit-count rule alone would delete it.
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    let oid = test_repo.head_oid();
    test_repo
        .in_dir(|| crate::branch::new::run(Some("feature-a".to_string()), Some(oid.to_string())))
        .unwrap();
    let path = test_repo.add_worktree("work-foo", &["feature-a"]);

    test_repo.in_dir(|| super::run("foo".to_string())).unwrap();

    assert!(!path.exists());
    assert!(test_repo.branch_exists("feature-a"));
}

#[test]
fn drop_refuses_detached_head_with_loose_commits() {
    let test_repo = TestRepo::new_with_remote();
    let path = test_repo.add_worktree("work-foo", &["--detach", "origin/main"]);
    std::fs::write(path.join("loose.txt"), "x").unwrap();
    crate::git::run_git(&path, &["add", "loose.txt"]).unwrap();
    crate::git::run_git(&path, &["commit", "-q", "-m", "Loose"]).unwrap();

    let err = test_repo
        .in_dir(|| super::run("foo".to_string()))
        .unwrap_err();

    assert!(err.to_string().contains("detached HEAD"), "got: {err}");
    assert!(path.exists());
}

#[test]
fn drop_removes_detached_head_on_a_remote_commit() {
    let test_repo = TestRepo::new_with_remote();
    let path = test_repo.add_worktree("work-foo", &["--detach", "origin/main"]);

    test_repo.in_dir(|| super::run("foo".to_string())).unwrap();

    assert!(!path.exists());
}

#[test]
fn drop_removes_detached_head_on_a_tagged_commit() {
    let test_repo = TestRepo::new_with_remote();
    let path = test_repo.add_worktree("work-foo", &["--detach", "origin/main"]);
    std::fs::write(path.join("release.txt"), "x").unwrap();
    crate::git::run_git(&path, &["add", "release.txt"]).unwrap();
    crate::git::run_git(&path, &["commit", "-q", "-m", "Release"]).unwrap();
    crate::git::run_git(&path, &["tag", "v1"]).unwrap();

    test_repo.in_dir(|| super::run("foo".to_string())).unwrap();

    assert!(!path.exists());
}

#[test]
fn drop_refusal_hint_is_its_own_unindented_line() {
    let (test_repo, path) = with_foo_worktree();
    std::fs::write(path.join("new.txt"), "x").unwrap();

    let err = test_repo
        .in_dir(|| super::run("foo".to_string()))
        .unwrap_err();

    let msg = err.to_string();
    assert_eq!(msg.lines().count(), 2, "got: {msg}");
    assert!(msg.lines().all(|l| !l.starts_with(' ')), "got: {msg}");
}

#[test]
fn drop_sees_untracked_files_the_config_hides() {
    let (test_repo, path) = with_foo_worktree();
    crate::git::run_git(&path, &["config", "status.showUntrackedFiles", "no"]).unwrap();
    std::fs::write(path.join("notes.txt"), "x").unwrap();

    let err = test_repo
        .in_dir(|| super::run("foo".to_string()))
        .unwrap_err();

    assert!(err.to_string().contains("changes"), "got: {err}");
    assert!(path.join("notes.txt").exists());
}
