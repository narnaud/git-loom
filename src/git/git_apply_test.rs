use crate::core::test_helpers::TestRepo;
use crate::git;

const BASE: &str = "1\n2\n3\n4\n5\n6\n7\n";

/// A repo whose HEAD carries `head_content` in `file`, plus the patch of a
/// commit that turned [`BASE`]'s fourth line into `FOUR` and is no longer in
/// history — the shape `fold <commit> zz` leaves behind once the commit has
/// been dropped.
fn repo_with_dropped_patch_in(file: &str, head_content: &str) -> (TestRepo, String) {
    let test_repo = TestRepo::new();
    test_repo.write_file(file, BASE);
    test_repo.stage_files(&[file]);
    test_repo.commit_staged("base");
    let base_oid = test_repo.head_oid();

    test_repo.write_file(file, &BASE.replace("4\n", "FOUR\n"));
    test_repo.stage_files(&[file]);
    test_repo.commit_staged("change 4");
    let patch = test_repo.diff_commit(&test_repo.head_oid().to_string());

    test_repo.reset_hard(base_oid);
    test_repo.write_file(file, head_content);
    test_repo.stage_files(&[file]);
    test_repo.commit_staged("later commit");

    // A staged change of the user's own, to check the index is left alone.
    test_repo.write_file("staged.txt", "staged");
    test_repo.stage_files(&["staged.txt"]);

    (test_repo, patch)
}

/// A later commit inside the hunk's context defeats a plain `git apply`; the
/// three-way fallback merges the patch in and leaves it unstaged.
#[test]
fn apply_to_worktree_falls_back_to_three_way() {
    let (test_repo, patch) = repo_with_dropped_patch_in("f.txt", &BASE.replace("6\n", "SIX\n"));

    git::apply_patch_to_worktree(&test_repo.workdir(), &patch).unwrap();

    assert_eq!(
        test_repo.read_file("f.txt"),
        "1\n2\n3\nFOUR\n5\nSIX\n7\n",
        "both changes should be there"
    );
    assert_eq!(test_repo.status_porcelain(), " M f.txt\nA  staged.txt\n");
}

/// A later commit on the very line the patch changes is a real conflict: the
/// error must not leave conflict markers or a half-merged index behind.
#[test]
fn apply_to_worktree_reports_a_conflict_and_restores_the_file() {
    let (test_repo, patch) = repo_with_dropped_patch_in("f.txt", &BASE.replace("4\n", "QUATRE\n"));

    git::apply_patch_to_worktree(&test_repo.workdir(), &patch).expect_err("the patch conflicts");

    assert_eq!(
        test_repo.read_file("f.txt"),
        BASE.replace("4\n", "QUATRE\n"),
        "no conflict markers"
    );
    assert_eq!(test_repo.status_porcelain(), "A  staged.txt\n");
}

/// `core.quotePath` defaults to on, so the conflicted path git hands back for a
/// non-ASCII name is escaped and quoted; cleaning the file up needs the real one.
#[test]
fn apply_to_worktree_cleans_up_a_non_ascii_path() {
    let quatre = BASE.replace("4\n", "QUATRE\n");
    let (test_repo, patch) = repo_with_dropped_patch_in("été.txt", &quatre);

    git::apply_patch_to_worktree(&test_repo.workdir(), &patch).expect_err("the patch conflicts");

    assert_eq!(
        test_repo.read_file("été.txt"),
        quatre,
        "no conflict markers"
    );
    assert_eq!(test_repo.status_porcelain(), "A  staged.txt\n");
}

/// `--3way` writes the files it could merge even when it conflicts on another,
/// so a failure has to put the cleanly-merged ones back too.
#[test]
fn apply_to_worktree_undoes_a_partial_apply() {
    let test_repo = TestRepo::new();
    let clean = BASE.replace("6\n", "SIX\n");
    let conflicting = BASE.replace("4\n", "QUATRE\n");

    test_repo.write_file("clean.txt", BASE);
    test_repo.write_file("conflicting.txt", BASE);
    test_repo.write_file("added.txt", "gone once the apply is undone\n");
    test_repo.stage_files(&["clean.txt", "conflicting.txt"]);
    test_repo.commit_staged("base");
    let base_oid = test_repo.head_oid();

    let four = BASE.replace("4\n", "FOUR\n");
    test_repo.write_file("clean.txt", &four);
    test_repo.write_file("conflicting.txt", &four);
    test_repo.stage_files(&["clean.txt", "conflicting.txt", "added.txt"]);
    test_repo.commit_staged("change 4 in both, add a file");
    let patch = test_repo.diff_commit(&test_repo.head_oid().to_string());

    test_repo.reset_hard(base_oid);
    test_repo.write_file("clean.txt", &clean);
    test_repo.write_file("conflicting.txt", &conflicting);
    test_repo.stage_files(&["clean.txt", "conflicting.txt"]);
    test_repo.commit_staged("later commit");

    git::apply_patch_to_worktree(&test_repo.workdir(), &patch).expect_err("the patch conflicts");

    assert_eq!(
        test_repo.read_file("clean.txt"),
        clean,
        "merged file put back"
    );
    assert_eq!(
        test_repo.read_file("conflicting.txt"),
        conflicting,
        "conflicted file put back"
    );
    assert!(
        !test_repo.workdir().join("added.txt").exists(),
        "the file the patch created should be gone again"
    );
    assert_eq!(test_repo.status_porcelain(), "");
}

/// The paths of the files to put back are file names, not patterns: a real file
/// called `a[12].txt` must not drag `a1.txt` — and the user's edits to it —
/// into the cleanup.
#[test]
fn apply_to_worktree_does_not_treat_a_path_as_a_glob() {
    let quatre = BASE.replace("4\n", "QUATRE\n");
    let (test_repo, patch) = repo_with_dropped_patch_in("a[12].txt", &quatre);
    test_repo.write_file("a1.txt", "committed");
    test_repo.stage_files(&["a1.txt"]);
    test_repo.commit_staged("a sibling the glob would match");
    test_repo.write_file("a1.txt", "the user's uncommitted work");

    git::apply_patch_to_worktree(&test_repo.workdir(), &patch).expect_err("the patch conflicts");

    assert_eq!(
        test_repo.read_file("a1.txt"),
        "the user's uncommitted work",
        "the cleanup must leave a file the patch never touched alone"
    );
    assert_eq!(
        test_repo.read_file("a[12].txt"),
        quatre,
        "no conflict markers"
    );
}

/// A three-way that refuses the patch outright — what happens when the user has
/// already changed one of its files — writes nothing, so there is nothing to
/// put back. Undoing "nothing" must not mean every tracked file in the repo.
#[test]
fn apply_to_worktree_leaves_the_tree_alone_when_nothing_was_written() {
    let (test_repo, patch) = repo_with_dropped_patch_in("f.txt", &BASE.replace("6\n", "SIX\n"));
    test_repo.write_file("unrelated.txt", "committed");
    test_repo.stage_files(&["unrelated.txt"]);
    test_repo.commit_staged("a file the patch never mentions");

    // A locally changed `f.txt` is what makes `--3way` refuse the whole patch.
    test_repo.write_file("f.txt", "the user's own rewrite\n");
    test_repo.write_file("unrelated.txt", "the user's uncommitted work");

    git::apply_patch_to_worktree(&test_repo.workdir(), &patch).expect_err("the patch is refused");

    assert_eq!(
        test_repo.read_file("unrelated.txt"),
        "the user's uncommitted work",
        "an untouched file must keep the user's changes"
    );
    assert_eq!(
        test_repo.read_file("f.txt"),
        "the user's own rewrite\n",
        "and so must the file the patch was refused over"
    );
}
