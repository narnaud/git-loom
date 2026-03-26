pub mod git_apply;
pub mod git_branch;
pub mod git_commit;
pub mod git_diff;
pub mod git_merge;
pub mod git_rebase;

pub use git_apply::{apply_cached_patch, apply_patch, apply_patch_reverse, restore_staged_patch};
pub use git_branch::{
    branch_create, branch_delete, branch_force_create, branch_rename, branch_switch,
    branch_switch_create_tracking, branch_switch_detach, branch_validate_name,
};
pub use git_commit::{
    commit, commit_amend, commit_amend_no_edit, commit_with_editor, reset_hard, reset_mixed,
    stage_all, stage_files, stage_path,
};
pub use git_diff::{
    diff_cached_files, diff_commit, diff_commit_file, diff_head, diff_head_file, diff_head_files,
    diff_head_name_only,
};
pub use git_merge::{MergeOutcome, continue_merge, merge_abort, merge_is_in_progress, merge_no_ff};
#[cfg(test)]
pub use git_rebase::rebase_onto;
pub use git_rebase::{
    RebaseOutcome, continue_rebase, continue_rebase_or_abort, rebase, rebase_abort,
    rebase_is_in_progress,
};

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::trace as loom_trace;

/// Minimum Git version required (--update-refs was added in 2.38).
const MIN_GIT_VERSION: (u32, u32) = (2, 38);

/// Run a git command, capture output, trace-log it, and bail on failure.
fn run_git_captured(workdir: &Path, args: &[&str]) -> Result<std::process::Output> {
    let start = Instant::now();
    let output = Command::new("git")
        .current_dir(workdir)
        .args(args)
        .output()?;

    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let cmd = args.join(" ");
    loom_trace::log_command("git", &cmd, duration_ms, output.status.success(), &stderr);

    if !output.status.success() {
        bail!("Git {} failed", cmd);
    }

    Ok(output)
}

/// Run a git command in the given working directory.
/// On failure, returns an error with the command name; stderr is recorded
/// in the trace log via `loom_trace::log_command`.
pub fn run_git(workdir: &Path, args: &[&str]) -> Result<()> {
    run_git_captured(workdir, args).map(|_| ())
}

/// Run a git command and return its stdout as a string.
/// On failure, returns an error with the command name; stderr is recorded
/// in the trace log via `loom_trace::log_command`.
pub fn run_git_stdout(workdir: &Path, args: &[&str]) -> Result<String> {
    let output = run_git_captured(workdir, args)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Check that the installed Git version meets the minimum requirement.
/// Returns an error with an actionable message if the version is too old.
pub fn check_git_version() -> Result<()> {
    let output = Command::new("git").arg("--version").output()?;
    let version_str = String::from_utf8_lossy(&output.stdout);

    // Parse "git version X.Y.Z..." → (X, Y)
    let (major, minor) = parse_git_version(&version_str)
        .with_context(|| format!("Could not parse Git version from: {}", version_str.trim()))?;

    if (major, minor) < MIN_GIT_VERSION {
        bail!(
            "Git {}.{} is too old, git-loom requires Git {}.{} or later (for --update-refs)\n\
             Current version: {}",
            major,
            minor,
            MIN_GIT_VERSION.0,
            MIN_GIT_VERSION.1,
            version_str.trim()
        );
    }

    Ok(())
}

/// Parse "git version X.Y.Z..." into (major, minor).
fn parse_git_version(version_str: &str) -> Option<(u32, u32)> {
    let version_part = version_str.trim().strip_prefix("git version ")?;
    let mut parts = version_part.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Run a git command with inherited stdio (for interactive commands / pager).
/// On failure, returns an error containing the command that failed.
///
/// Note: stderr is not captured (it flows to the terminal directly),
/// so the trace log will record an empty stderr string for these calls.
pub fn run_git_interactive(workdir: &Path, args: &[&str]) -> Result<()> {
    let start = Instant::now();
    let status = Command::new("git")
        .current_dir(workdir)
        .args(args)
        .status()?;

    let duration_ms = start.elapsed().as_millis();
    let cmd = args.join(" ");
    loom_trace::log_command("git", &cmd, duration_ms, status.success(), "");

    if !status.success() {
        bail!("Git {} failed", cmd);
    }

    Ok(())
}

/// Unstage specific files (remove from index without touching the working tree).
///
/// Wraps `git reset HEAD -- <files>`.
pub fn unstage_files(workdir: &Path, files: &[&str]) -> Result<()> {
    let mut args = vec!["reset", "HEAD", "--"];
    args.extend(files);
    run_git(workdir, &args)
}

/// Restore tracked files in the working tree to their HEAD state.
///
/// Wraps `git checkout HEAD -- <files>`.
pub fn restore_files_to_head(workdir: &Path, files: &[&str]) -> Result<()> {
    let mut args = vec!["checkout", "HEAD", "--"];
    args.extend(files);
    run_git(workdir, &args)
}

/// Resolve a git ref to its full commit hash.
///
/// Wraps `git rev-parse <ref>` and trims the output.
pub fn rev_parse(workdir: &Path, reference: &str) -> Result<String> {
    let out = run_git_stdout(workdir, &["rev-parse", reference])?;
    Ok(out.trim().to_string())
}

/// Truncate a full commit hash to a short display form (7 chars).
pub fn short_hash(hash: &str) -> &str {
    &hash[..7.min(hash.len())]
}

/// Resolve the path to the git-loom binary.
///
/// During `cargo test`, `current_exe()` returns the test harness binary in
/// `target/<profile>/deps/`. The actual git-loom binary lives one level up
/// in `target/<profile>/`. This function detects that case and returns the
/// correct path.
pub fn loom_exe_path() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    if let Some(parent) = exe.parent()
        && parent.file_name().and_then(|n| n.to_str()) == Some("deps")
    {
        let bin_name = if cfg!(windows) {
            "git-loom.exe"
        } else {
            "git-loom"
        };
        if let Some(profile_dir) = parent.parent() {
            let actual = profile_dir.join(bin_name);
            if actual.exists() {
                return Ok(actual);
            }
        }
    }
    Ok(exe)
}
