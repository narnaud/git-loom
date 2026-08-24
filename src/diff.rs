use anyhow::{Result, bail};

use crate::core::cli_args;
use crate::core::repo::{self, Target, TargetKind};
use crate::git;

/// The flags `diff` owns; everything else hyphenated goes to `git diff`.
const LOOM_FLAGS: &[&str] = &["--staged", "--cached", "-a", "--all"];

/// Show a diff using short IDs (like `git diff`).
///
/// By default shows unstaged changes (working tree vs index), like `git diff`.
/// `--staged` shows staged changes (index vs HEAD); `--all` shows everything
/// (working tree vs HEAD). Options loom doesn't define are forwarded to
/// `git diff` unchanged.
pub fn run(args: Vec<String>, staged: bool, all: bool) -> Result<()> {
    let split = cli_args::split(&args, LOOM_FLAGS);

    // clap only sees loom's own flags when they precede the first positional;
    // after that they land in `args`, so re-apply whatever `split` recaptured.
    let is_flag = |names: [&str; 2]| split.loom_flags.iter().any(|f| names.contains(&f.as_str()));
    let staged = staged || is_flag(["--staged", "--cached"]);
    let all = all || is_flag(["-a", "--all"]);
    if staged && all {
        bail!("--staged and --all are mutually exclusive");
    }

    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "diff")?;

    let mut git_args: Vec<String> = vec!["diff".to_string()];
    if staged {
        git_args.push("--staged".to_string());
    }
    git_args.extend(split.options);

    let mut file_paths: Vec<String> = split.pathspec;
    let mut has_commits = false;

    for arg in &split.targets {
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

    // With `--all` and no explicit commit, diff the working tree against HEAD so
    // both staged and unstaged changes are shown in a single view.
    if all && !has_commits {
        git_args.push("HEAD".to_string());
    }

    if !file_paths.is_empty() {
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
