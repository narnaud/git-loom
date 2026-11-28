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

/// `core.quotePath` defaults to on, so without `-z` a non-ASCII path comes back
/// escaped and quoted, matches no pathspec, and the hunk picker drops the file.
#[test]
fn diff_commit_name_status_returns_raw_paths() {
    let test_repo = TestRepo::new();
    test_repo.commit_multi(&[("café.txt", "a\nb\n"), ("plain.txt", "x\n")], "Add");
    let workdir = test_repo.workdir();
    git::run_git(&workdir, &["mv", "café.txt", "néw.txt"]).unwrap();
    test_repo.write_file("néw.txt", "a\nb\nc\n");
    test_repo.write_file("plain.txt", "y\n");
    test_repo.write_file("thé.txt", "t\n");
    test_repo.stage_files(&["néw.txt", "plain.txt", "thé.txt"]);
    test_repo.commit_staged("Rename");

    let mut changed = git::diff_commit_name_status(&workdir, "HEAD").unwrap();
    changed.sort();

    assert_eq!(
        changed,
        [
            ('A', "néw.txt".to_string()),
            ('A', "thé.txt".to_string()),
            ('D', "café.txt".to_string()),
            ('M', "plain.txt".to_string()),
        ]
    );
}

#[test]
fn a_patch_splits_on_the_paths_its_headers_name() {
    let ambiguous = concat!(
        "diff --git a/x b/y b/x b/y\n",
        "--- a/x b/y\n",
        "+++ b/x b/y\n",
        "@@ -1 +1 @@\n",
        "-diff --git a/z b/z\n",
        "+one\n",
    );
    let quoted = concat!(
        r#"diff --git "a/t\tq\"b\\s\303\251" "b/t\tq\"b\\s\303\251""#,
        "\n@@ -1 +1 @@\n-a\n+b\n",
    );
    let conflicted = "* Unmerged path c\ndiff --cc c\n@@@ -1,1 -1,1 +1,1 @@@\n";
    let gitlink_half = "diff --git a/t b/t\ndeleted file mode 160000\n";
    let file_half = "diff --git a/t b/t\nnew file mode 100644\n";
    let patch = [ambiguous, quoted, conflicted, gitlink_half, file_half].concat();

    let files = super::split_patch(&patch);

    let mut paths: Vec<&str> = files.keys().map(String::as_str).collect();
    paths.sort();
    assert_eq!(paths, ["t", "t\tq\"b\\sé", "x b/y"]);
    assert_eq!(files["x b/y"], ambiguous);
    assert_eq!(files["t\tq\"b\\sé"], quoted);
    assert_eq!(files["t"], [gitlink_half, file_half].concat());
}

#[test]
fn a_header_name_that_is_not_utf8_names_no_path() {
    let line = git::raw_text::decode(b"diff --git a/caf\xe9 b/caf\xe9\n");
    assert_eq!(super::header_path(&line), None);
    assert_eq!(
        super::header_path(r#"diff --git "a/caf\351" "b/caf\351""#),
        None
    );
    assert_eq!(
        super::header_path(r#"diff --git "a/caf\303\251" "b/caf\303\251""#).as_deref(),
        Some("café")
    );
}

/// `paths` plus enough absent ones to pass the pathspec budget, so the side is
/// diffed whole: as pathspecs they would overflow a Windows command line.
fn past_the_budget<'a>(paths: &[&'a str], absent: &'a [String]) -> Vec<&'a str> {
    let mut all = paths.to_vec();
    all.extend(absent.iter().map(String::as_str));
    all
}

fn absent_paths() -> Vec<String> {
    (0..1000).map(|n| format!("{n:064}.txt")).collect()
}

#[test]
fn paths_past_the_pathspec_budget_still_get_their_patches() {
    let test_repo = TestRepo::new();
    test_repo.write_file("a.txt", "one\n");
    test_repo.write_file("b.txt", "one\n");
    test_repo.stage_files(&["a.txt", "b.txt"]);
    test_repo.commit_staged("Add");
    test_repo.write_file("a.txt", "two\n");
    test_repo.write_file("b.txt", "two\n");
    let absent = absent_paths();

    let diffs =
        git::diff_by_file(&test_repo.workdir(), &past_the_budget(&["a.txt"], &absent)).unwrap();

    assert!(diffs.patch("a.txt").contains("+two\n"));
}

/// The status reads `caf\xe9.bin` as `caf\u{fffd}.bin`, which is not that file.
/// A pathspec of that name matches nothing; a whole-side diff must not either.
#[test]
fn a_whole_side_has_no_patch_under_a_lossy_name() {
    let test_repo = TestRepo::new();
    let repo = &test_repo.repo;
    let sig = git2::Signature::now("Test", "test@test.com").unwrap();
    let mut parent = test_repo.head_commit();
    for version in ["one", "two"] {
        let mut builder = repo.treebuilder(None).unwrap();
        let binary = repo.blob(format!("\0{version}").as_bytes()).unwrap();
        builder
            .insert(&b"caf\xe9.bin"[..], binary, 0o100644)
            .unwrap();
        let text = repo.blob(format!("{version}\n").as_bytes()).unwrap();
        builder.insert("ok.txt", text, 0o100644).unwrap();
        let tree = repo.find_tree(builder.write().unwrap()).unwrap();
        let oid = repo
            .commit(None, &sig, &sig, version, &tree, &[&parent])
            .unwrap();
        parent = repo.find_commit(oid).unwrap();
    }
    let absent = absent_paths();
    let paths = past_the_budget(&["ok.txt", "caf\u{fffd}.bin"], &absent);

    let diffs =
        git::diff_commit_by_file(&test_repo.workdir(), &parent.id().to_string(), &paths).unwrap();

    assert!(diffs.patch("ok.txt").contains("+two\n"));
    assert_eq!(diffs.patch("caf\u{fffd}.bin"), "");
    assert!(!diffs.is_binary("caf\u{fffd}.bin"));
}

/// Names git C-quotes even with `core.quotePath` off. Not on Windows, which
/// refuses them in a file name.
#[cfg(unix)]
#[test]
fn diff_by_file_finds_paths_git_quotes() {
    let test_repo = TestRepo::new();
    let paths = ["q\"uote.txt", "tab\there.txt", "back\\slash.txt"];
    for path in paths {
        test_repo.write_file(path, "one\n");
    }
    test_repo.stage_files(&paths);
    test_repo.commit_staged("Add");
    for path in paths {
        test_repo.write_file(path, "two\n");
    }

    let diffs = git::diff_by_file(&test_repo.workdir(), &paths).unwrap();

    for path in paths {
        assert!(diffs.patch(path).contains("+two\n"), "{path:?}");
    }
}
