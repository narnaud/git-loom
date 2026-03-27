use std::path::Path;

use anyhow::Result;

/// Outcome of a merge operation.
pub enum MergeOutcome {
    Completed,
    Conflicted,
}

/// Merge a branch into the current branch, forcing a merge commit.
///
/// Wraps `git merge --no-ff <branch> --no-edit`.
/// Returns `Conflicted` if the merge stopped due to conflicts,
/// `Completed` on success, or `Err` on any other failure.
pub fn merge_no_ff(workdir: &Path, git_dir: &Path, branch: &str) -> Result<MergeOutcome> {
    run_merge_cmd(workdir, git_dir, &["merge", "--no-ff", branch, "--no-edit"])
}

/// Continue an in-progress merge (equivalent to `git merge --continue`).
///
/// Note: `--continue` does not accept extra flags like `--no-edit`. The merge
/// commit message is taken from `MERGE_MSG` without opening an editor.
pub fn continue_merge(workdir: &Path, git_dir: &Path) -> Result<MergeOutcome> {
    run_merge_cmd(workdir, git_dir, &["merge", "--continue"])
}

fn run_merge_cmd(workdir: &Path, git_dir: &Path, args: &[&str]) -> Result<MergeOutcome> {
    match super::run_git(workdir, args) {
        Ok(()) => Ok(MergeOutcome::Completed),
        Err(e) => {
            if merge_is_in_progress(git_dir) {
                Ok(MergeOutcome::Conflicted)
            } else {
                Err(e)
            }
        }
    }
}

/// Abort an in-progress merge.
pub fn merge_abort(workdir: &Path) -> Result<()> {
    super::run_git(workdir, &["merge", "--abort"])
}

/// Check whether a merge is currently in progress.
///
/// Detects the presence of `MERGE_HEAD`, which git creates when a merge is paused.
pub fn merge_is_in_progress(git_dir: &Path) -> bool {
    git_dir.join("MERGE_HEAD").exists()
}
