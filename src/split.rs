use anyhow::{Result, bail};
use git2::{Oid, Repository};

use crate::git::{self, Target};
use crate::git_commands::{self, git_commit, git_rebase};
use crate::msg;
use crate::weave;

/// Split a commit into two sequential commits.
///
/// Dispatches based on the resolved target type:
/// - Commit → split the commit by selecting files for the first commit
pub fn run(target: String, message: Option<String>) -> Result<()> {
    let repo = git::open_repo()?;

    let resolved = git::resolve_target(&repo, &target)?;

    match resolved {
        Target::Commit(hash) => split_commit(&repo, &hash, message),
        Target::Branch(_) => bail!("Cannot split a branch"),
        Target::File(_) => bail!("Cannot split a file"),
        Target::Unstaged => bail!("Cannot split unstaged changes"),
        Target::CommitFile { .. } => bail!("Cannot split a commit file"),
    }
}

/// Split a commit by showing an interactive file picker.
fn split_commit(repo: &Repository, commit_hash: &str, message: Option<String>) -> Result<()> {
    let workdir = git::require_workdir(repo, "split")?;
    let commit_oid = repo.revparse_single(commit_hash)?.peel_to_commit()?.id();
    let commit = repo.find_commit(commit_oid)?;

    // Reject merge commits
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

    // Get the first commit message
    let msg1 = match message {
        Some(m) => m,
        None => msg::input("Message for the first commit", |s| {
            if s.trim().is_empty() {
                Err("Message cannot be empty")
            } else {
                Ok(())
            }
        })?,
    };

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
        &msg1,
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

    // Reject merge commits
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
        &message,
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
    msg1: &str,
    msg2: &str,
) -> Result<()> {
    let head_oid = git::head_oid(repo)?;
    let is_head = head_oid == commit_oid;
    let oid_str = commit_oid.to_string();
    let short_hash = git_commands::short_hash(&oid_str);

    if is_head {
        perform_head_split(workdir, selected, remaining, msg1, msg2)?;
    } else {
        perform_non_head_split(repo, workdir, commit_oid, selected, remaining, msg1, msg2)?;
    }

    msg::success(&format!("Split `{}` into 2 commits", short_hash));
    Ok(())
}

/// Split HEAD commit (no rebase needed).
///
/// ```text
/// reset_mixed(HEAD~1) → stage selected → commit(msg1) → stage remaining → commit(msg2)
/// ```
fn perform_head_split(
    workdir: &std::path::Path,
    selected: &[String],
    remaining: &[String],
    msg1: &str,
    msg2: &str,
) -> Result<()> {
    git_commit::reset_mixed(workdir, "HEAD~1")?;

    let selected_refs: Vec<&str> = selected.iter().map(|s| s.as_str()).collect();
    git_commit::stage_files(workdir, &selected_refs)?;
    git_commit::commit(workdir, msg1)?;

    let remaining_refs: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();
    git_commit::stage_files(workdir, &remaining_refs)?;
    git_commit::commit(workdir, msg2)?;

    Ok(())
}

/// Split a non-HEAD commit using edit-and-continue rebase.
///
/// ```text
/// Weave::from_repo → edit_commit(oid) → run_rebase (pauses)
/// → reset_mixed(HEAD~1) → stage selected → commit(msg1)
/// → stage remaining → commit(msg2)
/// → continue_rebase
/// ```
fn perform_non_head_split(
    repo: &Repository,
    workdir: &std::path::Path,
    commit_oid: Oid,
    selected: &[String],
    remaining: &[String],
    msg1: &str,
    msg2: &str,
) -> Result<()> {
    // Start edit rebase
    weave::start_edit_rebase(repo, workdir, commit_oid)?;

    // Now paused at the target commit — split it
    if let Err(e) = do_split_at_pause(workdir, selected, remaining, msg1, msg2) {
        let _ = git_rebase::abort(workdir);
        return Err(e);
    }

    // Continue the rebase
    git_rebase::continue_rebase(workdir)?;

    Ok(())
}

/// Perform the actual split while paused during a rebase.
fn do_split_at_pause(
    workdir: &std::path::Path,
    selected: &[String],
    remaining: &[String],
    msg1: &str,
    msg2: &str,
) -> Result<()> {
    git_commit::reset_mixed(workdir, "HEAD~1")?;

    let selected_refs: Vec<&str> = selected.iter().map(|s| s.as_str()).collect();
    git_commit::stage_files(workdir, &selected_refs)?;
    git_commit::commit(workdir, msg1)?;

    let remaining_refs: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();
    git_commit::stage_files(workdir, &remaining_refs)?;
    git_commit::commit(workdir, msg2)?;

    Ok(())
}

#[cfg(test)]
#[path = "split_test.rs"]
mod tests;
