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
