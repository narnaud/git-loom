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

/// A path is never a glob: staging `a[12].txt` must leave `a1.txt` alone.
#[test]
fn stage_path_takes_the_path_literally() {
    let test_repo = TestRepo::new();
    test_repo.commit("Add a1", "a1.txt");
    test_repo.commit("Add a[12]", "a[12].txt");
    test_repo.write_file("a1.txt", "changed");
    test_repo.write_file("a[12].txt", "changed");

    git::stage_path(test_repo.workdir().as_path(), "a[12].txt").unwrap();

    assert_eq!(test_repo.status_porcelain(), " M a1.txt\nM  a[12].txt\n");
}

/// Restoring `a[12].txt` must not discard the changes in `a1.txt`.
#[test]
fn restore_files_to_head_takes_the_path_literally() {
    let test_repo = TestRepo::new();
    test_repo.commit("Add a1", "a1.txt");
    test_repo.commit("Add a[12]", "a[12].txt");
    test_repo.write_file("a1.txt", "changed");
    test_repo.write_file("a[12].txt", "changed");

    git::restore_files_to_head(test_repo.workdir().as_path(), &["a[12].txt"]).unwrap();

    assert_eq!(test_repo.status_porcelain(), " M a1.txt\n");
}

/// `--porcelain` and its siblings make `git commit` print the status and exit
/// 0 without committing; the callers rewrite history on the amend's word.
#[test]
fn commit_amend_no_edit_refuses_an_amend_that_did_not_commit() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.write_file("file1.txt", "amended");
    let workdir = test_repo.workdir();
    git::stage_files(workdir.as_path(), &["file1.txt"]).unwrap();
    let head = test_repo.head_oid();

    let result = git::commit_amend_no_edit(workdir.as_path(), &["--porcelain"]);

    assert!(result.is_err(), "the dry run should not pass for an amend");
    assert_eq!(test_repo.head_oid(), head);
    assert_eq!(test_repo.status_porcelain().trim(), "M  file1.txt");
}

/// Loom's own arguments come last, so git's last-wins parse keeps the amend
/// whatever the user forwards (Spec 021).
#[test]
fn commit_amend_no_edit_outranks_a_forwarded_no_amend() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.write_file("file1.txt", "amended");
    let workdir = test_repo.workdir();
    git::stage_files(workdir.as_path(), &["file1.txt"]).unwrap();
    let head = test_repo.head_oid();
    let parent = git::rev_parse(workdir.as_path(), "HEAD^").unwrap();

    git::commit_amend_no_edit(workdir.as_path(), &["--no-amend"]).unwrap();

    assert_ne!(test_repo.head_oid(), head, "the amend replaced HEAD");
    assert_eq!(
        git::rev_parse(workdir.as_path(), "HEAD^").unwrap(),
        parent,
        "nothing was committed on top"
    );
    assert_eq!(test_repo.get_message(0), "First commit");
}

/// A hook that stages something of its own must not make a successful amend
/// look like a dry run — on either side of the commit. A `pre-commit` hook
/// changes what is committed, and `post-commit` runs whatever `--no-verify`
/// says.
#[cfg(unix)]
#[test]
fn commit_amend_no_edit_survives_a_hook_that_stages() {
    for (when, arg) in [("pre-commit", "-q"), ("post-commit", "--no-verify")] {
        let test_repo = TestRepo::new();
        test_repo.commit("First commit", "file1.txt");
        let workdir = test_repo.workdir();
        test_repo.install_hook(when, "echo hooked > hooked.txt\ngit add hooked.txt\n");
        test_repo.write_file("file1.txt", "amended");
        git::stage_files(workdir.as_path(), &["file1.txt"]).unwrap();

        git::commit_amend_no_edit(workdir.as_path(), &[arg]).unwrap();

        assert_eq!(test_repo.get_message(0), "First commit", "{when}");
        assert_eq!(test_repo.read_file("file1.txt"), "amended", "{when}");
        let hooked_committed = test_repo.commit_has_file(test_repo.head_oid(), "hooked.txt");
        assert_eq!(hooked_committed, when == "pre-commit", "{when}");
    }
}

#[test]
fn commit_amend_no_edit_takes_an_argument_that_does_commit() {
    let test_repo = TestRepo::new();
    test_repo.commit("First commit", "file1.txt");
    test_repo.write_file("file1.txt", "amended");
    let workdir = test_repo.workdir();
    git::stage_files(workdir.as_path(), &["file1.txt"]).unwrap();

    git::commit_amend_no_edit(workdir.as_path(), &["--no-verify"]).unwrap();

    assert_eq!(test_repo.get_message(0), "First commit");
    assert_eq!(test_repo.status_porcelain().trim(), "");
}

/// A fixup path squashes whatever sits on HEAD, so it has to know that git
/// put a commit there; `--amend` would have replaced the user's own instead.
#[test]
fn committed_onto_sees_what_git_did() {
    let t = TestRepo::new();
    let base = t.commit("Base", "base.txt").to_string();
    assert!(!super::committed_onto(&t.workdir(), &base));

    let child = t.commit("Child", "child.txt").to_string();
    assert!(super::committed_onto(&t.workdir(), &base));
    assert!(!super::committed_onto(&t.workdir(), &child));
}
