use anyhow::{Context, Result, bail};
use git2::Repository;
use std::cell::Cell;
use std::io::Write;
use std::path::Path;

use crate::core::diff::{self, parse_hunk_start};
use crate::core::hunk_select::{self, Picker};
use crate::core::repo;
use crate::core::{agent_mode, graph, msg};
use crate::git;
use crate::tui::hunk_selector::{FileEntry, HunkEntry, HunkOrigin};
use crate::tui::theme::TuiTheme;

/// What becomes of staged work a pick leaves out (Spec 019).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeftOut {
    /// `add -p`: unstaging is what an untick means.
    Unstaged,
    /// `commit -p`, `fold -p`: out of what they create, but still staged after.
    KeptStaged,
}

/// What a picker did to the index (Spec 006).
pub(crate) struct Picked<'a> {
    /// The paths it staged: those with a selected hunk.
    pub paths: Vec<String>,
    /// The staged work it unstaged because the pick left it out, put back when
    /// this drops. Empty for `LeftOut::Unstaged`.
    pub left_out: StagedAside<'a>,
}

/// Pick working-tree hunks for the given files (or all if empty / `zz`).
///
/// Returns what the picker staged, or `None` if it was cancelled. Never a path
/// the picker did not show (Spec 007).
/// With `picker.hunks` the selection is applied without rendering; in agent
/// mode without it, the hunks are listed as data instead (spec 019).
pub fn run_hunk_picker<'a>(
    repo: &Repository,
    workdir: &'a Path,
    filter: Option<&[String]>,
    picker: &Picker,
    left_out: LeftOut,
    theme: &graph::Theme,
) -> Result<Option<Picked<'a>>> {
    let mut entries = collect_file_entries(repo, workdir, filter)?;

    if entries.is_empty() {
        // Only a rendered picker can be cancelled (see `run_commit_hunk_picker`).
        if agent_mode::enabled() || !picker.hunks.is_empty() {
            bail!("No changes to stage");
        }
        msg::warn("No changes to stage");
        return Ok(None);
    }

    let source = binary_source(workdir, &entries);
    if !picker.hunks.is_empty() {
        hunk_select::apply(&source, &mut entries, picker)?;
        return stage_selection(workdir, &entries, left_out).map(Some);
    }

    if agent_mode::enabled() {
        return Err(hunk_select::respond(&source, entries, picker));
    }

    let tui_theme = TuiTheme::from_graph_theme(theme);
    let result = crate::tui::hunk_selector::run_hunk_selector(entries, tui_theme)?;

    match result {
        None => Ok(None),
        Some(selected_files) => stage_selection(workdir, &selected_files, left_out).map(Some),
    }
}

/// Stage what a picker kept.
pub(crate) fn stage_selection<'a>(
    workdir: &'a Path,
    files: &[FileEntry],
    left_out: LeftOut,
) -> Result<Picked<'a>> {
    let patch = apply_selections(workdir, files, left_out)?;
    Ok(Picked {
        paths: selected_paths(files),
        left_out: match left_out {
            LeftOut::Unstaged => StagedAside::none(workdir),
            LeftOut::KeptStaged => StagedAside::new(workdir, patch),
        },
    })
}

/// The content `git add` stages for the binary entries of `entries`, which
/// their listing shows only as a label: a file's blob id, a submodule's HEAD.
pub(crate) fn binary_stamp(workdir: &Path, entries: &[FileEntry]) -> Vec<Option<git2::Oid>> {
    entries
        .iter()
        .filter(|f| f.binary)
        .map(|f| {
            let path = workdir.join(&f.path);
            if path.is_dir() {
                Repository::open(&path).ok()?.head().ok()?.target()
            } else {
                git2::Oid::hash_file(git2::ObjectType::Blob, &path).ok()
            }
        })
        .collect()
}

/// What a working-tree listing is fingerprinted against in place of a commit:
/// the `binary_stamp` of its entries, which it lists only as a label, so a
/// replay after one changed on disk is refused rather than stage it (Spec 019).
fn binary_source(workdir: &Path, entries: &[FileEntry]) -> String {
    binary_stamp(workdir, entries)
        .iter()
        .map(|oid| oid.map_or_else(|| "-".to_string(), |oid| oid.to_string()))
        .collect::<Vec<_>>()
        .join(",")
}

/// Whether `picked`, read before `stamp` was taken of it, still lists the
/// changes of its paths, whatever it selected: a pick staged later must not
/// take what was edited since.
pub(crate) fn still_listed(
    repo: &Repository,
    workdir: &Path,
    picked: &[FileEntry],
    stamp: &[Option<git2::Oid>],
) -> Result<bool> {
    let paths: Vec<String> = picked.iter().map(|f| f.path.clone()).collect();
    let now = collect_file_entries(repo, workdir, Some(&paths))?;
    let same = |a: &FileEntry, b: &FileEntry| {
        a.path == b.path
            && a.index_status == b.index_status
            && a.worktree_status == b.worktree_status
            && a.binary == b.binary
            && a.hunks.len() == b.hunks.len()
            && a.hunks
                .iter()
                .zip(&b.hunks)
                .all(|(x, y)| x.hunk == y.hunk && x.origin == y.origin)
    };
    Ok(picked.len() == now.len()
        && picked.iter().zip(&now).all(|(a, b)| same(a, b))
        && binary_stamp(workdir, &now) == stamp)
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
///
/// All or nothing: a step that fails puts the index back as it was, since the
/// ones before it may already have unstaged the user's work.
///
/// Returns the staged work it unstaged, as a patch that stages it again.
pub(crate) fn apply_selections(
    workdir: &Path,
    files: &[FileEntry],
    left_out: LeftOut,
) -> Result<String> {
    // Kept staged, the left-out work travels as blobs in the returned patch,
    // so only an unstaging for good can lose it.
    if left_out == LeftOut::Unstaged {
        hunk_select::refuse_losing_index_content(files)?;
    }

    let index = git::git_path(workdir, "index")?;
    let before = match std::fs::read(&index) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        // Without a copy there is nothing to restore: `None` would delete it.
        Err(e) => return Err(e).context(format!("Failed to read '{}'", index.display())),
    };
    let result = apply_selections_unguarded(workdir, files, left_out);
    if result.is_err() {
        restore_index(&index, before.as_deref());
    }
    result
}

/// Put the index file back. Best-effort — the caller is already reporting a
/// failure — but a miss is said, since the index then is not as it was.
fn restore_index(index: &Path, before: Option<&[u8]>) {
    if !put_index_back(index, before) {
        msg::warn("the index could not be put back as it was: check `git status`");
    }
}

/// Write `before` as the index through `index.lock`, git's own protocol: the
/// lock keeps another writer out, and the rename lands it whole. `None` means
/// there was no index, so it is removed under the lock instead.
fn put_index_back(index: &Path, before: Option<&[u8]>) -> bool {
    let mut lock_name = index.as_os_str().to_owned();
    lock_name.push(".lock");
    let lock = std::path::PathBuf::from(lock_name);
    // Held by someone else: theirs to finish, not ours to remove.
    let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
    else {
        return false;
    };
    let done = match before {
        Some(bytes) => {
            let written = file.write_all(bytes).and_then(|()| file.sync_all()).is_ok();
            drop(file);
            written && std::fs::rename(&lock, index).is_ok()
        }
        None => {
            drop(file);
            match std::fs::remove_file(index) {
                Ok(()) => true,
                Err(e) => e.kind() == std::io::ErrorKind::NotFound,
            }
        }
    };
    if !done || before.is_none() {
        let _ = std::fs::remove_file(&lock);
    }
    done
}

fn apply_selections_unguarded(
    workdir: &Path,
    files: &[FileEntry],
    left_out: LeftOut,
) -> Result<String> {
    let mut to_stage_patch = String::new();
    // Stage hunks of a file that is also being unstaged, in part or whole:
    // diffed against the index before the unstaging, so they may no longer
    // apply there.
    // Never fuzzed: a looser match can land them in the wrong place.
    let mut to_stage_after_unstage = String::new();
    let mut paths_after_unstage: Vec<&str> = Vec::new();
    let mut to_unstage_patch = String::new();
    let mut files_to_add: Vec<&str> = Vec::new();
    let mut files_to_unstage: Vec<&str> = Vec::new();
    let mut total_staged = 0usize;
    let mut total_unstaged = 0usize;
    let mut files_changed = 0usize;

    for file in files {
        let is_untracked = file.is_untracked();
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
            let patch = diff::build_hunk_patch(&file.path, &hunks_to_stage);
            if hunks_to_unstage.is_empty() && !files_to_unstage.contains(&&*file.path) {
                to_stage_patch.push_str(&patch);
            } else {
                to_stage_after_unstage.push_str(&patch);
                paths_after_unstage.push(&file.path);
            }
            total_staged += hunks_to_stage.len();
        }

        if file_had_change {
            files_changed += 1;
        }
    }

    // The index before and after the unstaging differ by exactly what was
    // left out. Trees rather than the hunks: the diff then carries blob ids,
    // so putting it back can go three-way, binary files included.
    // Only when it is kept: `write-tree` refuses an index with unmerged paths,
    // which need not stop an `add -p` elsewhere.
    let unstaging = left_out == LeftOut::KeptStaged
        && (!to_unstage_patch.is_empty() || !files_to_unstage.is_empty());
    let before_unstage = if unstaging {
        Some(git::write_tree(workdir)?)
    } else {
        None
    };

    // Apply unstaging first (reverse-apply staged hunks that were deselected).
    if !to_unstage_patch.is_empty() {
        git::apply_cached_patch_reverse(workdir, &to_unstage_patch)?;
    }

    if !files_to_unstage.is_empty() {
        git::unstage_files(workdir, &files_to_unstage)?;
    }

    let left_out = match &before_unstage {
        Some(before) => git::diff_trees(workdir, &git::write_tree(workdir)?, before)?,
        None => String::new(),
    };

    // Apply staging (apply selected unstaged hunks to the index).
    if !to_stage_patch.is_empty() {
        git::apply_cached_patch(workdir, &to_stage_patch)?;
    }

    if !to_stage_after_unstage.is_empty() {
        let paths = paths_after_unstage
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join(", ");
        git::apply_cached_patch(workdir, &to_stage_after_unstage).with_context(|| {
            format!(
                "The picked hunks of {paths} were diffed against staged content being \
                 unstaged, and no longer apply\n\
                 Keep the staged hunks of that file in the pick, or commit them first"
            )
        })?;
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
    Ok(left_out)
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

    let parsed = diff::parse_hunks(&raw_diff);
    // A staged empty file has no `@@` hunk. Without an entry the picker never
    // offers it, and `commit -p` sets aside what it did not offer.
    if parsed.is_empty() && index_status == 'A' {
        hunks.push(HunkEntry {
            hunk: diff::DiffHunk {
                text: String::from(diff::EMPTY_ENTRY),
                modified_lines: vec![],
            },
            selected: true,
            origin: HunkOrigin::Staged,
        });
        return Ok(false);
    }
    for h in parsed {
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
                    text: String::from(diff::EMPTY_ENTRY),
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
        let mut text = format!("{}+1,{} @@\n", diff::NEW_FILE_HEADER, line_count);
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

/// Staged work an operation set aside, put back into the index when this
/// drops — every `?` in between included, which is what keeps it (Spec 006).
///
/// Hand it on where someone else owns the restore: a state file, or a worktree
/// snapshot taken before the unstaging. That owner has to be one this guard
/// cannot outlive uncollected — see [`StagedAside::handed_over`].
#[must_use = "dropping the guard right away puts the work straight back, undoing the unstage"]
pub(crate) struct StagedAside<'a> {
    workdir: &'a Path,
    patch: String,
    armed: Cell<bool>,
}

impl<'a> StagedAside<'a> {
    fn new(workdir: &'a Path, patch: String) -> Self {
        StagedAside {
            workdir,
            patch,
            armed: Cell::new(true),
        }
    }

    /// A guard over nothing: the operation set no staged work aside.
    pub(crate) fn none(workdir: &'a Path) -> Self {
        StagedAside::new(workdir, String::new())
    }

    pub(crate) fn patch(&self) -> &str {
        &self.patch
    }

    /// Put it back now, rather than wherever this would have dropped. Does
    /// nothing once [`StagedAside::handed_over`] has run.
    pub(crate) fn restore(self) {}

    /// Guard `other`'s patch too, put back along with this one's.
    pub(crate) fn absorb(mut self, other: StagedAside<'a>) -> Self {
        let extra = other.release();
        self.patch.push_str(&extra);
        self
    }

    /// Take the patch back: the caller restores it from here.
    #[must_use = "the set-aside patch is lost unless a new owner keeps it"]
    pub(crate) fn release(mut self) -> String {
        self.armed.set(false);
        std::mem::take(&mut self.patch)
    }

    /// Something else holds the patch now, so this must not put it back a
    /// second time.
    ///
    /// Data safety: that owner must be durable (a state file `loom abort`
    /// reads) or already have run. A rollback closure is neither until it
    /// runs, and [`git::rebase_abort_then_cleanup`] skips its closure when the
    /// abort fails — so call this from inside such a closure, never before it.
    /// Disarming early there drops the patch on the one path that cannot get
    /// it back; leaving the guard armed parks it instead.
    pub(crate) fn handed_over(&self) {
        self.armed.set(false);
    }
}

impl Drop for StagedAside<'_> {
    fn drop(&mut self) {
        if self.armed.get() {
            git::restore_loom_unstaged(self.workdir, &self.patch);
        }
    }
}

/// Save and unstage all currently staged changes, so an amend or rebase below
/// leaves them out. Restored when the returned guard drops.
pub(crate) fn save_and_unstage_staged<'a>(
    repo: &Repository,
    workdir: &'a Path,
) -> Result<StagedAside<'a>> {
    let staged = repo::get_staged_files(repo)?;
    if staged.is_empty() {
        return Ok(StagedAside::new(workdir, String::new()));
    }
    let refs: Vec<&str> = staged.iter().map(|s| s.as_str()).collect();
    let patch = git::diff_cached_files(workdir, &refs)?;
    git::unstage_files(workdir, &refs)?;
    Ok(StagedAside::new(workdir, patch))
}

/// Save the staged diff for files that are staged but NOT in `target_files`,
/// then unstage them so they don't leak into the upcoming commit. Restored
/// when the returned guard drops.
pub(crate) fn save_and_unstage_other_staged<'a>(
    repo: &Repository,
    workdir: &'a Path,
    target_files: &[&str],
) -> Result<StagedAside<'a>> {
    let staged = repo::get_staged_files(repo)?;
    let other: Vec<&str> = staged
        .iter()
        .filter(|f| !target_files.contains(&f.as_str()))
        .map(|s| s.as_str())
        .collect();
    if other.is_empty() {
        return Ok(StagedAside::new(workdir, String::new()));
    }
    let patch = git::diff_cached_files(workdir, &other)?;
    git::unstage_files(workdir, &other)?;
    Ok(StagedAside::new(workdir, patch))
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
