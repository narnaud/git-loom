use std::path::PathBuf;

use crate::core::repo;
use crate::core::shortid::IdAllocator;
use crate::core::test_helpers::TestRepo;

#[test]
fn name_of_strips_main_dir_prefix() {
    assert_eq!(super::name_of("work-foo", "work"), "foo");
    assert_eq!(super::name_of("work-foo-bar", "work"), "foo-bar");
    assert_eq!(super::name_of("other", "work"), "other");
    assert_eq!(super::name_of("work-", "work"), "work-");
}

fn with_foo_worktree() -> (TestRepo, PathBuf) {
    let test_repo = TestRepo::new_with_remote();
    let path = test_repo.add_worktree("work-foo", &["-b", "integration-foo", "origin/main"]);
    (test_repo, path)
}

#[test]
fn entries_list_main_first_and_mark_current() {
    let (_test_repo, path) = with_foo_worktree();

    let entries = super::entries(&path).unwrap();

    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["work", "foo"]);
    assert!(entries[0].main && !entries[0].current);
    assert!(!entries[1].main && entries[1].current);
    assert_eq!(entries[1].branch.as_deref(), Some("integration-foo"));
}

#[test]
fn resolve_accepts_id_name_branch_and_path() {
    let (test_repo, path) = with_foo_worktree();
    let workdir = test_repo.workdir();
    let entries = super::entries(&workdir).unwrap();
    let id = super::ids(&test_repo.repo, &entries)
        .get_worktree("foo")
        .to_string();

    for arg in [
        id.as_str(),
        "foo",
        "integration-foo",
        path.to_str().unwrap(),
    ] {
        let entry = super::resolve(&test_repo.repo, &workdir, arg).unwrap();
        assert_eq!(entry.name, "foo", "resolving {arg}");
    }
    let err = super::resolve(&test_repo.repo, &workdir, "nope").unwrap_err();
    assert!(err.to_string().contains("not found"), "got: {err}");
}

#[test]
fn ids_leave_status_ids_unchanged() {
    // A worktree named like a woven branch competes for the same candidates;
    // the branch must keep the ID status gives it.
    let test_repo = TestRepo::new_with_remote();
    test_repo.commit("A1", "a1.txt");
    let oid = test_repo.head_oid();
    test_repo
        .in_dir(|| crate::branch::new::run(Some("feature-a".to_string()), Some(oid.to_string())))
        .unwrap();
    test_repo.add_worktree(
        "work-feature-a",
        &["-b", "integration-feature-a", "origin/main"],
    );

    let info = repo::gather_repo_info(&test_repo.repo, false, 1).unwrap();
    let status_id = IdAllocator::new(info.collect_entities())
        .get_branch("feature-a")
        .to_string();
    let entries = super::entries(&test_repo.workdir()).unwrap();
    let ids = super::ids(&test_repo.repo, &entries);

    assert_eq!(ids.get_branch("feature-a"), status_id);
    assert!(!ids.get_worktree("feature-a").is_empty());
    assert_ne!(ids.get_worktree("feature-a"), status_id);
}

#[test]
fn resolve_prefers_a_name_over_an_id_spelled_the_same() {
    // `a-z` takes the ID `az` before the worktree named `az` can.
    let test_repo = TestRepo::new_with_remote();
    test_repo.add_worktree("work-a-z", &["--detach", "origin/main"]);
    test_repo.add_worktree("work-az", &["--detach", "origin/main"]);
    let workdir = test_repo.workdir();
    let entries = super::entries(&workdir).unwrap();
    assert_eq!(
        super::ids(&test_repo.repo, &entries).get_worktree("a-z"),
        "az"
    );

    let entry = super::resolve(&test_repo.repo, &workdir, "az").unwrap();

    assert_eq!(entry.name, "az");
}

#[test]
fn resolve_refuses_a_name_two_worktrees_share() {
    let (test_repo, foo) = with_foo_worktree();
    let other = test_repo.add_worktree("elsewhere/foo", &["--detach", "origin/main"]);
    let workdir = test_repo.workdir();

    let err = super::resolve(&test_repo.repo, &workdir, "foo").unwrap_err();
    assert!(err.to_string().contains("ambiguous"), "got: {err}");

    for path in [&foo, &other] {
        let entry = super::resolve(&test_repo.repo, &workdir, path.to_str().unwrap()).unwrap();
        assert_eq!(super::canonical(&entry.path), super::canonical(path));
    }
}
