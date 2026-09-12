use anyhow::{Result, bail};

use crate::core::repo::{self, Target, TargetKind};
use crate::core::staging;
use crate::core::{graph, msg};
use crate::git;

/// Stage files into the index using short IDs, filenames, or `zz` for all.
/// With `--patch`, opens an interactive hunk selector TUI.
///
/// `git_args` is whatever followed a `--`, passed to `git add` ahead of the
/// pathspec (see spec 021).
pub fn run(
    files: Vec<String>,
    patch: bool,
    git_args: Vec<String>,
    theme: &graph::Theme,
) -> Result<()> {
    // No files means the hunk picker too, so the message names the mode rather
    // than the flag. The picker stages by applying a patch, so there is no
    // `git add` for forwarded arguments to reach.
    if patch || files.is_empty() {
        if !git_args.is_empty() {
            bail!(
                "staging hunks interactively takes no `git add` arguments after `--`\n\
                 Files go before the separator: `loom add <files> -- <git args>`"
            );
        }
        return run_patch(files, theme);
    }

    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "add")?.to_path_buf();
    let git_opts: Vec<&str> = git_args.iter().map(String::as_str).collect();

    // Forwarded arguments decide what `git add` actually did — `--dry-run`
    // stages nothing — so loom reports only when it owns every argument and
    // otherwise leaves git's own output to speak.
    let report = |text: &str| {
        if git_opts.is_empty() {
            msg::success(text);
        }
    };

    // `zz` stages everything, regardless of other args.
    if files.iter().any(|f| f == "zz") {
        git::stage_all_opts(&workdir, &git_opts)?;
        report("Staged all changes");
        return Ok(());
    }

    // Resolve each argument to a file path.
    let mut paths = Vec::new();
    for arg in &files {
        match repo::resolve_arg(&repo, arg, &[TargetKind::File])? {
            Target::File(path) => paths.push(path),
            _ => unreachable!(),
        }
    }

    let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    git::stage_files_opts(&workdir, &path_refs, &git_opts)?;

    report(&format!("Staged {} file(s)", paths.len()));
    Ok(())
}

/// Interactive patch mode: collect diffs, launch TUI, apply selected hunks.
fn run_patch(files: Vec<String>, theme: &graph::Theme) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "add")?.to_path_buf();

    let confirmed = staging::run_hunk_picker(&repo, &workdir, &files, theme)?;
    if !confirmed {
        bail!("Cancelled");
    }
    Ok(())
}

#[cfg(test)]
#[path = "add_test.rs"]
mod tests;

#[cfg(test)]
#[path = "add_patch_test.rs"]
mod patch_tests;
