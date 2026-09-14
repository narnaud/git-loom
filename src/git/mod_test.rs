use super::resolve_loom_exe;

/// An installed binary is itself: nothing above it is a `deps` directory. The
/// path need not exist — this branch never looks at the filesystem.
#[test]
fn an_installed_binary_resolves_to_itself() {
    let exe = std::path::Path::new("/usr/local/bin/git-loom");

    assert_eq!(resolve_loom_exe(exe).unwrap(), exe);
}

/// A test harness resolves to the real binary one level up.
#[test]
fn a_test_harness_resolves_to_the_built_binary() {
    let dir = tempfile::tempdir().unwrap();
    let deps = dir.path().join("deps");
    std::fs::create_dir(&deps).unwrap();
    let built = dir
        .path()
        .join(format!("git-loom{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&built, "").unwrap();

    let resolved = resolve_loom_exe(&deps.join("git_loom-0123456789abcdef")).unwrap();

    assert_eq!(resolved, built);
}

/// Without that binary the harness must not be offered in its place: git would
/// run it as the rebase sequence editor and get an argument parser instead.
#[test]
fn a_test_harness_without_the_built_binary_errors() {
    let dir = tempfile::tempdir().unwrap();
    let deps = dir.path().join("deps");
    std::fs::create_dir(&deps).unwrap();
    let harness = deps.join("git_loom-0123456789abcdef");

    let err = resolve_loom_exe(&harness).expect_err("the built binary is missing");

    let msg = err.to_string();
    assert!(msg.contains("git-loom"), "unexpected error: {msg}");
    assert!(msg.contains("cargo build"), "unexpected error: {msg}");
    assert!(
        !msg.contains("git_loom-0123456789abcdef"),
        "the harness path is not the one to build: {msg}"
    );
}

/// Paths with nothing above them to inspect: a root, which has no parent at
/// all, and a bare name, whose parent is empty rather than absent.
#[test]
fn a_path_with_no_directory_above_it_resolves_to_itself() {
    for exe in [std::path::Path::new("/"), std::path::Path::new("git-loom")] {
        assert_eq!(resolve_loom_exe(exe).unwrap(), exe, "for {}", exe.display());
    }
}

/// Both sides of the record count, so the commits that add and remove a
/// submodule are recognised as well as the one that bumps it.
#[test]
fn commit_gitlinks_covers_add_bump_and_remove() {
    use crate::core::test_helpers::TestRepo;

    let test_repo = TestRepo::new();
    let (_first, second) = test_repo.add_submodule("Data");
    test_repo.write_file("plain.txt", "plain");
    test_repo.stage_files(&["plain.txt"]);
    test_repo.commit_staged("Add submodule");
    let added = test_repo.head_oid().to_string();

    test_repo.checkout_submodule("Data", second);
    test_repo.stage_files(&["Data"]);
    test_repo.commit_staged("Bump submodule");
    let bumped = test_repo.head_oid().to_string();

    crate::git::run_git(
        &test_repo.workdir(),
        &["rm", "-r", "-q", "--cached", "Data"],
    )
    .unwrap();
    test_repo.commit_staged("Remove submodule");
    let removed = test_repo.head_oid().to_string();

    let workdir = test_repo.workdir();
    for (label, oid, removes) in [
        ("added", &added, false),
        ("bumped", &bumped, false),
        ("removed", &removed, true),
    ] {
        let gitlinks = super::commit_gitlinks(&workdir, oid).unwrap();
        assert_eq!(
            gitlinks.get("Data"),
            Some(&removes),
            "{label} commit read the gitlink wrong"
        );
        assert!(
            !gitlinks.contains_key("plain.txt"),
            "{label} commit took a file"
        );
    }
}
