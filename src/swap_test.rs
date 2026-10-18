use crate::core::test_helpers::TestRepo;
use crate::core::weave::Weave;

// ── swap commits ──────────────────────────────────────────────────────────

#[test]
fn swap_commits_on_integration_line() {
    let test_repo = TestRepo::new_with_remote();
    let c1_oid = test_repo.commit("First", "first.txt");
    let c2_oid = test_repo.commit("Second", "second.txt");
    test_repo.commit("Third", "third.txt");

    let result = super::swap_two_commits(&test_repo.repo, c1_oid.to_string(), c2_oid.to_string());
    assert!(result.is_ok(), "swap_two_commits failed: {:?}", result);

    // Second was applied before First, so in newest-first order:
    // HEAD=Third, HEAD~1=First, HEAD~2=Second
    assert_eq!(test_repo.get_message(0), "Third");
    assert_eq!(test_repo.get_message(1), "First");
    assert_eq!(test_repo.get_message(2), "Second");
}

#[test]
fn swap_commits_in_branch_section() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    let a1_oid = test_repo.commit("A1", "a1.txt");
    let a2_oid = test_repo.commit("A2", "a2.txt");

    test_repo.switch_branch("integration");
    test_repo.commit("Int", "int.txt");
    test_repo.merge_no_ff("feature-a");

    let result = super::swap_two_commits(&test_repo.repo, a1_oid.to_string(), a2_oid.to_string());
    assert!(
        result.is_ok(),
        "swap_two_commits in branch failed: {:?}",
        result
    );

    // Verify via weave: A2 should now be first (oldest) in the section
    let graph = Weave::from_repo(&test_repo.repo).unwrap();
    assert_eq!(graph.branch_sections[0].commits[0].message, "A2");
    assert_eq!(graph.branch_sections[0].commits[1].message, "A1");
}

#[test]
fn swap_commits_across_sections_errors() {
    let test_repo = TestRepo::new_with_remote();
    let base_oid = test_repo.find_remote_branch_target("origin/main");

    test_repo.create_branch_at("feature-a", &base_oid.to_string());
    test_repo.switch_branch("feature-a");
    let a1_oid = test_repo.commit("A1", "a1.txt");

    test_repo.create_branch_at("feature-b", &base_oid.to_string());
    test_repo.switch_branch("feature-b");
    let b1_oid = test_repo.commit("B1", "b1.txt");

    test_repo.switch_branch("integration");
    test_repo.merge_no_ff("feature-a");
    test_repo.merge_no_ff("feature-b");

    let result = super::swap_two_commits(&test_repo.repo, a1_oid.to_string(), b1_oid.to_string());
    assert!(
        result.is_err(),
        "should fail for commits in different sections"
    );
    assert!(result.unwrap_err().to_string().contains("different"));
}

// ── Abort preserves working state ────────────────────────────────────────

/// Regression: loom abort after a swap conflict must preserve staged changes,
/// unstaged changes on other files, and new untracked files.
///
/// Conflict setup: Commit A creates `shared.txt`; Commit B modifies it.
/// Swapping them puts B first — B's diff expects A's content but the file
/// doesn't yet exist at the rebase base → conflict.
#[test]
fn swap_abort_preserves_working_state() {
    let test_repo = TestRepo::new_with_remote();

    let a_oid = test_repo.commit("version-a", "shared.txt");
    test_repo.write_file("shared.txt", "version-b");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("Commit B");
    let b_oid = test_repo.head_oid();

    // Working state before swap.
    test_repo.write_file("shared.txt", "working-edit");
    test_repo.write_file("other-staged.txt", "staged-content");
    test_repo.stage_files(&["other-staged.txt"]);
    test_repo.write_file("other-unstaged.txt", "unstaged-content");
    test_repo.write_file("new-file.txt", "new-content");

    let result = super::swap_two_commits(&test_repo.repo, a_oid.to_string(), b_oid.to_string());
    assert!(
        result.is_ok(),
        "swap should pause on conflict: {:?}",
        result
    );

    let state_path = test_repo.repo.path().join("loom").join("state.json");
    assert!(
        state_path.exists(),
        "loom state must exist when swap is paused on conflict"
    );

    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();
    crate::core::transaction::abort_cmd(&workdir, &git_dir).unwrap();

    assert_eq!(test_repo.read_file("shared.txt"), "working-edit");
    assert_eq!(test_repo.read_file("other-staged.txt"), "staged-content");
    assert_eq!(
        test_repo.read_file("other-unstaged.txt"),
        "unstaged-content"
    );
    assert!(
        workdir.join("new-file.txt").exists(),
        "new untracked file must survive abort"
    );
    assert_eq!(test_repo.read_file("new-file.txt"), "new-content");
}

#[test]
fn swap_refuses_when_a_swapped_commit_replays_empty() {
    // Both commits are named in the success message, so neither may vanish.
    let (t, redundant, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_below();
    let head_before = t.head_oid();
    let alpha_before = t.get_branch_target("alpha");
    t.write_file("three.txt", "three\nstaged edit\n");
    t.stage_files(&["three.txt"]);
    let staged_before = crate::git::diff_cached(&t.workdir()).unwrap();

    let err = super::swap_two_commits(&t.repo, redundant.to_string(), keeper.to_string())
        .unwrap_err()
        .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(t.head_oid(), head_before, "{err}");
    assert_eq!(t.get_branch_target("alpha"), alpha_before, "{err}");
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()), "{err}");
    assert!(
        !t.repo.path().join("loom").join("state.json").exists(),
        "{err}"
    );
    // `git rebase --abort` replays the autostash unstaged; `saved_staged_patch`
    // is what puts the index back.
    assert_eq!(
        crate::git::diff_cached(&t.workdir()).unwrap(),
        staged_before,
        "{err}"
    );
}

/// Regression: the autostash replay unstages a staged *modification* on a
/// rebase that completed, not only on one that was aborted.
#[test]
fn swap_keeps_staging_on_success() {
    let t = TestRepo::new_with_remote();
    let c1 = t.commit("First", "first.txt");
    let c2 = t.commit("Second", "second.txt");
    t.write_file("first.txt", "first\nstaged edit\n");
    t.write_file("brand-new.txt", "new\n");
    t.stage_files(&["first.txt", "brand-new.txt"]);
    t.write_file("second.txt", "second\nunstaged edit\n");
    let before = t.status_porcelain();

    super::swap_two_commits(&t.repo, c1.to_string(), c2.to_string()).unwrap();

    assert_eq!(t.status_porcelain(), before);
    // The letters alone would pass on a restore that staged the wrong bytes.
    assert_eq!(
        crate::git::run_git_stdout(&t.workdir(), &["show", ":first.txt"]).unwrap(),
        "first\nstaged edit\n"
    );
    assert_eq!(
        crate::git::run_git_stdout(&t.workdir(), &["show", ":brand-new.txt"]).unwrap(),
        "new\n"
    );
}

/// Same for the resumed half: `loom continue` finishes the rebase, so it owns
/// the restore the `Completed` arm would have done.
#[test]
fn swap_keeps_staging_across_continue() {
    let t = TestRepo::new_with_remote();

    // Both replays conflict on `shared.txt`: A creates it, B rewrites it, and
    // the swap replays each onto a tree the other has not touched yet.
    let a_oid = t.commit("version-a", "shared.txt");
    t.write_file("shared.txt", "version-b");
    t.stage_files(&["shared.txt"]);
    t.commit_staged("Commit B");
    let b_oid = t.head_oid();

    t.write_file("bystander.txt", "bystander\n");
    t.stage_files(&["bystander.txt"]);
    t.commit_staged("Commit C");
    t.write_file("bystander.txt", "bystander\nstaged edit\n");
    t.stage_files(&["bystander.txt"]);
    let before = t.status_porcelain();

    super::swap_two_commits(&t.repo, a_oid.to_string(), b_oid.to_string()).unwrap();

    let workdir = t.workdir();
    for content in ["version-b", "version-a"] {
        assert!(crate::git::rebase_is_in_progress(t.repo.path()));
        t.write_file("shared.txt", content);
        t.stage_files(&["shared.txt"]);
        crate::core::transaction::continue_cmd(&workdir, t.repo.path()).unwrap();
    }

    assert!(!crate::git::rebase_is_in_progress(t.repo.path()));
    assert_eq!(t.status_porcelain(), before);
}

/// The staged file is the one the replay conflicts on, so the autostash pop
/// conflicts too. Loom must leave that merge alone: the stages are the user's
/// to resolve and the staging lives in the stash git kept.
#[test]
fn swap_leaves_a_conflicted_autostash_pop_alone() {
    let t = TestRepo::new_with_remote();
    let a_oid = t.commit("version-a", "shared.txt");
    t.write_file("shared.txt", "version-b");
    t.stage_files(&["shared.txt"]);
    t.commit_staged("Commit B");
    let b_oid = t.head_oid();

    t.write_file("shared.txt", "staged-version");
    t.stage_files(&["shared.txt"]);
    t.write_file("shared.txt", "worktree-version");

    super::swap_two_commits(&t.repo, a_oid.to_string(), b_oid.to_string()).unwrap();
    let workdir = t.workdir();
    for content in ["version-b", "version-a"] {
        assert!(crate::git::rebase_is_in_progress(t.repo.path()));
        t.write_file("shared.txt", content);
        t.stage_files(&["shared.txt"]);
        crate::core::transaction::continue_cmd(&workdir, t.repo.path()).unwrap();
    }

    assert_eq!(t.status_porcelain(), "UU shared.txt\n");
    assert!(crate::git::has_unmerged_paths(&workdir));
    assert!(
        crate::git::run_git_stdout(&workdir, &["stash", "list"])
            .unwrap()
            .contains("autostash"),
        "the staging git could not replay stays in the stash"
    );
    // `git stash pop --index` is refused over an unmerged index, so the staged
    // side is handed over as a patch rather than left to the stash alone.
    let parked = crate::git::git_path(&workdir, "loom")
        .unwrap()
        .join("unrestored-staged-0.patch");
    assert!(parked.exists(), "the staged side is parked as well");
}

/// Git's autostash refuses to reset over a file removed from the index but
/// kept on disk, and leaves the rebase half-made: refused before it starts,
/// with nothing to undo.
#[test]
fn swap_refuses_a_file_removed_from_the_index_but_kept_on_disk() {
    let t = TestRepo::new_with_remote();
    t.commit("Add foo", "foo.txt");
    let c1 = t.commit("First", "first.txt");
    let c2 = t.commit("Second", "second.txt");
    let head = t.head_oid();
    crate::git::run_git(&t.workdir(), &["rm", "-q", "--cached", "foo.txt"]).unwrap();
    t.write_file("foo.txt", "my local edits\n");
    let before = t.status_porcelain();
    assert_eq!(before, "D  foo.txt\n?? foo.txt\n");

    let err = super::swap_two_commits(&t.repo, c1.to_string(), c2.to_string())
        .unwrap_err()
        .to_string();

    assert!(err.contains("`foo.txt`"), "{err}");
    assert!(!crate::git::rebase_is_in_progress(t.repo.path()));
    assert!(!t.repo.path().join("loom/state.json").exists());
    assert_eq!(t.head_oid(), head);
    assert_eq!(t.status_porcelain(), before);
    assert_eq!(t.read_file("foo.txt"), "my local edits\n");
}
