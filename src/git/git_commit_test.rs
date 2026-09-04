use crate::core::test_helpers::TestRepo;
use crate::git;

/// Delete `path` and stage the deletion, without going through the code under
/// test.
fn stage_deletion(test_repo: &TestRepo, path: &str) {
    let workdir = test_repo.workdir();
    std::fs::remove_file(workdir.join(path)).unwrap();
    git::run_git(workdir.as_path(), &["add", "-A", "--", path]).unwrap();
}

/// A staged deletion matches no pathspec, so `git add` would fail on it.
/// Staging it again is a no-op, not an error.
#[test]
fn stage_files_accepts_already_staged_deletion() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    stage_deletion(&test_repo, "file1.txt");

    let result = git::stage_files(test_repo.workdir().as_path(), &["file1.txt"]);

    assert!(result.is_ok(), "staging failed: {:?}", result);
    assert_eq!(test_repo.status_porcelain().trim(), "D  file1.txt");
}

/// A deletion that is not staged yet still has to be staged.
#[test]
fn stage_files_stages_unstaged_deletion() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    std::fs::remove_file(test_repo.workdir().join("file1.txt")).unwrap();

    git::stage_files(test_repo.workdir().as_path(), &["file1.txt"]).unwrap();

    assert_eq!(test_repo.status_porcelain().trim(), "D  file1.txt");
}

/// The skip applies per file: the rest of the batch is still staged.
#[test]
fn stage_files_stages_the_rest_of_the_batch() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.commit("Second commit", "file2.txt");
    stage_deletion(&test_repo, "file1.txt");
    test_repo.write_file("file2.txt", "changed");

    git::stage_files(test_repo.workdir().as_path(), &["file1.txt", "file2.txt"]).unwrap();

    let status = test_repo.status_porcelain();
    assert!(status.contains("D  file1.txt"), "status: {status}");
    assert!(status.contains("M  file2.txt"), "status: {status}");
}

/// A path that exists nowhere is still an error — the skip must not swallow a
/// typo in a filename.
#[test]
fn stage_files_rejects_an_unknown_path() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");

    let result = git::stage_files(test_repo.workdir().as_path(), &["nope.txt"]);

    assert!(result.is_err(), "unknown path should not be accepted");
}

/// A broken symlink is a working tree entry, so it must be staged even though
/// the deletion of the file it replaces is already staged.
#[cfg(unix)]
#[test]
fn stage_files_stages_a_symlink_over_a_staged_deletion() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    stage_deletion(&test_repo, "file1.txt");
    std::os::unix::fs::symlink("nowhere", test_repo.workdir().join("file1.txt")).unwrap();

    git::stage_files(test_repo.workdir().as_path(), &["file1.txt"]).unwrap();

    assert_eq!(test_repo.status_porcelain().trim(), "T  file1.txt");
}

/// `stage_path` shares the skip with `stage_files`.
#[test]
fn stage_path_accepts_already_staged_deletion() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    stage_deletion(&test_repo, "file1.txt");

    let result = git::stage_path(test_repo.workdir().as_path(), "file1.txt");

    assert!(result.is_ok(), "staging failed: {:?}", result);
    assert_eq!(test_repo.status_porcelain().trim(), "D  file1.txt");
}
