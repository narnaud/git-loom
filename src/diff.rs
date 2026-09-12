use anyhow::Result;

use crate::core::repo::{self, Target, TargetKind};
use crate::git;

/// Show a diff using short IDs (like `git diff`).
///
/// By default shows unstaged changes (working tree vs index), like `git diff`.
/// `--staged` shows staged changes (index vs HEAD); `--all` shows everything
/// (working tree vs HEAD).
///
/// `git_args` is whatever followed a `--`, appended after the revisions and
/// ahead of the pathspec loom builds from file targets (see spec 021).
pub fn run(args: Vec<String>, staged: bool, all: bool, git_args: Vec<String>) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "diff")?;

    let mut cmd: Vec<String> = vec!["diff".to_string()];
    if staged {
        cmd.push("--staged".to_string());
    }

    let mut file_paths: Vec<String> = Vec::new();
    let mut has_commits = false;

    for arg in &args {
        if let Some((left, right)) = arg.split_once("..") {
            // Commit range: resolve each side leniently (short IDs or raw refs like HEAD)
            let resolved_left = resolve_ref_leniently(&repo, left);
            let resolved_right = resolve_ref_leniently(&repo, right);
            cmd.push(format!("{}..{}", resolved_left, resolved_right));
            has_commits = true;
        } else {
            // Try to resolve as a file (short ID or path) or commit (short ID or hash)
            let resolved = repo::resolve_arg(&repo, arg, &[TargetKind::File, TargetKind::Commit])?;
            match resolved {
                Target::File(path) => file_paths.push(path),
                Target::Commit(hash) => {
                    cmd.push(hash);
                    has_commits = true;
                }
                _ => unreachable!(),
            }
        }
    }

    // With `--all` and no explicit commit, diff the working tree against HEAD so
    // both staged and unstaged changes are shown in a single view.
    if all && !has_commits {
        cmd.push("HEAD".to_string());
    }

    cmd.extend(git_args);

    if !file_paths.is_empty() {
        cmd.push("--".to_string());
        cmd.extend(file_paths);
    }

    let refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
    git::run_git_interactive(workdir, &refs)
}

/// Resolve a commit reference leniently: tries short ID and direct ref resolution.
/// Falls back to the raw string for refs that can't be resolved (HEAD, tags, etc.)
/// and does not reject merge commits, making it suitable for range endpoints.
fn resolve_ref_leniently(repo: &git2::Repository, arg: &str) -> String {
    match repo::resolve_arg(repo, arg, &[TargetKind::Commit]) {
        Ok(Target::Commit(hash)) => hash,
        _ => arg.to_string(),
    }
}

#[cfg(test)]
#[path = "diff_test.rs"]
mod tests;
