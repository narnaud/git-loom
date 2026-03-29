use anyhow::Result;
use git2::Repository;
use std::path::Path;

use crate::core::diff::{self, parse_hunk_start};
use crate::core::repo::{self, Target, TargetKind};
use crate::core::{graph, msg};
use crate::git;
use crate::tui::hunk_selector::{FileEntry, HunkEntry, HunkOrigin};
use crate::tui::theme::TuiTheme;

/// Open the interactive hunk picker for the given files (or all if empty / `zz`).
///
/// Returns `true` if the user confirmed, `false` if they cancelled.
pub fn run_hunk_picker(
    repo: &Repository,
    workdir: &Path,
    files: &[String],
    theme: &graph::Theme,
) -> Result<bool> {
    let entries = collect_file_entries(repo, workdir, files)?;

    if entries.is_empty() {
        msg::warn("No changes to stage");
        return Ok(false);
    }

    let tui_theme = TuiTheme::from_graph_theme(theme);
    let result = crate::tui::hunk_selector::run_hunk_selector(entries, tui_theme)?;

    match result {
        None => Ok(false),
        Some(selected_files) => {
            apply_selections(workdir, &selected_files)?;
            Ok(true)
        }
    }
}

/// Collect file entries with git status, staged/unstaged hunks, and proper initial selection.
pub(crate) fn collect_file_entries(
    repo: &Repository,
    workdir: &Path,
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

        if has_staged {
            is_binary |= collect_staged_hunks(workdir, &change.path, change.index, &mut hunks)?;
        }

        if has_unstaged {
            is_binary |=
                collect_unstaged_hunks(workdir, &change.path, change.worktree, &mut hunks)?;
        }

        if hunks.is_empty() {
            continue;
        }

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

/// Apply the user's selections: stage selected unstaged hunks, unstage deselected staged hunks.
pub(crate) fn apply_selections(workdir: &Path, files: &[FileEntry]) -> Result<()> {
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

        let mut hunks_to_stage: Vec<&diff::DiffHunk> = Vec::new();
        let mut hunks_to_unstage: Vec<&diff::DiffHunk> = Vec::new();

        for entry in &file.hunks {
            match (entry.origin, entry.selected) {
                (HunkOrigin::Staged, true) => {}
                (HunkOrigin::Staged, false) => {
                    file_had_change = true;
                    if file.binary || file.index_status == 'D' || file.index_status == 'A' {
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
                        if !files_to_add.contains(&&*file.path) {
                            files_to_add.push(&file.path);
                        }
                    } else {
                        hunks_to_stage.push(&entry.hunk);
                    }
                }
                (HunkOrigin::Unstaged, false) => {}
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

    if !files_to_unstage.is_empty() {
        git::unstage_files(workdir, &files_to_unstage)?;
    }

    // Apply staging (apply selected unstaged hunks to the index).
    if !to_stage_patch.is_empty() {
        git::apply_cached_patch(workdir, &to_stage_patch)?;
    }

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

/// Collect hunks from staged changes (HEAD → index).
///
/// Returns `true` if the file is binary.
fn collect_staged_hunks(
    workdir: &Path,
    path: &str,
    index_status: char,
    hunks: &mut Vec<HunkEntry>,
) -> Result<bool> {
    if index_status == 'D' {
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
    workdir: &Path,
    path: &str,
    worktree_status: char,
    hunks: &mut Vec<HunkEntry>,
) -> Result<bool> {
    if worktree_status == 'D' {
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
        let full_path = workdir.join(path);
        let raw_bytes = match std::fs::read(&full_path) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("warning: skipping unreadable file '{}'", path);
                return Ok(false);
            }
        };
        if raw_bytes.is_empty() {
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

fn hunk_sort_key(hunk: &diff::DiffHunk) -> usize {
    let first_line = hunk.text.lines().next().unwrap_or("");
    parse_hunk_start(first_line).unwrap_or(0)
}
