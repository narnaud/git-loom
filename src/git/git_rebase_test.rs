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
