pub mod git_branch;
pub mod git_commit;
pub mod git_merge;
pub mod git_rebase;

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

/// Apply a patch from stdin.
///
/// Wraps `git apply` with the patch passed via stdin.
pub fn apply_patch(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &[])
}

/// Apply a patch in reverse from stdin.
///
/// Wraps `git apply --reverse` with the patch passed via stdin.
pub fn apply_patch_reverse(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &["--reverse"])
}

/// Apply a patch to the index only (not the working tree).
///
/// Wraps `git apply --cached` with the patch passed via stdin.
pub fn apply_cached_patch(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &["--cached"])
}

fn apply_patch_with_flags(workdir: &Path, patch: &str, flags: &[&str]) -> Result<()> {
    let mut args = vec!["apply"];
    args.extend(flags);

    let start = Instant::now();
    let mut child = Command::new("git")
        .current_dir(workdir)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(patch.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr);
    loom_trace::log_command(
        "git",
        &args.join(" "),
        duration_ms,
        output.status.success(),
        &stderr,
    );

    if !output.status.success() {
        let flag = args[1..].join(" ");
        bail!("Git apply {} failed", flag);
    }

    Ok(())
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

/// Resolve a git ref to its full commit hash.
///
/// Wraps `git rev-parse <ref>` and trims the output.
pub fn rev_parse(workdir: &Path, reference: &str) -> Result<String> {
    let out = run_git_stdout(workdir, &["rev-parse", reference])?;
    Ok(out.trim().to_string())
}

/// Re-apply a previously saved staged patch, warning on failure.
///
/// No-ops if `patch` is empty. On failure, emits a warning to stderr — the
/// primary operation has already succeeded, so this is best-effort.
pub fn restore_staged_patch(workdir: &Path, patch: &str) -> Result<()> {
    if !patch.is_empty()
        && let Err(e) = apply_cached_patch(workdir, patch)
    {
        eprintln!(
            "Warning: could not restore pre-existing staged changes: {}",
            e
        );
    }
    Ok(())
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
