use crate::core::test_helpers::TestRepo;
use crate::git;

/// A repo whose local config is built to reshape every patch git prints.
fn hostile_repo() -> TestRepo {
    let test_repo = TestRepo::new();
    test_repo.set_config("color.ui", "false");
    test_repo.set_config("color.diff", "always");
    test_repo.set_config("diff.noprefix", "true");
    test_repo.set_config("diff.context", "0");
    test_repo.set_config("apply.whitespace", "error");
    test_repo
}

/// Rewrite the second line of `file` (committed as `body`) so the hunk needs
/// context on both sides.
fn edit_middle_line(test_repo: &TestRepo, file: &str) {
    let body = "one\ntwo\nthree\nfour\nfive\n";
    test_repo.write_file(file, body);
    test_repo.stage_files(&[file]);
    test_repo.commit_staged("body");
    test_repo.write_file(file, &body.replace("three", "THREE "));
}

/// `color.diff=always` beats `color.ui=false`; the replay path must not carry
/// escape codes, prefix-less paths, or zero-context hunks into a saved patch.
#[test]
fn replay_diff_ignores_patch_reshaping_config() {
    let test_repo = hostile_repo();
    edit_middle_line(&test_repo, "file.txt");

    let patch = git::diff_head_file(test_repo.workdir().as_path(), "file.txt").unwrap();

    assert!(!patch.contains('\x1b'), "escape codes in patch:\n{patch}");
    assert!(
        patch.contains("--- a/file.txt\n+++ b/file.txt"),
        "prefixes missing:\n{patch}"
    );
    assert!(
        patch.contains("@@ -1,5 +1,5 @@"),
        "context lines missing:\n{patch}"
    );
}

/// A saved patch that adds trailing whitespace still applies under
/// `apply.whitespace=error`.
#[test]
fn replay_diff_round_trips_through_apply() {
    let test_repo = hostile_repo();
    edit_middle_line(&test_repo, "file.txt");
    let workdir = test_repo.workdir();
    let patch = git::diff_head_file(workdir.as_path(), "file.txt").unwrap();
    test_repo.force_checkout();
    assert_eq!(
        read_lf(&test_repo, "file.txt"),
        "one\ntwo\nthree\nfour\nfive\n"
    );

    git::apply_patch(workdir.as_path(), &patch).unwrap();

    assert_eq!(
        read_lf(&test_repo, "file.txt"),
        "one\ntwo\nTHREE \nfour\nfive\n"
    );
}

/// Read `file` with LF line endings: on Windows, git's default
/// `core.autocrlf=true` checks files out with CRLF.
fn read_lf(test_repo: &TestRepo, file: &str) -> String {
    test_repo.read_file(file).replace("\r\n", "\n")
}

/// The display path drops color and external drivers but keeps textconv:
/// what a user reads in the TUI is still theirs to configure.
#[test]
fn display_diff_keeps_textconv_but_not_color() {
    let test_repo = hostile_repo();
    test_repo.set_config("diff.bin.textconv", "cat");
    test_repo.write_file(".gitattributes", "*.bin diff=bin\n");
    test_repo.write_file("data.bin", "\0old\n");
    test_repo.stage_files(&[".gitattributes", "data.bin"]);
    test_repo.commit_staged("binary");
    test_repo.write_file("data.bin", "\0new\n");
    let workdir = test_repo.workdir();

    let display = git::diff_head_file_display(workdir.as_path(), "data.bin").unwrap();
    let replay = git::diff_head_file(workdir.as_path(), "data.bin").unwrap();

    assert!(!display.contains('\x1b'), "escape codes:\n{display}");
    assert!(
        display.contains("+\0new"),
        "textconv not applied:\n{display}"
    );
    assert!(
        replay.contains("Binary files"),
        "textconv leaked:\n{replay}"
    );
}

/// An empty pathspec makes git match the whole tree, which would show a
/// caller every file when it asked for none.
#[test]
fn diff_head_files_display_with_no_paths_is_empty_not_everything() {
    let test_repo = TestRepo::new();
    test_repo.write_file("a.txt", "one\n");
    test_repo.stage_files(&["a.txt"]);
    test_repo.commit_staged("Add a");
    test_repo.write_file("a.txt", "two\n");
    let workdir = test_repo.workdir();

    assert_eq!(git::diff_head_files_display(&workdir, &[]).unwrap(), "");
    assert!(
        git::diff_head_files_display(&workdir, &["a.txt"])
            .unwrap()
            .contains("+two")
    );
}
