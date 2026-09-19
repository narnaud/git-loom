use anyhow::{Result, bail};
use git2::Repository;
use std::path::Path;

use crate::core::diff::{self, parse_hunk_start};
use crate::core::hunk_select::{self, Picker};
use crate::core::repo;
use crate::core::{agent_mode, graph, msg};
use crate::git;
use crate::tui::hunk_selector::{FileEntry, HunkEntry, HunkOrigin};
use crate::tui::theme::TuiTheme;

/// Open the interactive hunk picker for the given files (or all if empty / `zz`).
///
/// Returns the paths the picker staged — those with a selected hunk — or
/// `None` if it was cancelled. Never a path the picker did not show (Spec 007).
pub fn run_hunk_picker(
    repo: &Repository,
    workdir: &Path,
    filter: Option<&[String]>,
    theme: &graph::Theme,
) -> Result<Option<Vec<String>>> {
    let entries = collect_file_entries(repo, workdir, filter)?;

    if entries.is_empty() {
        msg::warn("No changes to stage");
        return Ok(None);
    }

    let tui_theme = TuiTheme::from_graph_theme(theme);
    let result = crate::tui::hunk_selector::run_hunk_selector(entries, tui_theme)?;

    match result {
        None => Ok(None),
        Some(selected_files) => {
            apply_selections(workdir, &selected_files)?;
            Ok(Some(selected_paths(&selected_files)))
        }
    }
}

/// The paths the picker staged: those with at least one selected hunk.
///
/// An entry the picker skipped — a staged change with no hunk to show, such as
/// a mode-only one — is not here, because nobody could pick it.
fn selected_paths(files: &[FileEntry]) -> Vec<String> {
    files
        .iter()
        .filter(|file| file.hunks.iter().any(|hunk| hunk.selected))
        .map(|file| file.path.clone())
        .collect()
}

/// The paths a picker is narrowed to, or `None` for every change (no files, or `zz`).
pub(crate) fn filter_paths(repo: &Repository, files: &[String]) -> Result<Option<Vec<String>>> {
    if files.is_empty() || files.iter().any(|f| f == "zz") {
        return Ok(None);
    }
    files
        .iter()
        .map(|arg| repo::resolve_file_arg(repo, arg))
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

/// Collect file entries with git status, staged/unstaged hunks, and proper
/// initial selection. `filter` comes from `filter_paths`; `None` is every change.
pub(crate) fn collect_file_entries(
    repo: &Repository,
    workdir: &Path,
    filter: Option<&[String]>,
) -> Result<Vec<FileEntry>> {
    let changes = repo::get_working_changes_recurse(repo)?;

    let mut entries = Vec::new();
    let gitlinks = git::index_gitlinks(workdir)?;

    for change in &changes {
        if let Some(filter) = filter
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

        // A submodule is one object id, not text: it has no hunks to choose
        // between, so both halves of its state are one whole entry. Staging by
        // path is right here — the working tree is where this change comes from.
        if gitlinks.contains(&change.path) {
            if has_staged {
                hunks.push(submodule_entry(true, HunkOrigin::Staged));
            }
            if has_unstaged {
                hunks.push(submodule_entry(false, HunkOrigin::Unstaged));
            }
            is_binary = true;
        } else {
            if has_staged {
                is_binary |= collect_staged_hunks(workdir, &change.path, change.index, &mut hunks)?;
            }

            if has_unstaged {
                is_binary |=
                    collect_unstaged_hunks(workdir, &change.path, change.worktree, &mut hunks)?;
            }
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
                // Commit-origin hunks are read-only here; callers handle them.
                (HunkOrigin::Commit, _) => {}
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
                text: String::from(diff::DELETED_ENTRY),
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
                text: String::from(diff::BINARY_ENTRY),
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
                text: String::from(diff::DELETED_ENTRY),
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
                msg::warn(&format!("skipping unreadable file '{}'", path));
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
                    text: String::from(diff::BINARY_ENTRY),
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
                text: String::from(diff::BINARY_ENTRY),
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

/// Pick hunks from a specific commit, interactively or by id.
///
/// Shows the diff of `oid` vs its parent. All hunks start unselected (no-op).
/// Returns `Some(files)` with the selections on confirm, `None` on cancel.
/// With `picker.hunks` the selection is applied without rendering; in agent
/// mode without it, the hunks are listed as data instead (spec 019).
pub fn run_commit_hunk_picker(
    workdir: &Path,
    oid: &str,
    files: &[String],
    picker: &Picker,
    theme: &graph::Theme,
) -> Result<Option<Vec<FileEntry>>> {
    let mut entries = collect_commit_hunks(workdir, oid, files)?;

    if entries.is_empty() {
        // Only a rendered picker can be cancelled, so neither an agent nor
        // `--hunks` may be told `Cancelled` when there was nothing to pick.
        if agent_mode::enabled() || !picker.hunks.is_empty() {
            // `fold` never filters, so that clause would name an argument the
            // caller could not have passed.
            let filtered = if files.is_empty() {
                ""
            } else {
                ", or the given files matched none"
            };
            bail!(
                "No hunks to select in `{}`\nIts changes carry no text -p can pick{filtered}",
                git::short_hash(oid)
            );
        }
        msg::warn("No changes in commit");
        return Ok(None);
    }

    if !picker.hunks.is_empty() {
        hunk_select::apply(oid, &mut entries, picker)?;
        return Ok(Some(entries));
    }

    if agent_mode::enabled() {
        // A listing whose every id `apply` would refuse is a prompt with no
        // answer. The picker still renders these, so this stays agent-only.
        if !hunk_select::has_selectable(&entries, picker.whole_files) {
            bail!(
                "No hunks to select in `{}`\n\
                 It changes only binary files, which -p cannot move",
                git::short_hash(oid)
            );
        }
        return Err(hunk_select::respond(oid, entries, picker));
    }

    let tui_theme = TuiTheme::from_graph_theme(theme);
    crate::tui::hunk_selector::run_hunk_selector(entries, tui_theme)
}

/// Collect file entries from a commit's diff (`git diff <oid>^..<oid>`).
///
/// All hunks are created with `HunkOrigin::Commit` and `selected = false`.
pub(crate) fn collect_commit_hunks(
    workdir: &Path,
    oid: &str,
    files: &[String],
) -> Result<Vec<FileEntry>> {
    let changed_files = git::diff_commit_name_status(workdir, oid)?;
    let gitlinks = git::commit_gitlinks(workdir, oid)?;

    let mut entries = Vec::new();

    for (status, path) in &changed_files {
        if !files.is_empty() && !files.iter().any(|f| f == path) {
            continue;
        }

        let mut hunks = Vec::new();
        let mut is_binary = false;

        if *status == 'D' {
            hunks.push(HunkEntry {
                hunk: diff::DiffHunk {
                    text: String::from(diff::DELETED_ENTRY),
                    modified_lines: vec![],
                },
                selected: false,
                origin: HunkOrigin::Commit,
            });
        } else if gitlinks.contains_key(path) {
            hunks.push(submodule_entry(false, HunkOrigin::Commit));
            is_binary = true;
        } else if git::diff_commit_file_is_binary(workdir, oid, path)? {
            hunks.push(HunkEntry {
                hunk: diff::DiffHunk {
                    text: String::from(diff::BINARY_ENTRY),
                    modified_lines: vec![],
                },
                selected: false,
                origin: HunkOrigin::Commit,
            });
            is_binary = true;
        } else {
            let raw_diff = git::diff_commit_file(workdir, oid, path)?;
            for h in diff::parse_hunks(&raw_diff) {
                hunks.push(HunkEntry {
                    hunk: h,
                    selected: false,
                    origin: HunkOrigin::Commit,
                });
            }
        }

        if hunks.is_empty() {
            continue;
        }

        hunks.sort_by_key(|entry| hunk_sort_key(&entry.hunk));

        entries.push(FileEntry {
            path: path.clone(),
            hunks,
            index_status: *status,
            worktree_status: ' ',
            binary: is_binary,
        });
    }

    Ok(entries)
}

/// Save and unstage all currently staged changes, returning a patch to restore them later.
///
/// Returns an empty string if nothing is staged. Callers must call
/// `git::restore_staged_patch` with the returned patch when the operation
/// completes (or is rolled back), so pre-existing staged work is never lost.
pub(crate) fn save_and_unstage_staged(repo: &Repository, workdir: &Path) -> Result<String> {
    let staged = repo::get_staged_files(repo)?;
    if staged.is_empty() {
        return Ok(String::new());
    }
    let refs: Vec<&str> = staged.iter().map(|s| s.as_str()).collect();
    let patch = git::diff_cached_files(workdir, &refs)?;
    git::unstage_files(workdir, &refs)?;
    Ok(patch)
}

/// Save the staged diff for files that are staged but NOT in `target_files`,
/// then unstage them so they don't leak into the upcoming commit.
///
/// Returns the patch as a string (may be empty if nothing to save).
pub(crate) fn save_and_unstage_other_staged(
    repo: &Repository,
    workdir: &Path,
    target_files: &[&str],
) -> Result<String> {
    let staged = repo::get_staged_files(repo)?;
    let other: Vec<&str> = staged
        .iter()
        .filter(|f| !target_files.contains(&f.as_str()))
        .map(|s| s.as_str())
        .collect();
    if other.is_empty() {
        return Ok(String::new());
    }
    let patch = git::diff_cached_files(workdir, &other)?;
    git::unstage_files(workdir, &other)?;
    Ok(patch)
}

/// The single entry a submodule contributes to a picker: one object id, with no
/// hunks to choose between. `binary` is the flag every caller already reads as
/// "take or leave this one whole".
fn submodule_entry(selected: bool, origin: HunkOrigin) -> HunkEntry {
    HunkEntry {
        hunk: diff::DiffHunk {
            text: String::from(diff::SUBMODULE_ENTRY),
            modified_lines: vec![],
        },
        selected,
        origin,
    }
}

fn hunk_sort_key(hunk: &diff::DiffHunk) -> usize {
    let first_line = hunk.text.lines().next().unwrap_or("");
    parse_hunk_start(first_line).unwrap_or(0)
}

#[cfg(test)]
#[path = "staging_test.rs"]
mod tests;
