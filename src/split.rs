use anyhow::{Result, bail};
use git2::{Oid, Repository};

use crate::core::hunk_select::{self, HunkArgs, Picker};
use crate::core::repo::{self, Target, TargetKind};
use crate::core::weave;
use crate::core::{agent_mode, changeid, diff, graph, msg, staging};
use crate::git;
use crate::tui::hunk_selector::FileEntry;

const COMMAND: &str = "split";

/// The invocation to repeat, carrying every argument that shapes the split, so
/// an agent can replay it with only the missing piece filled in (spec 019).
/// Dropping the `<files>` filter would list a different set of hunks and fail
/// the fingerprint check instead of splitting.
fn invocation(
    target: &str,
    message: Option<&str>,
    patch: bool,
    hunks: &HunkArgs,
    files: &[String],
) -> String {
    // The placeholder belongs only to the prompt asking for the message; every
    // other hint repeats the one the caller already gave, or replaying it
    // commits a subject of `<message>`.
    let message = message.map_or_else(|| "<message>".to_string(), hunk_select::quoted);
    let mut out = format!("loom split {} -m {message}", hunk_select::quoted(target));
    if patch {
        out.push_str(" -p");
    }
    for file in files {
        out.push(' ');
        out.push_str(&hunk_select::quoted(file));
    }
    if let Some(from) = hunks.from.as_deref().filter(|_| !hunks.is_empty()) {
        for id in &hunks.ids {
            out.push_str(&format!(" --hunks {}", hunk_select::quoted(id)));
        }
        out.push_str(&format!(" --hunks-from {}", hunk_select::quoted(from)));
    }
    out
}

/// Commit with `-m` message or open the editor; either way the new commit
/// gets a Change-Id (Spec 002).
fn commit_or_editor(
    repo: &Repository,
    workdir: &std::path::Path,
    message: Option<&str>,
) -> Result<()> {
    match message {
        Some(m) => git::commit(workdir, m),
        None => {
            git::commit_with_editor(workdir)?;
            changeid::ensure_on_head_or_warn(repo, workdir, None);
            Ok(())
        }
    }
}

/// Split a commit into two sequential commits, selecting files (or hunks with
/// `-p`) for the first one.
pub fn run(
    target: String,
    message: Option<String>,
    patch: bool,
    hunks: HunkArgs,
    files: Vec<String>,
    theme: &graph::Theme,
) -> Result<()> {
    let prompt_hint = invocation(&target, message.as_deref(), patch, &hunks, &files);

    // Without -m the first commit would open $GIT_EDITOR, which hangs a headless agent.
    if agent_mode::enabled() && message.is_none() {
        return Err(agent_mode::respond_needs_input(
            agent_mode::InputKind::Text,
            "Message for the first commit",
            vec![],
            false,
            &format!("re-run with: {prompt_hint}"),
        ));
    }

    let repo = repo::open_repo()?;

    let resolved = repo::resolve_arg(&repo, &target, &[TargetKind::Commit])?;

    let picker = patch.then_some(Picker {
        // Built without the selection flags: `respond` appends its own.
        command: invocation(
            &target,
            message.as_deref(),
            patch,
            &HunkArgs::default(),
            &files,
        ),
        hunks,
        // `split` stages a binary or deleted file whole (spec 013).
        whole_files: true,
        // Both commits it writes are the one it lists.
        target_hash: None,
    });

    match resolved {
        Target::Commit(hash) => split_commit(&repo, &hash, message, picker, files, theme),
        _ => unreachable!(),
    }
}

/// Split a commit, using provided files or an interactive picker if none are given.
/// `picker` is `Some` exactly when `-p` was given: hunk-level split.
fn split_commit(
    repo: &Repository,
    commit_hash: &str,
    message: Option<String>,
    picker: Option<Picker>,
    files: Vec<String>,
    theme: &graph::Theme,
) -> Result<()> {
    let workdir = repo::require_workdir(repo, COMMAND)?;
    let commit = repo.revparse_single(commit_hash)?.peel_to_commit()?;
    let commit_oid = commit.id();

    if commit.parent_count() > 1 {
        bail!("Cannot split a merge commit");
    }

    let original_msg = commit.message().unwrap_or("").trim().to_string();

    // The commit's file list is repo-relative, so the given paths must be too.
    let files: Vec<String> = files
        .iter()
        .map(|f| repo::to_repo_path(repo, f))
        .collect::<Result<_>>()?;

    if let Some(picker) = picker {
        let oid_str = commit_oid.to_string();
        let selections =
            staging::run_commit_hunk_picker(workdir, &oid_str, &files, &picker, theme)?
                .ok_or_else(msg::cancelled)?;

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

/// Split a commit with pre-selected files, bypassing the picker (for tests).
#[cfg(test)]
pub fn split_commit_with_selection(
    repo: &Repository,
    commit_hash: &str,
    selected: Vec<String>,
    message: String,
) -> Result<()> {
    let theme = graph::Theme::dark();
    split_commit(repo, commit_hash, Some(message), None, selected, &theme)
}

/// Show an interactive file picker for splitting.
fn pick_files(files: &[String]) -> Result<Vec<String>> {
    let selected = msg::multi_select(
        "Select files for the first commit:",
        files.to_vec(),
        "re-run with: loom split <target> -m <message> <files...>",
    )?;

    if selected.len() == files.len() {
        bail!("Must leave at least one file for the second commit");
    }

    Ok(selected)
}

/// Shared split dispatcher: save staged, run split logic, restore staged, print success.
///
/// `do_split` receives `is_head: bool` and returns `(hash1, hash2)`.
fn run_split(
    repo: &Repository,
    workdir: &std::path::Path,
    commit_oid: Oid,
    do_split: impl FnOnce(bool) -> Result<(String, String)>,
) -> Result<()> {
    let is_head = repo::head_oid(repo)? == commit_oid;
    let oid_str = commit_oid.to_string();
    let short_hash = git::short_hash(&oid_str);
    // Save pre-existing staged changes so `reset --mixed` does not discard them.
    let saved_staged = staging::save_and_unstage_staged(repo, workdir)?;
    let split_result = do_split(is_head);
    // Restore pre-existing staged changes regardless of outcome.
    saved_staged.restore();
    let (h1, h2) = split_result?;
    msg::success(&format!(
        "Split `{}` into {} and {}",
        short_hash,
        repo::describe_commit(workdir, &h1),
        repo::describe_commit(workdir, &h2)
    ));
    Ok(())
}

/// Wrap a head-split closure in an edit-and-continue rebase for non-HEAD commits.
///
/// Aborts the rebase automatically on error — split does not save LoomState.
fn perform_non_head_with(
    repo: &Repository,
    workdir: &std::path::Path,
    commit_oid: Oid,
    do_head_split: impl FnOnce() -> Result<(String, String)>,
) -> Result<(String, String)> {
    weave::start_edit_rebase(repo, workdir, commit_oid)?;
    let result = match do_head_split() {
        Ok(hashes) => hashes,
        Err(e) => return Err(git::rebase_abort_then_cleanup(workdir, e, || {})),
    };
    // Continue the rebase: later commits replay on top of the split commits, so
    // hash1/hash2 stay valid. Aborts on conflict — split saves no LoomState.
    git::continue_rebase_expecting_edit(workdir, git::AfterStop::nothing())?;
    Ok(result)
}

/// Perform the file-based split operation.
fn perform_split(
    repo: &Repository,
    workdir: &std::path::Path,
    commit_oid: Oid,
    selected: &[String],
    remaining: &[String],
    msg1: Option<&str>,
    msg2: &str,
) -> Result<()> {
    let (msg1, msg2) = stamped_messages(repo, workdir, msg1, msg2)?;
    let (msg1, msg2) = (msg1.as_deref(), msg2.as_str());
    run_split(repo, workdir, commit_oid, |is_head| {
        if is_head {
            perform_head_split(repo, workdir, selected, remaining, msg1, msg2)
        } else {
            perform_non_head_with(repo, workdir, commit_oid, || {
                perform_head_split(repo, workdir, selected, remaining, msg1, msg2)
            })
        }
    })
}

/// Both messages with their Change-Id (Spec 013), stamped before the split
/// dismantles the commit so no generation failure can strike in between. The
/// original message keeps its id; one without gets a fresh one.
fn stamped_messages(
    repo: &Repository,
    workdir: &std::path::Path,
    msg1: Option<&str>,
    msg2: &str,
) -> Result<(Option<String>, String)> {
    let stamped1 = match msg1 {
        Some(m) => Some(changeid::for_message(repo, workdir, m, None)?),
        None => None,
    };
    let mut stamped2 = changeid::for_message(repo, workdir, msg2, None)?;
    // Same ident, HEAD, and text give the same fresh id twice; the first
    // half's id then salts the second's.
    let id1 = stamped1.as_deref().and_then(changeid::from_message);
    if let Some(id1) = &id1
        && changeid::from_message(&stamped2).as_deref() == Some(id1)
    {
        let id2 = changeid::generate_unlike(workdir, msg2, id1)?;
        stamped2 = changeid::for_message(repo, workdir, msg2, Some(&id2))?;
    }
    Ok((stamped1, stamped2))
}

/// Split HEAD commit (no rebase needed).
///
/// ```text
/// reset_mixed(HEAD~1) → stage selected → commit(msg1) → stage remaining → commit(msg2)
/// ```
///
/// Returns `(hash1, hash2)` — the two new commit hashes.
fn perform_head_split(
    repo: &Repository,
    workdir: &std::path::Path,
    selected: &[String],
    remaining: &[String],
    msg1: Option<&str>,
    msg2: &str,
) -> Result<(String, String)> {
    git::reset_mixed(workdir, "HEAD~1")?;

    let selected_refs: Vec<&str> = selected.iter().map(|s| s.as_str()).collect();
    git::stage_files(workdir, &selected_refs)?;
    commit_or_editor(repo, workdir, msg1)?;

    let remaining_refs: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();
    git::stage_files(workdir, &remaining_refs)?;
    git::commit(workdir, msg2)?;

    let hash2 = git::rev_parse(workdir, "HEAD")?;
    let hash1 = git::rev_parse(workdir, "HEAD~1")?;

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
    let (msg1, msg2) = stamped_messages(repo, workdir, msg1, msg2)?;
    let (msg1, msg2) = (msg1.as_deref(), msg2.as_str());
    run_split(repo, workdir, commit_oid, |is_head| {
        if is_head {
            perform_head_split_by_hunks(repo, workdir, selections, msg1, msg2)
        } else {
            perform_non_head_with(repo, workdir, commit_oid, || {
                perform_head_split_by_hunks(repo, workdir, selections, msg1, msg2)
            })
        }
    })
}

/// HEAD hunk-based split.
///
/// ```text
/// reset_mixed(HEAD~1) → apply selected hunks → commit(msg1) → stage remaining → commit(msg2)
/// ```
fn perform_head_split_by_hunks(
    repo: &Repository,
    workdir: &std::path::Path,
    selections: &[FileEntry],
    msg1: Option<&str>,
    msg2: &str,
) -> Result<(String, String)> {
    // Captured before the reset moves HEAD off the commit being split. A
    // submodule has to move by this diff: `git add` would stage whatever its
    // checkout holds, not what the commit recorded.
    let mut gitlinks = std::collections::HashMap::new();
    for path in git::commit_gitlinks(workdir, "HEAD")?.into_keys() {
        let diff = git::diff_commit_file(workdir, "HEAD", &path)?;
        gitlinks.insert(path, diff);
    }

    git::reset_mixed(workdir, "HEAD~1")?;

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
        if let Some(diff) = gitlinks.get(&file.path) {
            git::apply_cached_patch(workdir, diff)?;
        } else if file.binary || file.index_status == 'D' {
            git::stage_path(workdir, &file.path)?;
        } else {
            selected_patch.push_str(&diff::build_hunk_patch(&file.path, &selected));
        }
    }
    if !selected_patch.is_empty() {
        git::apply_cached_patch(workdir, &selected_patch)?;
    }

    commit_or_editor(repo, workdir, msg1)?;

    for file in selections {
        if file.hunks.iter().any(|h| !h.selected) {
            if let Some(diff) = gitlinks.get(&file.path) {
                git::apply_cached_patch(workdir, diff)?;
            } else {
                git::stage_path(workdir, &file.path)?;
            }
        }
    }
    git::commit(workdir, msg2)?;

    let hash2 = git::rev_parse(workdir, "HEAD")?;
    let hash1 = git::rev_parse(workdir, "HEAD~1")?;
    Ok((hash1, hash2))
}

#[cfg(test)]
#[path = "split_test.rs"]
mod tests;
