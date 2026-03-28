use anyhow::Result;

use crate::core::diff::{self, parse_hunk_start};
use crate::core::repo::{self, Target, TargetKind};
use crate::core::{graph, msg};
use crate::git;
use crate::tui::hunk_selector::{FileEntry, HunkEntry, HunkOrigin};
use crate::tui::theme::TuiTheme;

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

    // Collect file entries with status info and parsed hunks.
    let entries = collect_file_entries(&repo, &workdir, &files)?;

    if entries.is_empty() {
        msg::warn("No changes to stage");
        return Ok(());
    }

    // Launch TUI.
    let tui_theme = TuiTheme::from_graph_theme(theme);
    let result = crate::tui::hunk_selector::run_hunk_selector(entries, tui_theme)?;

    let selected_files = match result {
        None => {
            msg::error("Operation was canceled by the user");
            return Ok(());
        }
        Some(f) => f,
    };

    // Apply selections.
    apply_selections(&workdir, &selected_files)
}

/// Collect file entries with git status, staged/unstaged hunks, and proper initial selection.
fn collect_file_entries(
    repo: &git2::Repository,
    workdir: &std::path::Path,
    files: &[String],
) -> Result<Vec<FileEntry>> {
    let changes = repo::get_working_changes_recurse(repo)?;

    // Filter to requested files if specified.
    let filter_paths: Option<Vec<String>> = if files.is_empty() || files.iter().any(|f| f == "zz") {
        None
    } else {
        let mut resolved = Vec::new();
        for arg in files {
            match repo::resolve_arg(repo, arg, &[TargetKind::File])? {
                Target::File(path) => resolved.push(path),
                _ => unreachable!(),
            }
        }
        Some(resolved)
    };

    let mut entries = Vec::new();

    for change in &changes {
        // Skip if user specified files and this isn't one of them.
        if let Some(ref filter) = filter_paths
            && !filter.contains(&change.path)
        {
            continue;
        }

        let has_staged = matches!(change.index, 'A' | 'M' | 'D' | 'R');
        let has_unstaged = matches!(change.worktree, 'M' | 'D' | '?');

        if !has_staged && !has_unstaged {
            continue;
        }

        let mut hunks = Vec::new();
        let mut is_binary = false;

        // Collect staged hunks.
        if has_staged {
            is_binary |= collect_staged_hunks(workdir, &change.path, change.index, &mut hunks)?;
        }

        // Collect unstaged hunks.
        if has_unstaged {
            is_binary |=
                collect_unstaged_hunks(workdir, &change.path, change.worktree, &mut hunks)?;
        }

        if hunks.is_empty() {
            continue;
        }

        // Sort hunks by line number so staged and unstaged hunks interleave correctly.
        hunks.sort_by_key(|entry| hunk_sort_key(&entry.hunk));

        entries.push(FileEntry {
            path: change.path.clone(),
            hunks,
            index_status: change.index,
            worktree_status: change.worktree,
            binary: is_binary,
        });
    }

    Ok(entries)
}

/// Collect hunks from staged changes (HEAD → index).
///
/// Returns `true` if the file is binary.
fn collect_staged_hunks(
    workdir: &std::path::Path,
    path: &str,
    index_status: char,
    hunks: &mut Vec<HunkEntry>,
) -> Result<bool> {
    if index_status == 'D' {
        // Staged deletion — single entry, no diff content.
        hunks.push(HunkEntry {
            hunk: diff::DiffHunk {
                text: String::from("(file deleted)"),
                modified_lines: vec![],
            },
            selected: true,
            origin: HunkOrigin::Staged,
        });
        return Ok(false);
    }

    // For 'A', 'M', 'R' — get the staged diff.
    if git::diff_cached_file_is_binary(workdir, path)? {
        hunks.push(HunkEntry {
            hunk: diff::DiffHunk {
                text: String::from("(binary file)"),
                modified_lines: vec![],
            },
            selected: true,
            origin: HunkOrigin::Staged,
        });
        return Ok(true);
    }

    let raw_diff = git::diff_cached_file(workdir, path)?;
    if raw_diff.is_empty() {
        return Ok(false);
    }

    for h in diff::parse_hunks(&raw_diff) {
        hunks.push(HunkEntry {
            hunk: h,
            selected: true,
            origin: HunkOrigin::Staged,
        });
    }
    Ok(false)
}

/// Collect hunks from unstaged changes (index → worktree).
///
/// Returns `true` if the file is binary.
fn collect_unstaged_hunks(
    workdir: &std::path::Path,
    path: &str,
    worktree_status: char,
    hunks: &mut Vec<HunkEntry>,
) -> Result<bool> {
    if worktree_status == 'D' {
        // Unstaged deletion — single entry.
        hunks.push(HunkEntry {
            hunk: diff::DiffHunk {
                text: String::from("(file deleted)"),
                modified_lines: vec![],
            },
            selected: false,
            origin: HunkOrigin::Unstaged,
        });
        return Ok(false);
    }

    if worktree_status == '?' {
        // Untracked file — check if binary first.
        let full_path = workdir.join(path);
        let raw_bytes = match std::fs::read(&full_path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("warning: skipping unreadable file '{}'", path);
                return Ok(false);
            }
        };
        if raw_bytes.is_empty() {
            // Empty new file — still show it so it can be staged.
            hunks.push(HunkEntry {
                hunk: diff::DiffHunk {
                    text: String::from("(empty file)"),
                    modified_lines: vec![],
                },
                selected: false,
                origin: HunkOrigin::Unstaged,
            });
            return Ok(false);
        }
        // Detect binary: any null byte in the first 8 KB (same heuristic as git).
        let check_len = raw_bytes.len().min(8192);
        if raw_bytes[..check_len].contains(&0) {
            hunks.push(HunkEntry {
                hunk: diff::DiffHunk {
                    text: String::from("(binary file)"),
                    modified_lines: vec![],
                },
                selected: false,
                origin: HunkOrigin::Unstaged,
            });
            return Ok(true);
        }
        let content = String::from_utf8_lossy(&raw_bytes);
        let line_count = content.lines().count();
        let mut text = format!("@@ -0,0 +1,{} @@\n", line_count);
        for line in content.lines() {
            text.push('+');
            text.push_str(line);
            text.push('\n');
        }
        hunks.push(HunkEntry {
            hunk: diff::DiffHunk {
                text,
                modified_lines: vec![],
            },
            selected: false,
            origin: HunkOrigin::Unstaged,
        });
        return Ok(false);
    }

    // Regular unstaged modification.
    if git::diff_file_is_binary(workdir, path)? {
        hunks.push(HunkEntry {
            hunk: diff::DiffHunk {
                text: String::from("(binary file)"),
                modified_lines: vec![],
            },
            selected: false,
            origin: HunkOrigin::Unstaged,
        });
        return Ok(true);
    }

    let raw_diff = git::diff_file(workdir, path)?;
    if raw_diff.is_empty() {
        return Ok(false);
    }

    for h in diff::parse_hunks(&raw_diff) {
        hunks.push(HunkEntry {
            hunk: h,
            selected: false,
            origin: HunkOrigin::Unstaged,
        });
    }
    Ok(false)
}

/// Extract a sort key from a hunk for ordering by line position.
///
/// Uses the pre-image start line from the `@@` header. Special hunks
/// (deleted files, binary files) sort to the top.
fn hunk_sort_key(hunk: &diff::DiffHunk) -> usize {
    let first_line = hunk.text.lines().next().unwrap_or("");
    parse_hunk_start(first_line).unwrap_or(0)
}

/// Apply the user's selections: stage selected unstaged hunks, unstage deselected staged hunks.
fn apply_selections(workdir: &std::path::Path, files: &[FileEntry]) -> Result<()> {
    let mut to_stage_patch = String::new();
    let mut to_unstage_patch = String::new();
    let mut files_to_add: Vec<&str> = Vec::new();
    let mut files_to_unstage: Vec<&str> = Vec::new();
    let mut total_staged = 0usize;
    let mut total_unstaged = 0usize;
    let mut files_changed = 0usize;

    for file in files {
        let is_untracked = file.index_status == '?' && file.worktree_status == '?';
        let mut file_had_change = false;

        // Collect hunks to stage/unstage per file to build proper patches.
        let mut hunks_to_stage: Vec<&diff::DiffHunk> = Vec::new();
        let mut hunks_to_unstage: Vec<&diff::DiffHunk> = Vec::new();

        for entry in &file.hunks {
            match (entry.origin, entry.selected) {
                (HunkOrigin::Staged, true) => {
                    // Keep as-is — no action needed.
                }
                (HunkOrigin::Staged, false) => {
                    file_had_change = true;
                    if file.binary || file.index_status == 'D' || file.index_status == 'A' {
                        // Binary, deletion, or new file — unstage the whole file.
                        if !files_to_unstage.contains(&&*file.path) {
                            files_to_unstage.push(&file.path);
                        }
                    } else {
                        hunks_to_unstage.push(&entry.hunk);
                    }
                }
                (HunkOrigin::Unstaged, true) => {
                    file_had_change = true;
                    if file.binary || is_untracked || file.worktree_status == 'D' {
                        // Binary, untracked, or deletion — stage the whole file.
                        if !files_to_add.contains(&&*file.path) {
                            files_to_add.push(&file.path);
                        }
                    } else {
                        hunks_to_stage.push(&entry.hunk);
                    }
                }
                (HunkOrigin::Unstaged, false) => {
                    // Keep as-is — no action needed.
                }
            }
        }

        if !hunks_to_unstage.is_empty() {
            to_unstage_patch.push_str(&diff::build_hunk_patch(&file.path, &hunks_to_unstage));
            total_unstaged += hunks_to_unstage.len();
        }

        if !hunks_to_stage.is_empty() {
            to_stage_patch.push_str(&diff::build_hunk_patch(&file.path, &hunks_to_stage));
            total_staged += hunks_to_stage.len();
        }

        if file_had_change {
            files_changed += 1;
        }
    }

    // Apply unstaging first (reverse-apply staged hunks that were deselected).
    if !to_unstage_patch.is_empty() {
        git::apply_cached_patch_reverse(workdir, &to_unstage_patch)?;
    }

    // Unstage whole files (staged deletions/new files that were deselected).
    if !files_to_unstage.is_empty() {
        git::unstage_files(workdir, &files_to_unstage)?;
    }

    // Apply staging (apply selected unstaged hunks to the index).
    if !to_stage_patch.is_empty() {
        git::apply_cached_patch(workdir, &to_stage_patch)?;
    }

    // Stage whole files (untracked files / deletions that were selected).
    if !files_to_add.is_empty() {
        git::stage_files(workdir, &files_to_add)?;
    }

    let total_ops = total_staged + total_unstaged + files_to_add.len() + files_to_unstage.len();
    if total_ops == 0 {
        msg::warn("No changes to apply");
    } else {
        msg::success(&format!(
            "Applied {} change(s) across {} file(s)",
            total_ops, files_changed
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "add_test.rs"]
mod tests;

#[cfg(test)]
#[path = "add_patch_test.rs"]
mod patch_tests;
