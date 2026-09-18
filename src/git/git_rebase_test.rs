use crate::core::test_helpers::TestRepo;
use crate::core::weave;
use crate::trace;

/// Regression (#159): `continue_rebase` must capture git's output and route it
/// to the trace, instead of running with inherited stdio and leaking git's
/// "Successfully rebased" / "Updated refs" messages to the terminal. Before the
/// fix it logged nothing at all, so asserting the trace records the step proves
/// the output is now captured (you can only log stderr you captured).
#[test]
fn continue_rebase_captures_output_to_trace() {
    let test_repo = TestRepo::new();
    let c1 = test_repo.commit("first", "a.txt");
    test_repo.commit("second", "b.txt");
    let workdir = test_repo.workdir();

    // Pause a rebase at the first commit so there is something to continue.
    weave::start_edit_rebase(&test_repo.repo, &workdir, c1).unwrap();

    // The trace logger is thread-local and cargo reuses threads across tests;
    // clear any logger a prior test leaked so our init reliably takes effect.
    let _ = trace::finalize();
    let git_dir = test_repo.repo.path().to_path_buf();
    trace::init(&git_dir, "git loom fold");
    let outcome = super::continue_rebase(&workdir).unwrap();
    let log_path = trace::finalize().expect("trace should have recorded an entry");

    assert!(matches!(outcome, super::RebaseOutcome::Completed));
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        content.contains("[git] rebase --continue"),
        "trace should record the continue step, got:\n{content}"
    );
}

/// A rebase whose todo still has an `edit` step ahead of it is not over when
/// `git rebase --continue` exits 0: it merely advanced to that next step.
/// Reporting `Completed` there would let the caller finish off a command while
/// the repository sits detached mid-rebase.
#[test]
fn continue_rebase_reports_paused_at_next_edit() {
    let test_repo = TestRepo::new();
    let base = test_repo.commit("base", "base.txt");
    let c1 = test_repo.commit("first", "a.txt");
    let c2 = test_repo.commit("second", "b.txt");
    let c3 = test_repo.commit("third", "c.txt");
    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();

    // Two `edit` steps: the rebase stops at the first, and continuing from it
    // stops at the second.
    let todo = format!("label onto\n\nreset onto\nedit {c1}\nedit {c2}\npick {c3}\n");
    assert_eq!(
        weave::run_rebase(&workdir, Some(&base.to_string()), &todo).unwrap(),
        super::RebaseOutcome::Paused,
        "the rebase stops at the first `edit`, it has not completed"
    );

    assert_eq!(
        super::continue_rebase(&workdir).unwrap(),
        super::RebaseOutcome::Paused,
        "the second `edit` is still ahead — the rebase is not over"
    );

    // Only once the last `edit` is passed does it actually finish.
    assert_eq!(
        super::continue_rebase(&workdir).unwrap(),
        super::RebaseOutcome::Completed
    );
    assert!(!super::rebase_is_in_progress(&git_dir));
}

/// `git rebase --continue` with no rebase in progress is a caller bug, not a
/// conflict: reporting `Stopped` would send the user off to resolve conflicts
/// that do not exist.
#[test]
fn continue_rebase_without_a_rebase_is_an_error() {
    let test_repo = TestRepo::new();
    test_repo.commit("first", "a.txt");
    let workdir = test_repo.workdir();

    let err = super::continue_rebase(&workdir).unwrap_err();
    assert!(
        err.to_string().contains("git rebase failed"),
        "expected the rebase failure itself, got: {err}"
    );
}

/// A command can fail before its rebase ever starts — the worktree check, the
/// git-dir lookup, a missing loom binary. There is nothing to abort then, so
/// the cleanup must still run: skipping it strands the temp branch, saved
/// patch or state file the caller was about to remove.
#[test]
fn cleanup_runs_when_there_was_no_rebase_to_abort() {
    let test_repo = TestRepo::new();
    test_repo.commit("first", "a.txt");
    let workdir = test_repo.workdir();
    assert!(!super::rebase_is_in_progress(test_repo.repo.path()));

    let mut cleaned = false;
    let err =
        super::rebase_abort_then_cleanup(&workdir, anyhow::anyhow!("boom"), || cleaned = true);

    assert!(cleaned, "nothing was running, so the cleanup must happen");
    assert_eq!(
        err.to_string(),
        "boom",
        "the command's own failure is what the user needs to see"
    );
}

/// With a rebase actually running, the abort has to happen before the cleanup,
/// and the caller's error is still the one reported.
#[test]
fn a_live_rebase_is_aborted_before_the_cleanup_runs() {
    let test_repo = TestRepo::new();
    let base = test_repo.commit("base", "base.txt");
    let c1 = test_repo.commit("first", "a.txt");
    let workdir = test_repo.workdir();

    let todo = format!("label onto\n\nreset onto\nedit {c1}\n");
    weave::run_rebase(&workdir, Some(&base.to_string()), &todo).unwrap();
    assert!(super::rebase_is_in_progress(test_repo.repo.path()));

    let mut cleaned = false;
    let err =
        super::rebase_abort_then_cleanup(&workdir, anyhow::anyhow!("boom"), || cleaned = true);

    assert!(cleaned, "the abort worked, so the cleanup must follow");
    assert!(
        !super::rebase_is_in_progress(test_repo.repo.path()),
        "the rebase should be gone"
    );
    assert_eq!(err.to_string(), "boom");
}

/// When the abort fails the rebase is still running, so the cleanup is skipped
/// — and the reported error must still carry the original failure, not replace
/// it with the hint.
#[test]
fn a_failed_abort_skips_the_cleanup_and_keeps_the_cause() {
    let test_repo = TestRepo::new();
    let base = test_repo.commit("base", "base.txt");
    let c1 = test_repo.commit("first", "a.txt");
    let workdir = test_repo.workdir();

    let todo = format!("label onto\n\nreset onto\nedit {c1}\n");
    weave::run_rebase(&workdir, Some(&base.to_string()), &todo).unwrap();

    // A held index.lock makes the abort fail, as a concurrent git process would.
    let lock = test_repo.repo.path().join("index.lock");
    std::fs::write(&lock, b"").unwrap();

    let mut cleaned = false;
    let err =
        super::rebase_abort_then_cleanup(&workdir, anyhow::anyhow!("boom"), || cleaned = true);

    assert!(
        !cleaned,
        "cleaning up on top of a live rebase is what this guards against"
    );
    let msg = err.to_string();
    assert!(msg.contains("boom"), "the cause must survive, got: {msg}");
    assert!(msg.contains("left mid-rebase"), "{msg}");

    std::fs::remove_file(&lock).unwrap();
    super::rebase_abort(&workdir).unwrap();
}

/// Build a repo where `rerere` has recorded a resolution for a conflict, then
/// replay that same conflict in a rebase. With `rerere.autoUpdate` on, git
/// stages the recorded resolution and the stop leaves a clean index — which
/// must not be mistaken for a rebase that broke down.
#[test]
fn rerere_resolved_stop_is_still_a_conflict() {
    let test_repo = TestRepo::new();
    test_repo.set_config("rerere.enabled", "true");
    test_repo.set_config("rerere.autoUpdate", "true");
    let workdir = test_repo.workdir();

    test_repo.write_file("f.txt", "base\n");
    test_repo.stage_files(&["f.txt"]);
    test_repo.commit_staged("base");
    let base = test_repo.head_oid().to_string();

    test_repo.write_file("f.txt", "onto side\n");
    test_repo.stage_files(&["f.txt"]);
    test_repo.commit_staged("onto side");
    let onto = test_repo.head_oid().to_string();

    // The same conflicting topic twice: the first rebase records the
    // resolution, the second one has rerere replay it.
    let conflict_on = |topic: &str| {
        test_repo.create_branch_at(topic, &base);
        test_repo.switch_branch(topic);
        test_repo.write_file("f.txt", "topic side\n");
        test_repo.stage_files(&["f.txt"]);
        test_repo.commit_staged("topic side");
        crate::git::run_git(&workdir, &["rebase", &onto]).unwrap_err();
        assert!(
            super::rebase_is_in_progress(test_repo.repo.path()),
            "the conflict must be what stopped the rebase"
        );
    };

    conflict_on("topic1");
    test_repo.write_file("f.txt", "resolved\n");
    crate::git::run_git(&workdir, &["add", "f.txt"]).unwrap();
    assert_eq!(
        super::continue_rebase(&workdir).unwrap(),
        super::RebaseOutcome::Completed
    );

    conflict_on("topic2");

    assert!(
        super::rebase_is_in_progress(test_repo.repo.path()),
        "the replayed conflict still stops the rebase"
    );
    assert_eq!(
        test_repo.read_file("f.txt"),
        "resolved\n",
        "rerere should have replayed the recorded resolution"
    );
    assert!(
        !super::has_unmerged_paths(&workdir),
        "rerere staged its resolution, so nothing is left unmerged"
    );
    assert!(
        super::auto_merge_id(&workdir).is_some(),
        "the stop must still count as a conflict"
    );

    let err = super::abort_after_failure(&workdir).to_string();
    assert!(
        err.contains("Rebase failed with conflicts"),
        "a stop rerere resolved is still a conflict to report: {err}"
    );
}

/// The reftable backend keeps refs in `.git/reftable/`, so `AUTO_MERGE` is no
/// file under the git dir there — reading it must go through git.
#[test]
fn auto_merge_id_works_on_a_reftable_repo() {
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().to_path_buf();
    let init = std::process::Command::new("git")
        .current_dir(&workdir)
        .args(["init", "--ref-format=reftable"])
        .output()
        .unwrap();
    if !init.status.success() {
        eprintln!("skipping: this git has no reftable backend");
        return;
    }
    for (key, value) in [("user.name", "Test"), ("user.email", "test@example.com")] {
        crate::git::run_git(&workdir, &["config", key, value]).unwrap();
    }

    let write = |content: &str| std::fs::write(workdir.join("f.txt"), content).unwrap();
    let commit = |message: &str| {
        crate::git::run_git(&workdir, &["add", "f.txt"]).unwrap();
        crate::git::run_git(&workdir, &["commit", "-m", message]).unwrap();
    };
    write("base\n");
    commit("base");
    crate::git::run_git(&workdir, &["branch", "topic"]).unwrap();
    write("onto side\n");
    commit("onto side");
    let onto = crate::git::run_git_stdout(&workdir, &["rev-parse", "HEAD"]).unwrap();
    crate::git::run_git(&workdir, &["switch", "topic"]).unwrap();
    write("topic side\n");
    commit("topic side");
    crate::git::run_git(&workdir, &["rebase", onto.trim()]).unwrap_err();

    assert!(
        !workdir.join(".git/AUTO_MERGE").exists(),
        "reftable keeps no AUTO_MERGE file — that is the point of this test"
    );
    assert!(
        super::auto_merge_id(&workdir).is_some(),
        "the conflict must be recognized on a reftable repo too"
    );
}

/// An untracked file in the way of a picked commit stops the rebase with a
/// clean index and no conflict — with `rerere` enabled too, which is what made
/// `MERGE_RR` useless as a signal: git writes it for any sequencer pick.
#[test]
fn untracked_file_stop_is_not_a_conflict() {
    let test_repo = TestRepo::new();
    test_repo.set_config("rerere.enabled", "true");
    let workdir = test_repo.workdir();

    test_repo.commit("base", "a.txt");
    let base = test_repo.head_oid().to_string();
    test_repo.write_file("foo.txt", "committed\n");
    test_repo.stage_files(&["foo.txt"]);
    crate::git::run_git(&workdir, &["commit", "-m", "add foo.txt"]).unwrap();
    let add_foo = test_repo.head_oid().to_string();
    crate::git::run_git(&workdir, &["rm", "-q", "foo.txt"]).unwrap();
    crate::git::run_git(&workdir, &["commit", "-m", "delete foo.txt"]).unwrap();
    let delete_foo = test_repo.head_oid().to_string();

    // Replaying "add foo.txt" on top of the deletion cannot write the file:
    // the user has an untracked one there.
    test_repo.write_file("foo.txt", "untracked\n");
    crate::git::run_git(
        &workdir,
        &["rebase", "--onto", &delete_foo, &base, &add_foo],
    )
    .unwrap_err();

    assert!(
        super::rebase_is_in_progress(test_repo.repo.path()),
        "the blocked pick stops the rebase"
    );
    assert!(!super::has_unmerged_paths(&workdir));
    assert!(
        super::auto_merge_id(&workdir).is_none(),
        "an untracked file in the way is not a conflict"
    );
}

#[test]
fn rebase_outcome_classifies_all_four_cases() {
    use super::{RebaseOutcome, rebase_outcome};
    let tmp = tempfile::tempdir().unwrap();
    let git_dir = tmp.path();
    let fail = || Err(anyhow::anyhow!("boom"));

    assert_eq!(
        rebase_outcome(git_dir, Ok(())).unwrap(),
        RebaseOutcome::Completed
    );
    assert!(rebase_outcome(git_dir, fail()).is_err());

    std::fs::create_dir(git_dir.join("rebase-merge")).unwrap();
    assert_eq!(
        rebase_outcome(git_dir, Ok(())).unwrap(),
        RebaseOutcome::Paused
    );
    assert_eq!(
        rebase_outcome(git_dir, fail()).unwrap(),
        RebaseOutcome::Stopped
    );
}

#[test]
fn verify_paused_at_refuses_a_stop_on_another_commit() {
    let test_repo = TestRepo::new();
    let base = test_repo.commit("base", "base.txt");
    let c1 = test_repo.commit("first", "a.txt");
    let c2 = test_repo.commit("second", "b.txt");
    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();

    let todo = format!("label onto\n\nreset onto\nedit {c1}\npick {c2}\n");
    weave::run_rebase(&workdir, Some(&base.to_string()), &todo).unwrap();
    assert!(crate::git::rebase_is_in_progress(&git_dir));

    // HEAD is the replay of c1, so verifying it against c2 must refuse.
    let err = crate::git::verify_paused_at(&workdir, &c2.to_string())
        .unwrap_err()
        .to_string();

    assert!(err.contains("was not replayed"), "{err}");
    assert!(!crate::git::rebase_is_in_progress(&git_dir), "{err}");
}

/// Author and message alone cannot tell a commit from its cherry-picked
/// duplicate, which is exactly the history this whole check exists for.
#[test]
fn verify_paused_at_refuses_a_stop_on_a_commit_of_the_same_identity() {
    let test_repo = TestRepo::new();
    let base = test_repo.commit("base", "base.txt");
    let c1 = test_repo.commit_at("dup", "a.txt", 1_000);
    let c2 = test_repo.commit_at("dup", "b.txt", 1_000);
    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();

    let todo = format!("label onto\n\nreset onto\nedit {c1}\nedit {c2}\n");
    weave::run_rebase(&workdir, Some(&base.to_string()), &todo).unwrap();
    assert!(crate::git::rebase_is_in_progress(&git_dir));

    let err = crate::git::verify_paused_at(&workdir, &c2.to_string())
        .unwrap_err()
        .to_string();

    assert!(err.contains("was not replayed"), "{err}");
    assert!(!crate::git::rebase_is_in_progress(&git_dir), "{err}");
}

/// `git rebase --skip` is a hard reset, so a stop that only looks empty must
/// never be skipped over work done while the rebase was paused.
#[test]
fn an_empty_stop_is_not_skipped_over_local_changes() {
    let test_repo = TestRepo::new();
    let main = test_repo.current_branch_name();
    test_repo.write_file("other.txt", "keep\n");
    test_repo.write_file("shared.txt", "start\n");
    test_repo.stage_files(&["other.txt", "shared.txt"]);
    test_repo.commit_staged("start");

    test_repo.create_branch("side");
    test_repo.switch_branch("side");
    test_repo.write_file("shared.txt", "final\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("side change");

    // The same content reaches the other line in two steps, so no patch-id
    // matches and the emptiness only shows when `side` is replayed onto it.
    test_repo.switch_branch(&main);
    test_repo.write_file("shared.txt", "middle\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("upstream step one");
    test_repo.write_file("shared.txt", "final\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("upstream step two");

    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();
    let empty = format!("--empty={}", crate::git::empty_stop_value());
    let _ = crate::git::run_git(&workdir, &["rebase", &empty, "HEAD", "side"]);
    assert!(crate::git::rebase_is_in_progress(&git_dir));

    test_repo.write_file("other.txt", "edited while paused\n");

    let outcome =
        crate::git::skip_empty_stops(&workdir, &git_dir, &[], crate::git::RebaseOutcome::Stopped)
            .unwrap();

    assert_eq!(outcome, crate::git::RebaseOutcome::Stopped);
    assert_eq!(test_repo.read_file("other.txt"), "edited while paused\n");
    crate::git::rebase_abort(&workdir).unwrap();
}

/// The replay of a commit never keeps its hash, so the check has to recognize
/// it by what a rebase does preserve.
#[test]
fn verify_paused_at_accepts_a_replay_with_a_new_hash() {
    let test_repo = TestRepo::new();
    let base = test_repo.commit("base", "base.txt");
    let target = test_repo.commit("target", "target.txt");

    test_repo.reset_hard(base);
    let onto = test_repo.commit("upstream", "upstream.txt");

    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();
    let todo = format!("label onto\n\nreset onto\nedit {target}\n");
    weave::run_rebase(&workdir, Some(&onto.to_string()), &todo).unwrap();

    assert!(crate::git::rebase_is_in_progress(&git_dir));
    assert_ne!(test_repo.head_oid(), target);

    crate::git::verify_paused_at(&workdir, &target.to_string()).unwrap();
    crate::git::rebase_abort(&workdir).unwrap();
}

/// The discriminator behind the skip: `git rebase --skip` is a hard reset, so
/// it runs only for a commit the repository says would add nothing.
#[test]
fn replays_empty_only_when_the_content_is_already_there() {
    let test_repo = TestRepo::new();
    let main = test_repo.current_branch_name();
    test_repo.write_file("shared.txt", "start\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("start");

    test_repo.create_branch("side");
    test_repo.switch_branch("side");
    test_repo.write_file("shared.txt", "side\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("side change");
    let side_change = test_repo.head_oid().to_string();

    let workdir = test_repo.workdir();
    test_repo.switch_branch(&main);
    assert!(!super::replays_empty(&workdir, &side_change));

    // The same content, reached independently: now it would add nothing.
    test_repo.write_file("shared.txt", "side\n");
    test_repo.stage_files(&["shared.txt"]);
    test_repo.commit_staged("same content, other commit");
    assert!(super::replays_empty(&workdir, &side_change));
}

/// Upstream taking the change *and* editing around it is the common shape, and
/// the replay is still empty — asking which files the commit touched would say
/// otherwise.
#[test]
fn replays_empty_when_the_upstream_also_changed_the_same_file() {
    let test_repo = TestRepo::new();
    let main = test_repo.current_branch_name();
    test_repo.write_file("f.txt", "1\n2\n3\n");
    test_repo.stage_files(&["f.txt"]);
    test_repo.commit_staged("base");

    test_repo.create_branch("side");
    test_repo.switch_branch("side");
    test_repo.write_file("f.txt", "1\n2\nX\n3\n");
    test_repo.stage_files(&["f.txt"]);
    test_repo.commit_staged("branch adds X");
    let side_change = test_repo.head_oid().to_string();

    test_repo.switch_branch(&main);
    test_repo.write_file("f.txt", "1\n2\nX\n3\n4\n");
    test_repo.stage_files(&["f.txt"]);
    test_repo.commit_staged("upstream adds X and 4");

    assert!(super::replays_empty(&test_repo.workdir(), &side_change));
}

/// Git quotes a non-ASCII path in its plumbing output, so a check built on
/// path lists read it back as "nothing to compare" — i.e. as empty.
#[test]
fn a_commit_touching_a_non_ascii_path_does_not_look_empty() {
    let test_repo = TestRepo::new();
    let main = test_repo.current_branch_name();
    test_repo.write_file("base.txt", "base\n");
    test_repo.stage_files(&["base.txt"]);
    test_repo.commit_staged("base");

    test_repo.create_branch("side");
    test_repo.switch_branch("side");
    test_repo.write_file("café.txt", "content\n");
    test_repo.stage_files(&["café.txt"]);
    test_repo.commit_staged("add an accented path");
    let side_change = test_repo.head_oid().to_string();

    let workdir = test_repo.workdir();
    test_repo.switch_branch(&main);
    assert!(!super::replays_empty(&workdir, &side_change));
}

/// A submodule's own worktree is not the superproject's to lose: autostash
/// never stashes it, so counting it would leave the guard permanently on and
/// every empty stop deadlocked.
#[test]
fn a_dirty_submodule_is_not_a_local_change() {
    let test_repo = crate::core::test_helpers::TestRepo::new();
    let (first, second) = test_repo.add_submodule("sub");
    test_repo.commit_staged("Add submodule");
    let workdir = test_repo.workdir();
    assert!(!super::has_local_changes(&workdir));

    // Dirty inside the submodule only: the superproject has nothing to stash.
    test_repo.write_file("sub/dirty.txt", "dirty\n");
    crate::git::run_git(&workdir.join("sub"), &["add", "dirty.txt"]).unwrap();
    assert!(!super::has_local_changes(&workdir));

    // A gitlink the index does move is still a change a `--skip` would eat.
    test_repo.checkout_submodule("sub", second);
    test_repo.stage_files(&["sub"]);
    assert!(super::has_local_changes(&workdir));
    let _ = first;
}

/// A stop loom will not act on is not a conflict for the user to resolve, so
/// the refusal fires whatever the tree holds — but the abort behind it is a
/// hard reset, so over pause-time edits it refuses and touches nothing.
#[test]
fn a_protected_commit_is_refused_even_with_local_changes() {
    let (t, keeper) = crate::core::test_helpers::repo_with_a_redundant_commit_above();
    let workdir = t.workdir();
    let git_dir = t.repo.path().to_path_buf();

    let mut graph = weave::Weave::from_repo(&t.repo).unwrap();
    assert!(graph.edit_commit(keeper));
    weave::run_rebase_protecting(
        &workdir,
        Some(&graph.base_oid.to_string()),
        &graph.to_todo(),
        &[],
    )
    .unwrap();

    // What a user does while an operation is paused.
    t.write_file("three.txt", "edited while paused\n");
    assert!(super::has_local_changes(&workdir));

    let outcome = crate::git::continue_rebase(&workdir).unwrap();
    let stopped = super::stopped_sha(&git_dir).expect("a stop to classify");
    assert!(
        super::replays_empty(&workdir, &stopped),
        "the fixture must stop on an empty replay"
    );

    let protect = [stopped];
    let err = crate::git::skip_empty_stops(&workdir, &git_dir, &protect, outcome)
        .unwrap_err()
        .to_string();

    assert!(err.contains("is redundant"), "{err}");
    assert_eq!(
        t.read_file("three.txt"),
        "edited while paused\n",
        "an abort here is a hard reset over the user's own edits: {err}"
    );
    assert!(
        crate::git::rebase_is_in_progress(&git_dir),
        "nothing was undone: {err}"
    );
    assert!(
        !err.contains("run `loom abort`") || err.contains("stash"),
        "`loom abort` is the same hard reset, so it is not the bare advice: {err}"
    );
    crate::git::rebase_abort(&workdir).unwrap();
}
