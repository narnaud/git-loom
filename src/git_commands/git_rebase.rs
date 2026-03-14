use std::path::Path;

use anyhow::{Result, bail};

/// Outcome of a rebase operation.
pub enum RebaseOutcome {
    Completed,
    Conflicted,
}

/// Continue an in-progress rebase.
///
/// Returns `Completed` if the rebase finished without conflicts,
/// or `Conflicted` if a new conflict was encountered.
/// Does NOT abort on conflict — the caller is responsible.
pub fn continue_rebase(workdir: &Path) -> Result<RebaseOutcome> {
    if super::run_git(workdir, &["rebase", "--continue"]).is_ok() {
        Ok(RebaseOutcome::Completed)
    } else {
        Ok(RebaseOutcome::Conflicted)
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
pub fn abort(workdir: &Path) -> Result<()> {
    super::run_git(workdir, &["rebase", "--abort"])
}

/// Check whether a rebase is currently in progress in the repository.
///
/// Detects the presence of `rebase-merge/` or `rebase-apply/` directories
/// under the git dir, which git creates when a rebase is paused.
pub fn is_in_progress(git_dir: &Path) -> bool {
    git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
}

/// Continue an in-progress rebase, aborting automatically on conflict.
///
/// Used by out-of-scope callers (`fold` edit-and-continue paths,
/// `split`, `reword`) that want the old hard-fail behavior.
pub fn continue_rebase_or_abort(workdir: &Path) -> Result<()> {
    match continue_rebase(workdir)? {
        RebaseOutcome::Completed => Ok(()),
        RebaseOutcome::Conflicted => {
            let _ = abort(workdir);
            bail!("Rebase failed with conflicts — aborted");
        }
    }
}
