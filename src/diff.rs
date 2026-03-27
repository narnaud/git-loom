use anyhow::Result;

use crate::core::repo::{self, Target, TargetKind};
use crate::git;

/// Show a diff using short IDs (like `git diff`).
pub fn run(args: Vec<String>) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "diff")?;

    if args.is_empty() {
        return git::run_git_interactive(workdir, &["diff"]);
    }

    let mut git_args: Vec<String> = vec!["diff".to_string()];
    let mut file_paths: Vec<String> = Vec::new();
    let mut has_commits = false;

    for arg in &args {
        if let Some((left, right)) = arg.split_once("..") {
            // Commit range: resolve each side leniently (short IDs or raw refs like HEAD)
            let resolved_left = resolve_ref_leniently(&repo, left);
            let resolved_right = resolve_ref_leniently(&repo, right);
            git_args.push(format!("{}..{}", resolved_left, resolved_right));
            has_commits = true;
        } else {
            // Try to resolve as a file (short ID or path) or commit (short ID or hash)
            let resolved = repo::resolve_arg(&repo, arg, &[TargetKind::File, TargetKind::Commit])?;
            match resolved {
                Target::File(path) => file_paths.push(path),
                Target::Commit(hash) => {
                    git_args.push(hash);
                    has_commits = true;
                }
                _ => unreachable!(),
            }
        }
    }

    // File diffs are always shown against HEAD so staged changes are included.
    if !file_paths.is_empty() {
        if !has_commits {
            git_args.push("HEAD".to_string());
        }
        git_args.push("--".to_string());
        git_args.extend(file_paths);
    }

    let refs: Vec<&str> = git_args.iter().map(|s| s.as_str()).collect();
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
