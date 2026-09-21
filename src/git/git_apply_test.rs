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

/// The restore runs after the caller's own work has landed, so a failure must
/// not reach the caller: `branch new` deletes the branch it just wove on `Err`.
/// It also must not cost staging that was already there — the whole point.
#[test]
fn restore_staged_after_rebase_keeps_what_the_autostash_left() {
    let t = TestRepo::new();
    t.write_file("a.txt", "l1\nl2\nl3\nl4\nl5\nl6\n");
    t.stage_files(&["a.txt"]);
    t.commit_staged("base");

    // A staged edit plus a staged new file, as the user left them.
    t.write_file("a.txt", "l1-EDIT\nl2\nl3\nl4\nl5\nl6\n");
    t.write_file("n.txt", "new\n");
    t.stage_files(&["a.txt", "n.txt"]);
    let workdir = t.workdir();
    let patch = git::diff_cached(&workdir).unwrap();

    // What the rebase leaves: it rewrote a line inside the saved hunk's
    // context, the autostash replay unstaged the edit, and the new file's
    // index entry survived — so a plain cached apply would refuse both ends.
    // A pathspec commit rewrites HEAD without disturbing `n.txt`'s entry.
    t.write_file("a.txt", "l1\nl2\nl3-UPSTREAM\nl4\nl5\nl6\n");
    git::run_git(
        &workdir,
        &["commit", "-m", "upstream rewrite", "--", "a.txt"],
    )
    .unwrap();
    t.write_file("a.txt", "l1-EDIT\nl2\nl3-UPSTREAM\nl4\nl5\nl6\n");

    git::restore_staged_after_rebase(&workdir, &patch);

    assert_eq!(t.status_porcelain(), "M  a.txt\nA  n.txt\n");
    assert_eq!(
        git::run_git_stdout(&workdir, &["show", ":a.txt"]).unwrap(),
        "l1-EDIT\nl2\nl3-UPSTREAM\nl4\nl5\nl6\n",
        "the staged edit is merged onto what the rebase wrote"
    );
}

/// A held `index.lock` makes git fail; the caller still hears nothing.
#[test]
fn restore_staged_after_rebase_never_fails_the_caller() {
    let t = TestRepo::new();
    t.commit("First", "first.txt");
    t.write_file("first.txt", "staged edit\n");
    t.stage_files(&["first.txt"]);
    let workdir = t.workdir();
    let patch = git::diff_cached(&workdir).unwrap();
    assert!(!patch.is_empty());

    // What the autostash leaves, so the restore has real work to do.
    git::unstage_files(&workdir, &["first.txt"]).unwrap();
    std::fs::write(t.repo.path().join("index.lock"), b"").unwrap();

    git::restore_staged_after_rebase(&workdir, &patch);

    // The rehearsal locks its own scratch index and passes; the real apply is
    // what the lock stops, so the patch has to come back to the user.
    let parked = git::git_path(&workdir, "loom").unwrap();
    assert!(
        std::fs::read_dir(&parked)
            .expect("the patch is parked under the git dir")
            .filter_map(|e| e.ok())
            .any(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with("unrestored-staged")),
        "a failed restore hands the patch over"
    );
}

/// A three-way conflict writes stages into whatever index it is given, so the
/// real one must never be the index it is tried against.
#[test]
fn restore_staged_after_rebase_leaves_no_conflict_in_the_index() {
    let t = TestRepo::new();
    t.write_file("a.txt", "l1\nl2\nl3\n");
    t.stage_files(&["a.txt"]);
    t.commit_staged("base");

    t.write_file("a.txt", "l1\nMINE\nl3\n");
    t.stage_files(&["a.txt"]);
    let workdir = t.workdir();
    let patch = git::diff_cached(&workdir).unwrap();

    // The rebase rewrote the very line the staged edit touches, so the restore
    // cannot merge: `--3way` conflicts.
    t.write_file("a.txt", "l1\nTHEIRS\nl3\n");
    t.stage_files(&["a.txt"]);
    t.commit_staged("upstream rewrite");

    git::restore_staged_after_rebase(&workdir, &patch);

    assert!(!git::has_unmerged_paths(&workdir));
    assert_eq!(
        t.status_porcelain(),
        "",
        "the index is left as the rebase left it"
    );
    // A clean autostash replay drops the stash, so the patch is the only copy
    // of the staged side left: it has to be handed over, not dropped.
    let parked = git::git_path(&workdir, "loom").unwrap();
    let saved: Vec<_> = std::fs::read_dir(&parked)
        .expect("the patch is parked under the git dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("unrestored-staged")
        })
        .collect();
    assert_eq!(saved.len(), 1, "exactly one parked patch");
    assert!(
        std::fs::read_to_string(saved[0].path())
            .unwrap()
            .contains("MINE"),
        "the parked patch carries the staged content"
    );
}

/// The saved patch is what is left of the user's work when a rollback cannot
/// replay it, so two failures in a row must not land on the same file.
#[test]
fn save_patch_aside_never_writes_over_an_earlier_save() {
    let test_repo = TestRepo::new();
    test_repo.commit("A commit", "file1.txt");
    let workdir = test_repo.workdir();

    let first = git::save_patch_aside(&workdir, "unrestored", "first patch").unwrap();
    let second = git::save_patch_aside(&workdir, "unrestored", "second patch").unwrap();

    assert_eq!(first.file_name().unwrap(), "unrestored-0.patch");
    assert_eq!(second.file_name().unwrap(), "unrestored-1.patch");
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "first patch");
    assert_eq!(std::fs::read_to_string(&second).unwrap(), "second patch");
    assert!(
        first.starts_with(test_repo.repo.path()),
        "saved under the git dir, not next to the user's files: {}",
        first.display()
    );
}

/// `diff_cached` carries binaries inline so they can be restored; a staged
/// deletion has no postimage at all. Neither shape is text for `--3way`.
#[test]
fn restore_staged_after_rebase_carries_binary_and_deleted_files() {
    let t = TestRepo::new();
    t.write_file("keep.txt", "keep\n");
    std::fs::write(t.workdir().join("bin.dat"), [0u8, 1, 2, 3, 0, 255]).unwrap();
    t.write_file("gone.txt", "gone\n");
    t.stage_files(&["keep.txt", "bin.dat", "gone.txt"]);
    t.commit_staged("base");

    // A staged binary rewrite and a staged deletion.
    std::fs::write(t.workdir().join("bin.dat"), [9u8, 9, 9, 0, 7]).unwrap();
    std::fs::remove_file(t.workdir().join("gone.txt")).unwrap();
    let workdir = t.workdir();
    git::stage_files(&workdir, &["bin.dat", "gone.txt"]).unwrap();
    let patch = git::diff_cached(&workdir).unwrap();
    let before = t.status_porcelain();

    // What the autostash leaves: the index back at HEAD for both.
    git::run_git(&workdir, &["reset", "--mixed", "HEAD"]).unwrap();
    std::fs::write(t.workdir().join("bin.dat"), [9u8, 9, 9, 0, 7]).unwrap();

    git::restore_staged_after_rebase(&workdir, &patch);

    assert_eq!(t.status_porcelain(), before);
    assert_eq!(
        git::run_git_stdout(&workdir, &["show", ":bin.dat"])
            .map(|s| s.into_bytes())
            .unwrap_or_default()
            .len(),
        5,
        "the staged binary comes back byte for byte"
    );
}

/// A rebase still on disk after its abort failed: nothing later will put the
/// patch back, so it has to reach the user as a file.
#[test]
fn restore_or_park_after_abort_parks_when_the_rebase_survived() {
    let t = TestRepo::new();
    t.commit("First", "first.txt");
    t.write_file("first.txt", "staged edit\n");
    t.stage_files(&["first.txt"]);
    let workdir = t.workdir();
    let patch = git::diff_cached(&workdir).unwrap();

    // What a failed abort leaves behind: git's own rebase state on disk.
    std::fs::create_dir_all(t.repo.path().join("rebase-merge")).unwrap();
    let err = anyhow::anyhow!("the abort failed too");

    git::restore_or_park_after_abort(&workdir, &patch, &err);

    let parked = git::git_path(&workdir, "loom").unwrap();
    assert!(
        std::fs::read_dir(&parked)
            .expect("the patch is parked under the git dir")
            .filter_map(|e| e.ok())
            .any(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with("unrestored-staged")),
        "a rebase loom could not free hands the patch over"
    );
}

/// `msg::warn` is not captured here, so this covers the file it parks and not
/// the replay line it prints.
#[test]
fn save_or_warn_parks_a_patch_and_skips_an_empty_one() {
    let t = TestRepo::new();
    t.commit("A commit", "file1.txt");
    let workdir = t.workdir();

    git::save_or_warn(
        &workdir,
        "unrestored-staged",
        "a patch",
        git::Replay::Cached,
    );

    let parked = git::git_path(&workdir, "loom").unwrap();
    let saved: Vec<_> = std::fs::read_dir(&parked)
        .expect("parked under the git dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("unrestored-staged")
        })
        .collect();
    assert_eq!(saved.len(), 1);
    assert_eq!(std::fs::read_to_string(saved[0].path()).unwrap(), "a patch");

    // An empty patch is nothing to park and nothing to say.
    git::save_or_warn(&workdir, "unrestored", "", git::Replay::Worktree);
    assert!(!parked.join("unrestored-0.patch").exists());
}

/// The other half of the same helper: with no rebase left on disk the patch
/// goes back into the index instead of to a file.
#[test]
fn restore_or_park_after_abort_restages_when_the_rebase_is_gone() {
    let t = TestRepo::new();
    t.write_file("a.txt", "base\n");
    t.stage_files(&["a.txt"]);
    t.commit_staged("base");

    t.write_file("a.txt", "staged edit\n");
    t.stage_files(&["a.txt"]);
    let workdir = t.workdir();
    let patch = git::diff_cached(&workdir).unwrap();
    let before = t.status_porcelain();
    git::unstage_files(&workdir, &["a.txt"]).unwrap();

    // No `rebase-merge` directory, so the abort really did free the repository.
    git::restore_or_park_after_abort(&workdir, &patch, &anyhow::anyhow!("the rebase stopped"));

    assert_eq!(t.status_porcelain(), before);
    let parked = git::git_path(&workdir, "loom").unwrap();
    assert!(
        !parked.join("unrestored-staged-0.patch").exists(),
        "it went back into the index, so there is nothing to hand over"
    );
}

/// Several call sites restore before deleting their state file, so a failed
/// delete can leave the restore to run a second time. Re-applying a patch the
/// index already carries has to be a no-op, binary entries included.
#[test]
fn restore_staged_after_rebase_is_idempotent() {
    let t = TestRepo::new();
    t.write_file("keep.txt", "keep\n");
    t.write_file("gone.txt", "gone\n");
    std::fs::write(t.workdir().join("bin.dat"), [0u8, 1, 2, 3, 0, 255]).unwrap();
    t.stage_files(&["keep.txt", "gone.txt", "bin.dat"]);
    t.commit_staged("base");

    t.write_file("keep.txt", "keep\nstaged edit\n");
    std::fs::write(t.workdir().join("bin.dat"), [9u8, 9, 9, 0, 7]).unwrap();
    t.write_file("brand-new.txt", "new\n");
    let workdir = t.workdir();
    t.stage_files(&["keep.txt", "bin.dat", "brand-new.txt"]);
    // `git apply --cached` refuses a deletion whose entry is already gone,
    // and refuses the whole patch with it.
    git::run_git(&workdir, &["rm", "--cached", "-q", "gone.txt"]).unwrap();
    let patch = git::diff_cached(&workdir).unwrap();
    let before = t.status_porcelain();

    // Already applied: the index is exactly what the patch describes.
    git::restore_staged_after_rebase(&workdir, &patch);

    assert_eq!(t.status_porcelain(), before);
    assert_eq!(git::diff_cached(&workdir).unwrap(), patch);
    let parked = git::git_path(&workdir, "loom")
        .unwrap()
        .join("unrestored-staged-0.patch");
    assert!(!parked.exists(), "a no-op must not hand anything over");
}

/// The second restore a failed state delete brings on, with the rebase having
/// rewritten the file under the patch. Git refuses the staged deletion whose
/// entry is already gone, and the patch bytes no longer match the rewritten
/// preimage, so loom cannot tell this from a patch with work left in it: it
/// must keep the staged changes and hand the patch over, never drop either.
#[test]
fn restore_staged_after_rebase_parks_a_second_restore_over_a_moved_head() {
    let t = TestRepo::new();
    t.write_file("keep.txt", "alpha\nbeta\ngamma\ndelta\n");
    t.write_file("gone.txt", "gone\n");
    t.stage_files(&["keep.txt", "gone.txt"]);
    t.commit_staged("base");

    let workdir = t.workdir();
    t.write_file("keep.txt", "alpha staged\nbeta\ngamma\ndelta\n");
    t.stage_files(&["keep.txt"]);
    git::run_git(&workdir, &["rm", "--cached", "-q", "gone.txt"]).unwrap();
    let patch = git::diff_cached(&workdir).unwrap();

    // What the rebase leaves: the far end of the file rewritten, and an index
    // back at HEAD because the autostash replay reaches the worktree only.
    git::run_git(&workdir, &["reset", "--hard", "HEAD"]).unwrap();
    t.write_file("keep.txt", "alpha\nbeta\ngamma\ndelta rewritten\n");
    t.stage_files(&["keep.txt"]);
    t.commit_staged("rewrite");

    git::restore_staged_after_rebase(&workdir, &patch);
    let staged = git::run_git_stdout(&workdir, &["diff", "--cached", "--name-status"]).unwrap();
    assert_eq!(
        staged, "D\tgone.txt\nM\tkeep.txt\n",
        "the first restore must stage both halves"
    );

    git::restore_staged_after_rebase(&workdir, &patch);

    assert_eq!(
        git::run_git_stdout(&workdir, &["diff", "--cached", "--name-status"]).unwrap(),
        staged,
        "the second restore must not cost what the first put back"
    );
    let parked = git::git_path(&workdir, "loom")
        .unwrap()
        .join("unrestored-staged-0.patch");
    assert_eq!(std::fs::read_to_string(&parked).unwrap(), patch);
}

/// A patch git refuses outright must be handed over, not dropped. Git
/// validates the whole patch before writing any of it, so a refusal over one
/// path leaves the index untouched — which reads exactly like a restore that
/// had nothing to do, and took the other paths' staged work with it.
#[test]
fn restore_staged_after_rebase_parks_a_patch_naming_a_removed_path() {
    let t = TestRepo::new();
    t.write_file("keep.txt", "alpha\nbeta\ngamma\ndelta\n");
    t.write_file("x.txt", "x\n");
    t.stage_files(&["keep.txt", "x.txt"]);
    t.commit_staged("base");

    let workdir = t.workdir();
    t.write_file("keep.txt", "alpha staged\nbeta\ngamma\ndelta\n");
    t.write_file("x.txt", "x\nsecret\n");
    t.stage_files(&["keep.txt", "x.txt"]);
    let patch = git::diff_cached(&workdir).unwrap();

    // The rebase removed the file one half of the patch is against.
    git::run_git(&workdir, &["reset", "--hard", "HEAD"]).unwrap();
    git::run_git(&workdir, &["rm", "-q", "x.txt"]).unwrap();
    t.commit_staged("remove x.txt");

    git::restore_staged_after_rebase(&workdir, &patch);

    let parked = git::git_path(&workdir, "loom")
        .unwrap()
        .join("unrestored-staged-0.patch");
    assert_eq!(
        std::fs::read_to_string(&parked).unwrap(),
        patch,
        "the staged side is only in the patch by now, so it has to be handed over"
    );
}

/// A refusal raised before the rebase started autostashed nothing and left the
/// index where it was, so it is the user's and this must not write to it. The
/// patch names work the index does not hold, which is what would show if the
/// early return went.
#[test]
fn restore_or_park_after_abort_leaves_the_index_alone_before_the_rebase_starts() {
    let t = TestRepo::new();
    t.write_file("a.txt", "committed\n");
    t.stage_files(&["a.txt"]);
    t.commit_staged("base");

    let workdir = t.workdir();
    t.write_file("aside.txt", "set aside\n");
    t.stage_files(&["aside.txt"]);
    let patch = git::diff_cached(&workdir).unwrap();
    git::run_git(&workdir, &["reset", "-q", "HEAD"]).unwrap();

    let err = git::before_rebase_starts::<()>(Err(anyhow::anyhow!("checked out elsewhere")))
        .expect_err("tagged as raised before the rebase started");
    git::restore_or_park_after_abort(&workdir, &patch, &err);

    assert_eq!(git::diff_cached(&workdir).unwrap(), "");
    assert!(
        !git::git_path(&workdir, "loom").unwrap().exists(),
        "nothing to park either"
    );
}

/// `loom fold -p` empties the index itself before rebasing, so a failure with
/// no rebase left on disk has no autostash behind it: the staged side is only
/// in the patch, and must go back even though nothing was ever stashed.
#[test]
fn restore_loom_unstaged_restores_when_no_rebase_is_on_disk() {
    let t = TestRepo::new();
    t.write_file("a.txt", "committed\n");
    t.stage_files(&["a.txt"]);
    t.commit_staged("base");

    let workdir = t.workdir();
    t.write_file("a.txt", "staged\n");
    t.stage_files(&["a.txt"]);
    let patch = git::diff_cached(&workdir).unwrap();
    // What `staging::save_and_unstage_staged` leaves behind.
    git::run_git(&workdir, &["reset", "-q", "HEAD"]).unwrap();

    git::restore_loom_unstaged(&workdir, &patch);

    assert_eq!(git::diff_cached(&workdir).unwrap(), patch);
}

/// Builds the conflict the hint is about: every file in `files` is changed on
/// both sides, so the merge leaves them all with stages in the index.
fn conflict_on(t: &TestRepo, files: &[&str]) {
    let workdir = t.workdir();
    for file in files {
        if let Some(dir) = workdir.join(file).parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
    }
    let commit = |content: &str| {
        for file in files {
            t.write_file(file, content);
        }
        t.stage_files(files);
        t.commit_staged(content);
        t.head_oid().to_string()
    };
    let base = commit("base");
    let theirs = commit("theirs");
    git::run_git(&workdir, &["checkout", "-q", "-b", "ours", &base]).unwrap();
    commit("ours");
    git::run_git(&workdir, &["merge", "--no-commit", &theirs]).unwrap_err();
}

/// The hint is a command line the user retypes, so every path in it has to
/// survive a shell: bare, `my file.txt` reaches git as two pathspecs that
/// match nothing, and git reports success having cleared no stage at all.
#[test]
fn the_unmerged_reset_hint_quotes_every_path_it_names() {
    let t = TestRepo::new();
    let workdir = t.workdir();
    conflict_on(&t, &["a.txt", "my file.txt", "été.txt", "weird;&name.txt"]);

    // Index order is byte order, so the whole line is fixed: every entry is
    // there, in that order, rooted, and quoted against the shell.
    assert_eq!(
        super::unmerged_reset_hint(&workdir).expect("the merge left stages behind"),
        "it left unmerged entries in the index — `git reset -- \
         ':(top,literal)a.txt' ':(top,literal)my file.txt' \
         ':(top,literal)weird;&name.txt' ':(top,literal)été.txt'` clears them"
    );
}

/// The whole point of the hint: running it clears the stages it names and
/// leaves the rest of the index alone. Run from a subdirectory, because that
/// is where the user usually is and git reports these paths from the root.
#[test]
fn the_hinted_reset_clears_the_stages_and_spares_the_rest() {
    let t = TestRepo::new();
    let workdir = t.workdir();
    conflict_on(&t, &["sub/a.txt", "sub/g[1].txt"]);

    t.write_file(
        "keep.txt",
        "the user staged this
",
    );
    t.stage_files(&["keep.txt"]);

    let specs = super::unmerged_pathspecs(&workdir);
    let mut args = vec!["reset", "--"];
    args.extend(specs.iter().map(String::as_str));
    git::run_git(&workdir.join("sub"), &args).unwrap();

    assert!(
        git::unmerged_paths(&workdir).is_empty(),
        "the hinted reset must clear every stage it names, from a subdirectory"
    );
    assert_eq!(
        crate::core::repo::get_staged_files(&t.repo).unwrap(),
        vec!["keep.txt".to_string()],
        "staging the patch has no copy of must survive the reset"
    );
}

/// A name git reports in bytes loom cannot decode arrives holding U+FFFD, and
/// a pathspec built from it matches nothing — git would exit 0 having cleared
/// no stage. So the hint names nothing, and still does not send the user to an
/// unscoped `git reset`, which unstages what the parked patch has no copy of.
#[test]
fn the_unmerged_reset_hint_names_no_path_it_could_not_decode() {
    let specs = vec![
        ":(top,literal)a.txt".to_string(),
        ":(top,literal)caf\u{fffd}.txt".to_string(),
    ];

    assert_eq!(
        super::reset_hint_for(&specs).expect("the index is unmerged"),
        "it left unmerged entries in the index — `git status` names them, and a \
         `git reset` limited to those paths clears them"
    );
}

/// No stages, no hint: the caller must not warn about an index it did not
/// leave conflicted.
#[test]
fn the_unmerged_reset_hint_is_none_on_a_clean_index() {
    let t = TestRepo::new();
    t.commit("only", "a.txt");

    assert!(super::unmerged_reset_hint(&t.workdir()).is_none());
}
