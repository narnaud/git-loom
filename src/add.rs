use anyhow::Result;

use crate::core::repo::{self, Target, TargetKind};
use crate::core::staging;
use crate::core::{graph, msg};
use crate::git;

/// Stage files into the index using short IDs, filenames, or `zz` for all.
/// With `--patch`, opens an interactive hunk selector TUI.
pub fn run(files: Vec<String>, patch: bool, theme: &graph::Theme) -> Result<()> {
    if patch || files.is_empty() {
        return run_patch(files, theme);
    }

    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "add")?.to_path_buf();

    // `zz` stages everything, regardless of other args.
    if files.iter().any(|f| f == "zz") {
        git::stage_all(&workdir)?;
        msg::success("Staged all changes");
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
    git::stage_files(&workdir, &path_refs)?;

    msg::success(&format!("Staged {} file(s)", paths.len()));
    Ok(())
}

/// Interactive patch mode: collect diffs, launch TUI, apply selected hunks.
fn run_patch(files: Vec<String>, theme: &graph::Theme) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "add")?.to_path_buf();

    let confirmed = staging::run_hunk_picker(&repo, &workdir, &files, theme)?;
    if !confirmed {
        msg::error("Operation was canceled by the user");
    }
    Ok(())
}

#[cfg(test)]
#[path = "add_test.rs"]
mod tests;

#[cfg(test)]
#[path = "add_patch_test.rs"]
mod patch_tests;
