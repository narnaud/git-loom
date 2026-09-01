use std::path::Path;

use anyhow::Result;

/// Outcome of a rebase operation.
#[derive(Debug, PartialEq, Eq)]
pub enum RebaseOutcome {
    /// The rebase finished and git left no state behind.
    Completed,
    /// git exited successfully but the rebase is still in progress: it stopped
    /// at an `edit` or `break` step.
    Paused,
    /// The rebase stopped part-way and git left its state on disk. A conflict
    /// is the usual reason, but not the only one — see [`abort_after_failure`].
    Stopped,
}

/// Continue an in-progress rebase.
///
/// Returns `Completed` if the rebase finished, `Paused` if it advanced to the
/// next `edit`/`break` step, or `Stopped` if it stopped again. Does NOT abort —
/// the caller is responsible.
///
/// Sets `GIT_EDITOR=true` to suppress the editor for commit messages
/// during `--continue` (matching the suppression applied during the
/// initial rebase in `weave::run_rebase`).
pub fn continue_rebase(workdir: &Path) -> Result<RebaseOutcome> {
    use std::process::Command;
    use std::time::Instant;

    use crate::trace as loom_trace;

    // Resolve this before continuing: failing afterwards would report an error
    // for a rebase that already moved on, and the caller would keep its state
    // file for a step that is done.
    let git_dir = super::absolute_git_dir(workdir)?;

    let start = Instant::now();
    let output = Command::new("git")
        .current_dir(workdir)
        .args(["rebase", "--continue"])
        .env("GIT_EDITOR", "true")
        .output()?;

    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr);
    loom_trace::log_command(
        "git",
        "rebase --continue",
        duration_ms,
        output.status.success(),
        &stderr,
    );

    if !output.status.success() {
        return Ok(RebaseOutcome::Stopped);
    }

    // Exit 0 does not mean the rebase is over: git also exits 0 when it stops
    // at an `edit` or `break` step.
    if rebase_is_in_progress(&git_dir) {
        return Ok(RebaseOutcome::Paused);
    }

    Ok(RebaseOutcome::Completed)
}

/// Run a plain `git rebase` with the given extra args.
///
/// Returns `RebaseOutcome::Completed` on success, or
/// `RebaseOutcome::Stopped` if git left its state behind (detected by the
/// presence of `rebase-merge/` or `rebase-apply/`), whatever stopped it.
/// Any other failure (e.g., bad args) is returned as `Err`.
pub fn rebase(git_dir: &Path, workdir: &Path, upstream: &str) -> Result<RebaseOutcome> {
    match super::run_git(
        workdir,
        &[
            "rebase",
            "--autostash",
            "--update-refs",
            "--rebase-merges",
            upstream,
        ],
    ) {
        Ok(()) => Ok(RebaseOutcome::Completed),
        Err(e) => {
            if rebase_is_in_progress(git_dir) {
                Ok(RebaseOutcome::Stopped)
            } else {
                Err(e)
            }
        }
    }
}

/// Rebase commits between `upstream` and HEAD onto `newbase`.
///
/// Runs `git rebase --onto <newbase> <upstream> --update-refs`.
/// The `--update-refs` flag keeps any branch refs in the rebased range up to date.
#[cfg(test)]
pub fn rebase_onto(workdir: &Path, newbase: &str, upstream: &str) -> Result<()> {
    super::run_git(
        workdir,
        &[
            "rebase",
            "--onto",
            newbase,
            upstream,
            "--autostash",
            "--update-refs",
        ],
    )
}

/// Abort an in-progress rebase.
pub fn rebase_abort(workdir: &Path) -> Result<()> {
    super::run_git(workdir, &["rebase", "--abort"])
}

/// Check whether a rebase is currently in progress in the repository.
///
/// Detects the presence of `rebase-merge/` or `rebase-apply/` directories
/// under the git dir, which git creates when a rebase is paused.
pub fn rebase_is_in_progress(git_dir: &Path) -> bool {
    git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
}

/// Step numbers of a paused rebase, as `(current, total)`.
///
/// Read from the `msgnum`/`end` files git keeps in the rebase state directory.
/// Returns `None` if no rebase is in progress or the files are unreadable.
pub fn rebase_progress(git_dir: &Path) -> Option<(usize, usize)> {
    let dir = ["rebase-merge", "rebase-apply"]
        .iter()
        .map(|d| git_dir.join(d))
        .find(|d| d.exists())?;
    let read = |name: &str| -> Option<usize> {
        std::fs::read_to_string(dir.join(name))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    Some((read("msgnum")?, read("end")?))
}

/// Roll `cause` back: abort the rebase if one is running, then run `cleanup`.
///
/// The cleanup a caller wants here — deleting a temp branch, resetting refs,
/// restoring a saved patch, dropping the state file — assumes no rebase is
/// under way. So it runs in exactly two cases: the abort succeeded, or there
/// was no rebase to abort (the command failed before starting one). Only a
/// rebase that is still running after a *failed* abort skips it, because
/// resetting refs on top of that would make the mess worse.
///
/// Returns the error to report, which always keeps `cause` visible: the
/// top-level handler prints one message, so a `context` would hide the reason
/// the command failed in the first place.
pub fn rebase_abort_then_cleanup(
    workdir: &Path,
    cause: anyhow::Error,
    cleanup: impl FnOnce(),
) -> anyhow::Error {
    // If the git dir cannot be found, assume the worst and try the abort: a
    // skipped cleanup strands a temp branch, while cleaning up on top of a live
    // rebase can throw work away.
    let running = super::absolute_git_dir(workdir)
        .map(|git_dir| rebase_is_in_progress(&git_dir))
        .unwrap_or(true);

    if !running {
        cleanup();
        return cause;
    }

    match rebase_abort(workdir) {
        Ok(()) => {
            cleanup();
            cause
        }
        Err(_) => anyhow::anyhow!(
            "{cause}\n\
             The abort failed too, so the repository is left mid-rebase.\n\
             Run `loom abort` once git is free ({})",
            super::ABORT_FAILED_CAUSE
        ),
    }
}

/// Abort a rebase that failed, and build the error to report.
///
/// Two things the caller cannot assume: the rebase may have stopped for a
/// reason other than a conflict (a stale `index.lock`, a concurrent git
/// process), and the abort itself may fail — saying "aborted" then would strand
/// the user in a half-rewritten repository.
pub fn abort_after_failure(workdir: &Path) -> anyhow::Error {
    let conflicted = has_unmerged_paths(workdir);
    match rebase_abort(workdir) {
        Ok(()) if conflicted => anyhow::anyhow!("Rebase failed with conflicts — aborted"),
        Ok(()) => anyhow::anyhow!(
            "Rebase stopped before finishing — aborted\n\
             Run `loom trace` to see why"
        ),
        Err(_) => anyhow::anyhow!(
            "Rebase failed, and the abort failed too — the repository is left mid-rebase.\n\
             Run `loom abort` once git is free ({}).",
            super::ABORT_FAILED_CAUSE
        ),
    }
}

/// Whether the index has unmerged entries — i.e. the operation really did stop
/// on a conflict.
pub fn has_unmerged_paths(workdir: &Path) -> bool {
    super::run_git_stdout(workdir, &["diff", "--name-only", "--diff-filter=U"])
        .is_ok_and(|out| !out.trim().is_empty())
}

/// Continue a rebase whose todo this caller filled with `edit` steps, aborting
/// automatically on conflict.
///
/// Reaching the next `edit` is the expected outcome here, so `Paused` is
/// success. Only for callers that put those steps in the todo themselves
/// (`fold`'s edit-and-continue and multi-phase paths, `split`) — a caller whose
/// todo has none would take a rebase left mid-flight for a finished one.
pub fn continue_rebase_expecting_edit(workdir: &Path) -> Result<()> {
    match continue_rebase(workdir)? {
        RebaseOutcome::Completed | RebaseOutcome::Paused => Ok(()),
        RebaseOutcome::Stopped => Err(abort_after_failure(workdir)),
    }
}

#[cfg(test)]
#[path = "git_rebase_test.rs"]
mod tests;
