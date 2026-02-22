/// Merge a branch into the current branch, forcing a merge commit.
///
/// Wraps `git merge --no-ff <branch> --no-edit`.
/// Use this when a fast-forward merge would skip creating the merge topology.
#[cfg(test)]
pub fn merge_no_ff(workdir: &std::path::Path, branch: &str) -> anyhow::Result<()> {
    super::run_git(workdir, &["merge", "--no-ff", branch, "--no-edit"])
}
