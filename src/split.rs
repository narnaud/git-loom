use anyhow::{Result, bail};
use git2::{Oid, Repository};

use crate::git::{self, Target, TargetKind};
use crate::git_commands::{self, git_commit, git_rebase};
use crate::msg;
use crate::weave;

/// Commit with `-m` message or open the editor.
fn commit_or_editor(workdir: &std::path::Path, message: Option<&str>) -> Result<()> {
    match message {
        Some(m) => git_commit::commit(workdir, m),
        None => git_commit::commit_with_editor(workdir),
    }
}

/// Split a commit into two sequential commits.
///
/// Dispatches based on the resolved target type:
/// - Commit → split the commit by selecting files for the first commit
pub fn run(target: String, message: Option<String>) -> Result<()> {
    let repo = git::open_repo()?;

    let resolved = git::resolve_arg(&repo, &target, &[TargetKind::Commit])?;

    match resolved {
        Target::Commit(hash) => split_commit(&repo, &hash, message),
        _ => unreachable!(),
    }
}

/// Split a commit by showing an interactive file picker.
fn split_commit(repo: &Repository, commit_hash: &str, message: Option<String>) -> Result<()> {
    let workdir = git::require_workdir(repo, "split")?;
    let commit_oid = repo.revparse_single(commit_hash)?.peel_to_commit()?.id();
    let commit = repo.find_commit(commit_oid)?;

    if commit.parent_count() > 1 {
        bail!("Cannot split a merge commit");
    }

    // Get files changed in the commit
    let files = git::commit_file_paths(repo, commit_oid)?;
    if files.len() < 2 {
        bail!("Cannot split a commit with only one file");
    }

    // Show interactive file picker
    let selected = pick_files(&files)?;

    let original_msg = commit.message().unwrap_or("").trim().to_string();

    // Compute remaining files
    let remaining: Vec<String> = files
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
/// This is the testable core that bypasses the interactive file picker.
#[cfg(test)]
pub fn split_commit_with_selection(
    repo: &Repository,
    commit_hash: &str,
    selected: Vec<String>,
    message: String,
) -> Result<()> {
    let workdir = git::require_workdir(repo, "split")?;
    let commit_oid = repo.revparse_single(commit_hash)?.peel_to_commit()?.id();
    let commit = repo.find_commit(commit_oid)?;

    if commit.parent_count() > 1 {
        bail!("Cannot split a merge commit");
    }

    // Get files changed in the commit
    let all_files = git::commit_file_paths(repo, commit_oid)?;
    if all_files.len() < 2 {
        bail!("Cannot split a commit with only one file");
    }

    if selected.is_empty() {
        bail!("Must select at least one file for the first commit");
    }

    let remaining: Vec<String> = all_files
        .into_iter()
        .filter(|f| !selected.contains(f))
        .collect();

    if remaining.is_empty() {
        bail!("Must leave at least one file for the second commit");
    }

    let original_msg = commit.message().unwrap_or("").trim().to_string();

    perform_split(
        repo,
        workdir,
        commit_oid,
        &selected,
        &remaining,
        Some(message.as_str()),
        &original_msg,
    )
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
    let head_oid = git::head_oid(repo)?;
    let is_head = head_oid == commit_oid;
    let oid_str = commit_oid.to_string();
    let short_hash = git_commands::short_hash(&oid_str);

    // Save pre-existing staged changes so reset --mixed doesn't discard them.
    // (For non-HEAD splits, the rebase autostash handles this, but saving
    // here is harmless — it will be empty.)
    let saved_staged = save_staged(repo, workdir)?;

    let split_result = if is_head {
        perform_head_split(workdir, selected, remaining, msg1, msg2)
    } else {
        perform_non_head_split(repo, workdir, commit_oid, selected, remaining, msg1, msg2)
    };

    // Restore pre-existing staged changes regardless of outcome.
    git_commands::restore_staged_patch(workdir, &saved_staged)?;

    let (new_hash1, new_hash2) = split_result?;

    msg::success(&format!(
        "Split `{}` into `{}` and `{}`",
        short_hash,
        git_commands::short_hash(&new_hash1),
        git_commands::short_hash(&new_hash2)
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
    git_commit::reset_mixed(workdir, "HEAD~1")?;

    let selected_refs: Vec<&str> = selected.iter().map(|s| s.as_str()).collect();
    git_commit::stage_files(workdir, &selected_refs)?;
    commit_or_editor(workdir, msg1)?;

    let remaining_refs: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();
    git_commit::stage_files(workdir, &remaining_refs)?;
    git_commit::commit(workdir, msg2)?;

    // HEAD is the second commit, HEAD~1 is the first
    let hash2 = git_commands::rev_parse(workdir, "HEAD")?;
    let hash1 = git_commands::rev_parse(workdir, "HEAD~1")?;

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
            let _ = git_rebase::abort(workdir);
            return Err(e);
        }
    };

    // Continue the rebase — later commits are replayed on top of the split
    // commits, so hash1 and hash2 remain valid (they are ancestors).
    // Abort automatically on conflict — split does not save LoomState.
    git_rebase::continue_rebase_or_abort(workdir)?;

    Ok((hash1, hash2))
}

/// Save all staged changes aside so they can be restored after the split.
fn save_staged(repo: &Repository, workdir: &std::path::Path) -> Result<String> {
    let staged = git::get_staged_files(repo)?;
    if staged.is_empty() {
        return Ok(String::new());
    }
    let refs: Vec<&str> = staged.iter().map(|s| s.as_str()).collect();
    let patch = git_commands::diff_cached_files(workdir, &refs)?;
    git_commands::unstage_files(workdir, &refs)?;
    Ok(patch)
}

#[cfg(test)]
#[path = "split_test.rs"]
mod tests;
