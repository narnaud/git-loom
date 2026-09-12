use crate::core::test_helpers::TestRepo;

fn no_pager() {
    // SAFETY: tests are serialized via `in_dir`'s global mutex.
    unsafe { std::env::set_var("GIT_PAGER", "cat") };
}

#[test]
fn diff_no_args() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("Initial commit", "file.txt");

    let result = test_repo.in_dir(|| super::run(vec![], false, false, vec![]));
    assert!(result.is_ok(), "diff with no args should succeed");
}

#[test]
fn diff_staged() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("Initial commit", "file.txt");

    let result = test_repo.in_dir(|| super::run(vec![], true, false, vec![]));
    assert!(result.is_ok(), "diff --staged should succeed");
}

#[test]
fn diff_all() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("Initial commit", "file.txt");

    let result = test_repo.in_dir(|| super::run(vec![], false, true, vec![]));
    assert!(result.is_ok(), "diff --all should succeed");
}

#[test]
fn diff_commit_by_hash() {
    no_pager();
    let test_repo = TestRepo::new();
    let oid = test_repo.commit("Test commit", "file.txt");

    let result = test_repo.in_dir(|| super::run(vec![oid.to_string()], false, false, vec![]));
    assert!(result.is_ok(), "diff with commit hash should succeed");
}

#[test]
fn diff_commit_range() {
    no_pager();
    let test_repo = TestRepo::new();
    let oid1 = test_repo.commit("First commit", "file1.txt");
    let oid2 = test_repo.commit("Second commit", "file2.txt");

    let range = format!("{}..{}", oid1, oid2);
    let result = test_repo.in_dir(|| super::run(vec![range], false, false, vec![]));
    assert!(result.is_ok(), "diff with commit range should succeed");
}

#[test]
fn diff_invalid_target_fails() {
    let test_repo = TestRepo::new();

    let result =
        test_repo.in_dir(|| super::run(vec!["nonexistent_xyz".to_string()], false, false, vec![]));
    assert!(result.is_err(), "diff with invalid target should fail");
}

#[test]
fn diff_forwards_an_option_after_the_separator() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("Initial commit", "file.txt");

    let result = test_repo.in_dir(|| super::run(vec![], false, false, vec!["-w".to_string()]));
    assert!(result.is_ok(), "diff should forward -w to git diff");
}

#[test]
fn diff_forwards_a_detached_option_value() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("Initial commit", "file.txt");

    // `-S <string>` takes its value as a separate token. The sniffer this
    // replaced would have resolved that token as a target.
    let result = test_repo.in_dir(|| {
        super::run(
            vec![],
            false,
            false,
            vec!["-S".to_string(), "x".to_string()],
        )
    });
    assert!(result.is_ok(), "a detached option value belongs to git");
}

#[test]
fn diff_forwards_a_short_flag_loom_also_defines() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("Initial commit", "file.txt");

    // Loom's own `-a` is `--all`; after the separator it is git's `--text`.
    let result = test_repo.in_dir(|| super::run(vec![], false, false, vec!["-a".to_string()]));
    assert!(result.is_ok(), "-a after the separator is git's --text");
}

#[test]
fn diff_forwards_a_pathspec_after_the_separator() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("Initial commit", "file.txt");

    let result = test_repo.in_dir(|| {
        super::run(
            vec![],
            false,
            false,
            vec!["--".to_string(), "file.txt".to_string()],
        )
    });
    assert!(result.is_ok(), "diff should forward the pathspec to git");
}

#[test]
fn forwarded_option_precedes_the_pathspec_loom_builds() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("Initial commit", "file.txt");

    // Appended after loom's own `--`, `--stat` would be a pathspec and git
    // would reject it as matching nothing.
    let result = test_repo.in_dir(|| {
        super::run(
            vec!["file.txt".to_string()],
            false,
            false,
            vec!["--stat".to_string()],
        )
    });
    assert!(result.is_ok(), "--stat must stay ahead of loom's `--`");
}

#[test]
fn diff_unknown_option_reaches_git() {
    no_pager();
    let test_repo = TestRepo::new();
    test_repo.commit("Initial commit", "file.txt");

    let result = test_repo.in_dir(|| {
        super::run(
            vec![],
            false,
            false,
            vec!["--definitely-not-a-git-option".to_string()],
        )
    });
    assert!(
        result.is_err(),
        "git should reject an option loom passed through"
    );
}
