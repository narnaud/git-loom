use std::path::Path;

/// Merge a branch into the current branch.
///
/// Wraps `git merge <branch> --no-edit`.
pub fn merge(workdir: &Path, branch: &str) -> Result<(), Box<dyn std::error::Error>> {
    super::run_git(workdir, &["merge", branch, "--no-edit"])
}
