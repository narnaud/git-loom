pub mod git_branch;
pub mod git_commit;
pub mod git_merge;
pub mod git_rebase;

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Minimum Git version required (--update-refs was added in 2.38).
const MIN_GIT_VERSION: (u32, u32) = (2, 38);

/// Run a git command in the given working directory.
/// On failure, returns an error containing stderr output.
pub fn run_git(workdir: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .current_dir(workdir)
        .args(args)
        .output()?;

    if !output.status.success() {
        bail!("Git {} failed", args.join(" "));
    }

    Ok(())
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

/// Run a git command and return its stdout as a string.
/// On failure, returns an error containing stderr output.
pub fn run_git_stdout(workdir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(workdir)
        .args(args)
        .output()?;

    if !output.status.success() {
        bail!("Git {} failed", args.join(" "));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
    apply_patch_impl(workdir, patch, false)
}

/// Apply a patch in reverse from stdin.
///
/// Wraps `git apply --reverse` with the patch passed via stdin.
pub fn apply_patch_reverse(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_impl(workdir, patch, true)
}

fn apply_patch_impl(workdir: &Path, patch: &str, reverse: bool) -> Result<()> {
    let mut args = vec!["apply"];
    if reverse {
        args.push("--reverse");
    }

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
    if !output.status.success() {
        let flag = if reverse { " --reverse" } else { "" };
        bail!("Git apply{} failed", flag);
    }

    Ok(())
}

/// Get the diff of currently staged (cached) changes.
///
/// Wraps `git diff --cached`. Returns the patch text, or empty string if
/// nothing is staged.
pub fn diff_cached(workdir: &Path) -> Result<String> {
    run_git_stdout(workdir, &["diff", "--cached"])
}

/// Unstage specific files (remove from index without touching the working tree).
///
/// Wraps `git reset HEAD -- <files>`.
pub fn unstage_files(workdir: &Path, files: &[&str]) -> Result<()> {
    let mut args = vec!["reset", "HEAD", "--"];
    args.extend(files);
    run_git(workdir, &args)
}

/// Restore files in the working tree to their HEAD state.
///
/// Wraps `git checkout HEAD -- <files>`. Discards working-tree changes for the
/// specified files without affecting other files or the index.
pub fn restore_files_to_head(workdir: &Path, files: &[&str]) -> Result<()> {
    let mut args = vec!["checkout", "HEAD", "--"];
    args.extend(files);
    run_git(workdir, &args)
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
