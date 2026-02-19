use std::path::Path;

/// Merge a branch into the current branch, forcing a merge commit.
///
/// Wraps `git merge --no-ff <branch> --no-edit`.
/// Use this when a fast-forward merge would skip creating the merge topology.
pub fn merge_no_ff(workdir: &Path, branch: &str) -> Result<(), Box<dyn std::error::Error>> {
    super::run_git(workdir, &["merge", "--no-ff", branch, "--no-edit"])
}
