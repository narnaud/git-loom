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
/// next `edit`/`break` step, or `Stopped` if it stopped again. A failure that
/// leaves no rebase in progress is an error. Does NOT abort — the caller is
/// responsible.
pub fn continue_rebase(workdir: &Path) -> Result<RebaseOutcome> {
    // Resolve this before continuing: failing afterwards would report an error
    // for a rebase that already moved on, and the caller would keep its state
    // file for a step that is done.
    let git_dir = super::absolute_git_dir(workdir)?;

    rebase_outcome(&git_dir, super::run_git(workdir, &["rebase", "--continue"]))
}

/// Classify how a `git rebase …` command ended, from whether it succeeded and
/// whether git left its state behind (`rebase-merge/` or `rebase-apply/`).
///
/// Exit 0 does not mean the rebase is over: git also exits 0 when it stops at
/// an `edit` or `break` step (`Paused`). A failure that left state behind is
/// `Stopped`, whatever stopped it; any other failure (bad args, missing ref)
/// is returned as `Err`.
pub fn rebase_outcome(git_dir: &Path, result: Result<()>) -> Result<RebaseOutcome> {
    let in_progress = rebase_is_in_progress(git_dir);
    match result {
        Ok(()) if in_progress => Ok(RebaseOutcome::Paused),
        Ok(()) => Ok(RebaseOutcome::Completed),
        Err(_) if in_progress => Ok(RebaseOutcome::Stopped),
        Err(e) => Err(e),
    }
}

/// Run a plain `git rebase` onto `upstream`; see [`rebase_outcome`] for the
/// result.
pub fn rebase(git_dir: &Path, workdir: &Path, upstream: &str) -> Result<RebaseOutcome> {
    rebase_outcome(
        git_dir,
        super::run_git(
            workdir,
            &[
                "rebase",
                "--autostash",
                "--update-refs",
                "--rebase-merges",
                upstream,
            ],
        ),
    )
}

/// Rebase commits between `upstream` and HEAD onto `newbase`
/// (`git rebase --onto <newbase> <upstream> --update-refs`), keeping branch
/// refs in the range up to date.
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

/// Whether a rebase is in progress: git leaves a `rebase-merge/` or
/// `rebase-apply/` directory under the git dir while one is paused.
pub fn rebase_is_in_progress(git_dir: &Path) -> bool {
    git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
}

/// Step numbers of a paused rebase as `(current, total)`, read from the
/// `msgnum`/`end` files in git's rebase state dir. `None` when no rebase is in
/// progress or the files are unreadable.
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
    let conflicted = has_unmerged_paths(workdir) || auto_merge_id(workdir).is_some();
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

/// The id of `AUTO_MERGE`, the ref git keeps while a conflicted merge is
/// unfinished — a conflicted pick during a rebase included — and drops once the
/// resolution is committed. `Some` therefore means the stop came from a
/// conflict, resolved or not, and the id names *which* conflict, so a caller
/// that read it before continuing can tell a fresh one from the one it was on.
///
/// Asks git rather than looking for a file: `AUTO_MERGE` is a ref, and the
/// reftable backend keeps no file of that name. Only the `ort` strategy writes
/// it, so under any other this reports `None` and the caller falls back to its
/// generic message.
pub fn auto_merge_id(workdir: &Path) -> Option<String> {
    let out =
        super::run_git_stdout(workdir, &["rev-parse", "--verify", "--quiet", "AUTO_MERGE"]).ok()?;
    let id = out.trim().to_string();
    (!id.is_empty()).then_some(id)
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
