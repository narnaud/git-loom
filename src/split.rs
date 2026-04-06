use anyhow::{Result, bail};
use git2::{Oid, Repository};

use crate::core::repo::{self, Target, TargetKind};
use crate::core::weave;
use crate::core::{diff, graph, msg, staging};
use crate::git;
use crate::tui::hunk_selector::FileEntry;

/// Commit with `-m` message or open the editor.
fn commit_or_editor(workdir: &std::path::Path, message: Option<&str>) -> Result<()> {
    match message {
        Some(m) => git::commit(workdir, m),
        None => git::commit_with_editor(workdir),
    }
}

/// Split a commit into two sequential commits.
///
/// Dispatches based on the resolved target type:
/// - Commit → split the commit by selecting files (or hunks with `-p`) for the first commit
pub fn run(
    target: String,
    message: Option<String>,
    patch: bool,
    files: Vec<String>,
    theme: &graph::Theme,
) -> Result<()> {
    let repo = repo::open_repo()?;

    let resolved = repo::resolve_arg(&repo, &target, &[TargetKind::Commit])?;

    match resolved {
        Target::Commit(hash) => split_commit(&repo, &hash, message, patch, files, theme),
        _ => unreachable!(),
    }
}

/// Split a commit, using provided files or an interactive picker if none are given.
fn split_commit(
    repo: &Repository,
    commit_hash: &str,
    message: Option<String>,
    patch: bool,
    files: Vec<String>,
    theme: &graph::Theme,
) -> Result<()> {
    let workdir = repo::require_workdir(repo, "split")?;
    let commit_oid = repo.revparse_single(commit_hash)?.peel_to_commit()?.id();
    let commit = repo.find_commit(commit_oid)?;

    if commit.parent_count() > 1 {
        bail!("Cannot split a merge commit");
    }

    let original_msg = commit.message().unwrap_or("").trim().to_string();

    if patch {
        let oid_str = commit_oid.to_string();
        let selections = staging::run_commit_hunk_picker(workdir, &oid_str, &files, theme)?
            .ok_or_else(|| anyhow::anyhow!("Cancelled"))?;

        let has_selected = selections
            .iter()
            .any(|f| f.hunks.iter().any(|h| h.selected));
        let has_unselected = selections
            .iter()
            .any(|f| f.hunks.iter().any(|h| !h.selected));
        if !has_selected {
            bail!("Must select at least one hunk for the first commit");
        }
        if !has_unselected {
            bail!("Must leave at least one hunk for the second commit");
        }

        return perform_split_by_hunks(
            repo,
            workdir,
            commit_oid,
            &selections,
            message.as_deref(),
            &original_msg,
        );
    }

    // File-based split
    let all_files = repo::commit_file_paths(repo, commit_oid)?;
    if all_files.len() < 2 {
        bail!("Cannot split a commit with only one file");
    }

    let selected = if files.is_empty() {
        pick_files(&all_files)?
    } else {
        let remaining_count = all_files.iter().filter(|f| !files.contains(f)).count();
        if remaining_count == 0 {
            bail!("Must leave at least one file for the second commit");
        }
        files
    };

    let remaining: Vec<String> = all_files
        .into_iter()
        .filter(|f| !selected.contains(f))
        .collect();

    perform_split(
        repo,
        workdir,
        commit_oid,
        &selected,
        &remaining,
        message.as_deref(),
        &original_msg,
    )
}

/// Split a commit with pre-selected files (no interactive picker).
///
/// Bypasses the interactive file picker — useful for tests.
#[cfg(test)]
pub fn split_commit_with_selection(
    repo: &Repository,
    commit_hash: &str,
    selected: Vec<String>,
    message: String,
) -> Result<()> {
    let theme = graph::Theme::dark();
    split_commit(repo, commit_hash, Some(message), false, selected, &theme)
}

/// Show an interactive file picker for splitting.
fn pick_files(files: &[String]) -> Result<Vec<String>> {
    let selected = inquire::MultiSelect::new("Select files for the first commit:", files.to_vec())
        .with_validator(|selection: &[inquire::list_option::ListOption<&String>]| {
            if selection.is_empty() {
                return Ok(inquire::validator::Validation::Invalid(
                    "Must select at least one file".into(),
                ));
            }
            Ok(inquire::validator::Validation::Valid)
        })
        .prompt()?;

    if selected.len() == files.len() {
        bail!("Must leave at least one file for the second commit");
    }

    Ok(selected)
}

/// Perform the actual split operation.
fn perform_split(
    repo: &Repository,
    workdir: &std::path::Path,
    commit_oid: Oid,
    selected: &[String],
    remaining: &[String],
    msg1: Option<&str>,
    msg2: &str,
) -> Result<()> {
    let head_oid = repo::head_oid(repo)?;
    let is_head = head_oid == commit_oid;
    let oid_str = commit_oid.to_string();
    let short_hash = git::short_hash(&oid_str);

    // Save pre-existing staged changes so reset --mixed doesn't discard them.
    // (For non-HEAD splits, the rebase autostash handles this, but saving
    // here is harmless — it will be empty.)
    let saved_staged = staging::save_and_unstage_staged(repo, workdir)?;

    let split_result = if is_head {
        perform_head_split(workdir, selected, remaining, msg1, msg2)
    } else {
        perform_non_head_split(repo, workdir, commit_oid, selected, remaining, msg1, msg2)
    };

    // Restore pre-existing staged changes regardless of outcome.
    git::restore_staged_patch(workdir, &saved_staged)?;

    let (new_hash1, new_hash2) = split_result?;

    msg::success(&format!(
        "Split `{}` into `{}` and `{}`",
        short_hash,
        git::short_hash(&new_hash1),
        git::short_hash(&new_hash2)
    ));
    Ok(())
}

/// Split HEAD commit (no rebase needed).
///
/// ```text
/// reset_mixed(HEAD~1) → stage selected → commit(msg1) → stage remaining → commit(msg2)
/// ```
///
/// Returns `(hash1, hash2)` — the two new commit hashes.
fn perform_head_split(
    workdir: &std::path::Path,
    selected: &[String],
    remaining: &[String],
    msg1: Option<&str>,
    msg2: &str,
) -> Result<(String, String)> {
    git::reset_mixed(workdir, "HEAD~1")?;

    let selected_refs: Vec<&str> = selected.iter().map(|s| s.as_str()).collect();
    git::stage_files(workdir, &selected_refs)?;
    commit_or_editor(workdir, msg1)?;

    let remaining_refs: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();
    git::stage_files(workdir, &remaining_refs)?;
    git::commit(workdir, msg2)?;

    // HEAD is the second commit, HEAD~1 is the first
    let hash2 = git::rev_parse(workdir, "HEAD")?;
    let hash1 = git::rev_parse(workdir, "HEAD~1")?;

    Ok((hash1, hash2))
}

/// Split a non-HEAD commit using edit-and-continue rebase.
///
/// ```text
/// Weave::from_repo → edit_commit(oid) → run_rebase (pauses)
/// → reset_mixed(HEAD~1) → stage selected → commit(msg1)
/// → stage remaining → commit(msg2)
/// → continue_rebase
/// ```
///
/// Returns `(hash1, hash2)` — the two new commit hashes.
fn perform_non_head_split(
    repo: &Repository,
    workdir: &std::path::Path,
    commit_oid: Oid,
    selected: &[String],
    remaining: &[String],
    msg1: Option<&str>,
    msg2: &str,
) -> Result<(String, String)> {
    // Start edit rebase
    weave::start_edit_rebase(repo, workdir, commit_oid)?;

    // Now paused at the target commit — split it (same as HEAD split since
    // the rebase has made the target commit HEAD).
    let (hash1, hash2) = match perform_head_split(workdir, selected, remaining, msg1, msg2) {
        Ok(hashes) => hashes,
        Err(e) => {
            let _ = git::rebase_abort(workdir);
            return Err(e);
        }
    };

    // Continue the rebase — later commits are replayed on top of the split
    // commits, so hash1 and hash2 remain valid (they are ancestors).
    // Abort automatically on conflict — split does not save LoomState.
    git::continue_rebase_or_abort(workdir)?;

    Ok((hash1, hash2))
}

/// Perform the hunk-based split operation.
fn perform_split_by_hunks(
    repo: &Repository,
    workdir: &std::path::Path,
    commit_oid: Oid,
    selections: &[FileEntry],
    msg1: Option<&str>,
    msg2: &str,
) -> Result<()> {
    let head_oid = repo::head_oid(repo)?;
    let is_head = head_oid == commit_oid;
    let oid_str = commit_oid.to_string();
    let short_hash = git::short_hash(&oid_str);

    let saved_staged = staging::save_and_unstage_staged(repo, workdir)?;

    let split_result = if is_head {
        perform_head_split_by_hunks(workdir, selections, msg1, msg2)
    } else {
        perform_non_head_split_by_hunks(repo, workdir, commit_oid, selections, msg1, msg2)
    };

    git::restore_staged_patch(workdir, &saved_staged)?;

    let (new_hash1, new_hash2) = split_result?;

    msg::success(&format!(
        "Split `{}` into `{}` and `{}`",
        short_hash,
        git::short_hash(&new_hash1),
        git::short_hash(&new_hash2)
    ));
    Ok(())
}

/// HEAD hunk-based split.
///
/// ```text
/// reset_mixed(HEAD~1) → apply selected hunks → commit(msg1) → stage remaining → commit(msg2)
/// ```
fn perform_head_split_by_hunks(
    workdir: &std::path::Path,
    selections: &[FileEntry],
    msg1: Option<&str>,
    msg2: &str,
) -> Result<(String, String)> {
    git::reset_mixed(workdir, "HEAD~1")?;

    // Stage selected hunks for the first commit.
    let mut selected_patch = String::new();
    for file in selections {
        let selected: Vec<_> = file
            .hunks
            .iter()
            .filter(|h| h.selected)
            .map(|h| &h.hunk)
            .collect();
        if selected.is_empty() {
            continue;
        }
        if file.binary || file.index_status == 'D' {
            git::stage_path(workdir, &file.path)?;
        } else {
            selected_patch.push_str(&diff::build_hunk_patch(&file.path, &selected));
        }
    }
    if !selected_patch.is_empty() {
        git::apply_cached_patch(workdir, &selected_patch)?;
    }

    commit_or_editor(workdir, msg1)?;

    // Stage remaining changes for the second commit.
    for file in selections {
        if file.hunks.iter().any(|h| !h.selected) {
            git::stage_path(workdir, &file.path)?;
        }
    }
    git::commit(workdir, msg2)?;

    let hash2 = git::rev_parse(workdir, "HEAD")?;
    let hash1 = git::rev_parse(workdir, "HEAD~1")?;
    Ok((hash1, hash2))
}

/// Non-HEAD hunk-based split using edit-and-continue rebase.
fn perform_non_head_split_by_hunks(
    repo: &Repository,
    workdir: &std::path::Path,
    commit_oid: Oid,
    selections: &[FileEntry],
    msg1: Option<&str>,
    msg2: &str,
) -> Result<(String, String)> {
    weave::start_edit_rebase(repo, workdir, commit_oid)?;

    let (hash1, hash2) = match perform_head_split_by_hunks(workdir, selections, msg1, msg2) {
        Ok(hashes) => hashes,
        Err(e) => {
            let _ = git::rebase_abort(workdir);
            return Err(e);
        }
    };

    git::continue_rebase_or_abort(workdir)?;
    Ok((hash1, hash2))
}

#[cfg(test)]
#[path = "split_test.rs"]
mod tests;
