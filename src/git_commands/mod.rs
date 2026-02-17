pub mod git_branch;
pub mod git_commit;
pub mod git_merge;
pub mod git_rebase;

use std::path::Path;
use std::process::Command;

/// Minimum Git version required (--update-refs was added in 2.38).
const MIN_GIT_VERSION: (u32, u32) = (2, 38);

/// Run a git command in the given working directory.
/// On failure, returns an error containing stderr output.
pub fn run_git(workdir: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .current_dir(workdir)
        .args(args)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {} failed:\n{}", args.join(" "), stderr).into());
    }

    Ok(())
}

/// Check that the installed Git version meets the minimum requirement.
/// Returns an error with an actionable message if the version is too old.
pub fn check_git_version() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git").arg("--version").output()?;
    let version_str = String::from_utf8_lossy(&output.stdout);

    // Parse "git version X.Y.Z..." → (X, Y)
    let (major, minor) = parse_git_version(&version_str)
        .ok_or_else(|| format!("Could not parse Git version from: {}", version_str.trim()))?;

    if (major, minor) < MIN_GIT_VERSION {
        return Err(format!(
            "Git {}.{} is too old. git-loom requires Git {}.{} or later (for --update-refs).\n\
             Current version: {}",
            major,
            minor,
            MIN_GIT_VERSION.0,
            MIN_GIT_VERSION.1,
            version_str.trim()
        )
        .into());
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
pub fn loom_exe_path() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
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
