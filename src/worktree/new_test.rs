use std::path::{Path, PathBuf};

use git2::{BranchType, Repository};

use crate::core::test_helpers::TestRepo;

fn head_and_upstream(path: &Path) -> (String, String) {
    let repo = Repository::open(path).unwrap();
    let name = repo.head().unwrap().shorthand().unwrap().to_string();
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

fn sibling(test_repo: &TestRepo, dir: &str) -> PathBuf {
    test_repo.workdir().parent().unwrap().join(dir)
}

#[test]
fn new_creates_sibling_worktree_on_tracking_integration_branch() {
    let test_repo = TestRepo::new_with_remote();

    let result = test_repo.in_dir(|| super::run("foo".to_string()));
    assert!(result.is_ok(), "new failed: {:?}", result.err());

    assert_eq!(
        head_and_upstream(&sibling(&test_repo, "work-foo")),
        ("integration-foo".to_string(), "origin/main".to_string())
    );
    assert_eq!(test_repo.current_branch_name(), "integration");
}

#[test]
fn new_from_linked_worktree_places_it_beside_the_main_one() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.in_dir(|| super::run("foo".to_string())).unwrap();
    let foo = sibling(&test_repo, "work-foo");

    let result = test_repo.in_dir_path(&foo, || super::run("bar".to_string()));
    assert!(result.is_ok(), "new failed: {:?}", result.err());

    assert_eq!(
        head_and_upstream(&sibling(&test_repo, "work-bar")),
        ("integration-bar".to_string(), "origin/main".to_string())
    );
}

#[test]
fn new_refuses_existing_directory() {
    let test_repo = TestRepo::new_with_remote();
    std::fs::create_dir(sibling(&test_repo, "work-foo")).unwrap();

    let err = test_repo
        .in_dir(|| super::run("foo".to_string()))
        .unwrap_err();

    assert!(err.to_string().contains("already exists"), "got: {err}");
    assert!(!test_repo.branch_exists("integration-foo"));
}

#[test]
fn new_refuses_taken_branch() {
    let test_repo = TestRepo::new_with_remote();
    test_repo.create_branch("integration-foo");

    let err = test_repo
        .in_dir(|| super::run("foo".to_string()))
        .unwrap_err();

    assert!(err.to_string().contains("already exists"), "got: {err}");
    assert!(!sibling(&test_repo, "work-foo").exists());
}

#[test]
fn new_refuses_path_separator() {
    let test_repo = TestRepo::new_with_remote();

    let err = test_repo
        .in_dir(|| super::run("a/b".to_string()))
        .unwrap_err();

    assert!(err.to_string().contains("path separator"), "got: {err}");
}

#[test]
fn new_refuses_a_main_worktree_that_is_a_bare_repository() {
    let test_repo = TestRepo::new_with_remote();
    let parent = test_repo.workdir().parent().unwrap().to_path_buf();
    let bare = parent.join("bare.git").display().to_string();
    let linked = parent.join("linked");
    let workdir = test_repo.workdir().display().to_string();
    crate::git::run_git(&parent, &["clone", "-q", "--bare", &workdir, &bare]).unwrap();
    let linked_arg = linked.display().to_string();
    crate::git::run_git(
        Path::new(&bare),
        &["worktree", "add", "-q", "--detach", &linked_arg],
    )
    .unwrap();

    let err = test_repo
        .in_dir_path(&linked, || super::run("foo".to_string()))
        .unwrap_err();

    assert!(
        err.to_string().contains("not the main checkout"),
        "got: {err}"
    );
    assert!(!parent.join("bare.git-foo").exists());
}

#[test]
fn new_that_git_fails_leaves_no_branch_behind() {
    // A registered worktree whose directory is gone passes the `exists` check,
    // and git refuses the path only after creating the branch.
    let test_repo = TestRepo::new_with_remote();
    let stale = test_repo.add_worktree("work-foo", &["--detach", "origin/main"]);
    std::fs::remove_dir_all(&stale).unwrap();

    let result = test_repo.in_dir(|| super::run("foo".to_string()));

    assert!(result.is_err());
    assert!(!test_repo.branch_exists("integration-foo"));
}

#[test]
fn new_from_a_pushed_feature_branch_tracks_the_main_worktree_upstream() {
    let test_repo = TestRepo::new_with_remote();
    let workdir = test_repo.workdir();
    crate::git::run_git(
        &workdir,
        &["update-ref", "refs/remotes/origin/feat", "HEAD"],
    )
    .unwrap();
    crate::git::run_git(
        &workdir,
        &["branch", "-q", "--track", "feat", "origin/feat"],
    )
    .unwrap();
    let feat = test_repo.add_worktree("work-feat", &["feat"]);

    test_repo
        .in_dir_path(&feat, || super::run("bar".to_string()))
        .unwrap();

    assert_eq!(
        head_and_upstream(&sibling(&test_repo, "work-bar")),
        ("integration-bar".to_string(), "origin/main".to_string())
    );
}
