use std::path::Path;

use anyhow::Result;

use super::run_git_stdout;

/// Get the diff for a single commit (its changes relative to its parent).
///
/// Wraps `git diff <oid>^..<oid>`.
pub fn diff_commit(workdir: &Path, oid: &str) -> Result<String> {
    run_git_stdout(workdir, &["diff", &format!("{}^..{}", oid, oid)])
}

/// Get the diff for a single file within a commit (relative to its parent).
///
/// Wraps `git diff <oid>^..<oid> -- <path>`.
pub fn diff_commit_file(workdir: &Path, oid: &str, path: &str) -> Result<String> {
    run_git_stdout(
        workdir,
        &["diff", &format!("{}^..{}", oid, oid), "--", path],
    )
}

/// Get the staged (cached) diff for specific files.
///
/// Wraps `git diff --cached -- <files>`. Returns an empty string if the
/// files have no staged changes.
pub fn diff_cached_files(workdir: &Path, files: &[&str]) -> Result<String> {
    let mut args = vec!["diff", "--cached", "--"];
    args.extend(files);
    run_git_stdout(workdir, &args)
}

/// Get the diff of all tracked files against HEAD (name-only).
///
/// Wraps `git diff HEAD --name-only`. Returns one filename per line.
pub fn diff_head_name_only(workdir: &Path) -> Result<String> {
    run_git_stdout(workdir, &["diff", "HEAD", "--name-only"])
}

/// Get the unified diff for a single file against HEAD.
///
/// Wraps `git diff HEAD -- <path>`.
pub fn diff_head_file(workdir: &Path, path: &str) -> Result<String> {
    run_git_stdout(workdir, &["diff", "HEAD", "--", path])
}

/// Get the full unified diff of all working-tree changes against HEAD.
///
/// Wraps `git diff HEAD`.
pub fn diff_head(workdir: &Path) -> Result<String> {
    run_git_stdout(workdir, &["diff", "HEAD"])
}

/// Get the unified diff for specific files against HEAD.
///
/// Wraps `git diff HEAD -- <files>`.
pub fn diff_head_files(workdir: &Path, files: &[&str]) -> Result<String> {
    let mut args = vec!["diff", "HEAD", "--"];
    args.extend(files);
    run_git_stdout(workdir, &args)
}
