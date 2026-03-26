use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{Result, bail};

use crate::trace as loom_trace;

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
