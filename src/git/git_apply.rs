use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};

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

/// Apply a patch to the working tree, falling back to a three-way merge.
///
/// A plain `git apply` fails as soon as a later commit changed a line inside a
/// hunk's context, even though the change itself still fits — the usual case
/// when a commit's own diff is replayed onto a history where that commit is
/// gone but its successors remain. `--3way` merges through the blob ids in the
/// diff header instead, so only a real overlap fails.
///
/// On failure the working tree is put back as it was, best effort: `--3way`
/// writes the files it could merge even when it conflicts on another.
///
/// The patch must carry `index` lines with blob ids (`git diff --full-index`)
/// for the three-way to be possible at all; without them git refuses the
/// fallback and only the plain apply's result stands.
pub fn apply_patch_to_worktree(workdir: &Path, patch: &str) -> Result<()> {
    let Err(plain) = apply_patch(workdir, patch) else {
        return Ok(());
    };

    // What the apply changes has to be undone if it fails, and a patch's own
    // paths do not spell that out — `--numstat` names only a rename's new side.
    // The working tree does: whatever newly differs from the index, the apply
    // wrote. Files the user had already changed cannot be among them — `--3way`
    // refuses a patch that touches one.
    let Ok(before) = super::worktree_dirty_paths(workdir) else {
        return Err(plain);
    };
    // The outer error is the fallback failing to even start, which says more
    // than the plain apply did; the inner one is git refusing the patch.
    if apply_patch_three_way(workdir, patch)?.is_err() {
        let written: Vec<String> = super::worktree_dirty_paths(workdir)
            .unwrap_or_default()
            .difference(&before)
            .cloned()
            .collect();
        // Both attempts are in the trace with their own stderr. The two errors
        // carry the same message, so hand back the one the caller asked for.
        restore_from_index(workdir, &written);
        return Err(plain);
    }
    Ok(())
}

/// Run the three-way apply against a throwaway copy of the index.
///
/// `--3way` implies `--index`, but these changes belong in the working tree
/// alone. A scratch index keeps the real one — staged work, intent-to-add
/// entries, the stat cache — untouched, and leaves no conflicted entries to
/// clean up. The outer `Result` is the setup failing, the inner one is git
/// refusing the patch. `rerere` is off because this merge is loom replaying a patch, not
/// a conflict the user ever resolved: recording it, or silently replaying an
/// earlier resolution over it, would both be wrong.
fn apply_patch_three_way(workdir: &Path, patch: &str) -> Result<Result<()>> {
    let index = super::git_path(workdir, "index")?;
    // Named per process: two looms in one repo must not share a scratch index.
    let scratch = index.with_file_name(format!("loom-apply-index-{}", std::process::id()));
    std::fs::copy(&index, &scratch).with_context(|| {
        format!(
            "Failed to copy '{}' to '{}'",
            index.display(),
            scratch.display()
        )
    })?;

    let result = run_apply(
        workdir,
        patch,
        &["-c", "rerere.enabled=false"],
        &["--3way"],
        Some(&scratch),
    );

    let _ = std::fs::remove_file(&scratch);
    Ok(result)
}

/// Put the working-tree files a failed apply had already written back the way
/// they were: from the index for the tracked ones, by deletion for the ones the
/// patch created. Best-effort — the caller is already reporting a failure.
fn restore_from_index(workdir: &Path, paths: &[String]) {
    // Nothing written, nothing to put back. Falling through with an empty list
    // would ask git about every path it knows instead of about none.
    if paths.is_empty() {
        return;
    }
    let refs: Vec<&str> = paths.iter().map(|p| p.as_str()).collect();
    let tracked = super::ls_files(workdir, &refs).unwrap_or_default();
    if !tracked.is_empty() {
        let tracked_refs: Vec<&str> = tracked.iter().map(|p| p.as_str()).collect();
        let _ = super::checkout_index_force(workdir, &tracked_refs);
    }
    for path in paths {
        if !tracked.contains(path) {
            let _ = std::fs::remove_file(workdir.join(path));
        }
    }
}

/// Apply a patch to the index only (not the working tree).
///
/// Wraps `git apply --cached` with the patch passed via stdin.
pub fn apply_cached_patch(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &["--cached"])
}

/// Reverse-apply a patch from the index only (unstage specific hunks).
///
/// Wraps `git apply --cached --reverse` with the patch passed via stdin.
pub fn apply_cached_patch_reverse(workdir: &Path, patch: &str) -> Result<()> {
    apply_patch_with_flags(workdir, patch, &["--cached", "--reverse"])
}

fn apply_patch_with_flags(workdir: &Path, patch: &str, flags: &[&str]) -> Result<()> {
    run_apply(workdir, patch, &[], flags, None)
}

/// Run `git apply` with `flags`, `config` as extra `-c` settings, and the index
/// at `index_file` when one is given.
fn run_apply(
    workdir: &Path,
    patch: &str,
    config: &[&str],
    flags: &[&str],
    index_file: Option<&Path>,
) -> Result<()> {
    let mut args = vec!["apply"];
    args.extend(flags);

    let start = Instant::now();
    let mut command = Command::new("git");
    command
        .current_dir(workdir)
        .args(super::FORCED_CONFIG)
        .args(config)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(index) = index_file {
        command.env("GIT_INDEX_FILE", index);
    }
    let mut child = command.spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(patch.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The trace has to show what actually ran: the extra `-c` settings and the
    // redirected index change the outcome, and this is the command someone
    // reads back when a three-way went wrong.
    let mut logged: Vec<&str> = config.to_vec();
    logged.extend(&args);
    let index_note = index_file.map(|i| format!("GIT_INDEX_FILE={}", i.display()));
    if let Some(note) = &index_note {
        logged.insert(0, note);
    }
    loom_trace::log_command(
        "git",
        &logged.join(" "),
        duration_ms,
        output.status.success(),
        &stderr,
    );

    if !output.status.success() {
        bail!("git apply failed");
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

#[cfg(test)]
#[path = "git_apply_test.rs"]
mod tests;
