use std::path::Path;

use anyhow::{Context, Result, bail};
use git2::Repository;
use serde::{Deserialize, Serialize};

use crate::core::agent_mode;
use crate::core::changeid;
use crate::core::graph;
use crate::core::hunk_select::{self, HunkArgs};
use crate::core::msg;
use crate::core::repo;
use crate::core::staging::{self, StagedAside};
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave::{self, RebaseOutcome, Weave};
use crate::git;

#[derive(Serialize, Deserialize)]
struct CommitContext {
    branch_name: String,
    /// Staged work set aside before staging this commit's files, put back once
    /// the commit lands. The rollback keeps the whole pre-commit index instead,
    /// so this cannot live in `Rollback` (see [`run`]).
    ///
    /// `None` only in a state file written before this field existed, where the
    /// same patch is the rollback's — an abort reading that file restores the
    /// set-aside subset it always did, not the whole index.
    #[serde(default)]
    saved_staged: Option<String>,
}

/// Create a commit without leaving the integration branch.
///
/// Stages files, then either commits on the integration branch itself (a
/// loose commit: `-i`, or a branch name matching its upstream), or commits at
/// HEAD and uses Weave to relocate it to the target feature branch (creating
/// merge topology if needed).
///
/// `git_args` is whatever followed a `--`, passed to `git commit` (see spec
/// 021). It applies to the commit loom creates, not to the rebase that
/// relocates it.
pub fn run(
    branch: Option<String>,
    integration: bool,
    message: Option<String>,
    patch: Option<HunkArgs>,
    files: Vec<String>,
    git_args: Vec<String>,
    theme: &graph::Theme,
) -> Result<()> {
    let git_opts: Vec<&str> = git_args.iter().map(String::as_str).collect();

    // Without -m the commit would open $GIT_EDITOR, which hangs a headless agent.
    if agent_mode::enabled() && message.is_none() {
        let command = if patch.is_some() {
            hunk_select::patch_command("loom commit -m <message> [-b <branch> | -i]", &files)
        } else {
            "loom commit -m <message> [-b <branch> | -i] [files...]".to_string()
        };
        return Err(agent_mode::respond_needs_input(
            agent_mode::InputKind::Text,
            "Commit message",
            vec![],
            false,
            &format!(
                "re-run with: {command}{}",
                hunk_select::git_args_suffix(&git_opts)
            ),
        ));
    }

    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "commit")?.to_path_buf();
    let git_dir = repo.path().to_path_buf();

    // Gather repo info once — also serves as verification that we're on an
    // integration branch (gather_repo_info requires an upstream).
    let info = repo::gather_repo_info(&repo, false, 1).context(
        "Must be on an integration branch to use commit\n\
         Use `git commit` directly on feature branches",
    )?;

    // A loose commit lands on the integration branch itself: no rebase, no
    // state file, so nothing below it needs the index snapshot.
    let loose = integration
        || (branch.is_none()
            && info.branch_name == repo::upstream_local_branch(&info.upstream.label));

    // Ask for the branch before `-p` stages anything: the branch prompt fires
    // after staging, and answering it re-runs the command, which without `-p`
    // stages whole files and undoes the picking.
    if agent_mode::enabled() && patch.is_some() && !loose && branch.is_none() {
        let hint = format!(
            "re-run with: {}{} (a new name creates the branch), \
             or -i for the integration branch itself",
            hunk_select::patch_command(
                &format!("loom commit -b <branch> -m {}", message_arg(&message)),
                &files,
            ),
            hunk_select::git_args_suffix(&git_opts)
        );
        ask_branch_name(&info, &hint)?;
        bail!("agent mode must answer the branch prompt");
    }

    // Resolved once, before staging: a bad `-b` would leave the index changed,
    // and staging can shift the short ID it was typed as.
    let explicit_branch = match &branch {
        Some(b) => Some(check_explicit_branch(&repo, &info, &workdir, b)?),
        None => None,
    };

    // The index exactly as the user left it: what `loom abort` and a refused
    // rebase have to put back, since the reset below them undoes the commit
    // and leaves its content unstaged. A whole staged binary rides along into
    // `state.json`; losing the staging is the worse trade.
    let index_before = if loose {
        String::new()
    } else {
        git::diff_cached(&workdir)?
    };

    // The replay hints repeat what the agent typed, not the stamped message.
    let replay_message = message_arg(&message);

    // Stamp before the index is touched: nothing below may fail without
    // restoring what `resolve_staging` sets aside, and this can (`git var`).
    let message = match &message {
        Some(m) => Some(changeid::for_message(&repo, &workdir, m, None)?),
        None => None,
    };

    // Stage files, saving aside any pre-existing staged file this commit must
    // not take.
    let staged_aside = if let Some(hunks) = patch {
        // The listing tells the agent to repeat this invocation (spec 019), so
        // it names the branch only when this one did: inventing `-i` would send
        // the replay to the integration branch instead of the branch prompt.
        let target = match (&branch, integration) {
            (Some(name), _) => format!(" -b {}", hunk_select::quoted(name)),
            (None, true) => " -i".to_string(),
            (None, false) => String::new(),
        };
        let command =
            hunk_select::patch_command(&format!("loom commit{target} -m {replay_message}"), &files);
        let picker = hunk_select::worktree_picker(hunks, command, None, &git_opts);
        resolve_staging_patch(&repo, &workdir, &picker, &files, theme)?
    } else {
        resolve_staging(&repo, &workdir, &files)?
    };

    repo::verify_has_staged_changes(&repo)?;

    let do_commit = || -> Result<()> {
        let head_before = repo::head_oid(&repo).ok();
        git::commit_opts(&workdir, message.as_deref(), &git_opts)?;
        // The editor path: the message is only known now. A forwarded
        // `--dry-run` creates nothing, and HEAD is then not loom's to amend.
        if message.is_none() && repo::head_oid(&repo).ok() != head_before {
            changeid::ensure_on_head_or_warn(&repo, &workdir, None);
        }
        Ok(())
    };

    // Loose commit: commit on the integration branch itself, targeting no
    // feature branch. Happens with -i, or with no -b when the local branch name
    // matches the upstream's local counterpart (e.g. "main" tracking
    // "origin/main").
    if loose {
        let result = do_commit();
        // Put back either way: a loose commit writes no state file, so there
        // is no later owner for the patch and no rollback to hold it.
        staged_aside.restore();
        result?;
        let new_head = repo::head_oid(&repo)?;
        msg::success(&format!(
            "Created commit {}",
            repo::describe_commit(&workdir, &new_head.to_string())
        ));
        return Ok(());
    }

    let saved_head = repo::head_oid(&repo)?.to_string();

    // Resolve branch target (may create a new branch at merge-base).
    // Returns whether the branch was newly created — only newly-created
    // branches are deleted on rollback (not pre-existing empty ones).
    let (branch_name, branch_is_new) =
        resolve_branch_target(&repo, &info, &workdir, explicit_branch)?;

    // Empty branches (pointing at merge-base) need a branch section and
    // merge entry created in the Weave before moving the commit there.
    let branch_is_empty =
        is_branch_at_merge_base(&repo, &branch_name, info.upstream.merge_base_oid)?;

    do_commit()?;

    let head_oid = repo::head_oid(&repo)?;

    let mut graph = Weave::from_repo_with_info(&repo, &info)?;

    if branch_is_empty {
        graph.add_branch_section(
            branch_name.clone(),
            vec![branch_name.clone()],
            vec![],
            "onto".to_string(),
        );
        graph.add_merge(branch_name.clone(), None, None);
    }

    graph.move_commit(head_oid, &branch_name)?;

    let todo = graph.to_todo();

    // Save LoomState before the rebase so we can resume on conflict.
    let mut delete_branches = vec![];
    if branch_is_new {
        delete_branches.push(branch_name.clone());
    }
    let ctx = CommitContext {
        branch_name: branch_name.clone(),
        saved_staged: Some(staged_aside.patch().to_string()),
    };
    let state = LoomState {
        command: "commit".to_string(),
        rollback: Rollback {
            reset_mixed_to: saved_head.clone(),
            delete_branches,
            saved_staged_patch: index_before,
            ..Default::default()
        },
        context: serde_json::to_value(&ctx)?,
        // `post_commit` names the new commit by reading `branch_name` back, so
        // a replay that came out empty would report the tip it was appended to.
        protect: vec![head_oid.to_string()],
        targets: Vec::new(),
    };
    transaction::save(&git_dir, &state)?;
    // The state file owns the patch from here: the `Completed` arm puts it
    // back on success, `Rollback` on abort.
    let saved_staged = staged_aside.release();

    let base = graph.base_oid.to_string();

    // The rollback undoes the commit, so an empty replay is reported by
    // `roll_back_failed_rebase`, which knows whether that undo ran.
    let outcome = weave::run_rebase_protecting(&workdir, Some(&base), &todo, state.protected())
        .map_err(|e| transaction::roll_back_failed_rebase(&workdir, &git_dir, &state, e))?;
    match outcome {
        RebaseOutcome::Completed => {
            git::restore_staged_after_rebase(&workdir, &saved_staged);
            transaction::delete(&git_dir)?;
            post_commit(&workdir, &branch_name)?;
        }
        RebaseOutcome::Stopped => {
            transaction::warn_paused(&workdir, "commit");
        }
        RebaseOutcome::Paused => {
            transaction::warn_paused_at_edit(Some("commit"));
        }
    }

    Ok(())
}

/// Resume a `commit` operation after a conflict has been resolved.
pub fn after_continue(
    workdir: &Path,
    rollback: &crate::core::transaction::Rollback,
    context: &serde_json::Value,
) -> Result<()> {
    let ctx: CommitContext =
        serde_json::from_value(context.clone()).context("Failed to parse commit resume context")?;
    let saved_staged = ctx
        .saved_staged
        .unwrap_or_else(|| rollback.saved_staged_patch.clone());
    git::restore_staged_after_rebase(workdir, &saved_staged);
    post_commit(workdir, &ctx.branch_name)
}

/// Post-rebase work: print the success message. The restore runs before the
/// state file goes, so both callers do it themselves.
fn post_commit(workdir: &Path, branch_name: &str) -> Result<()> {
    let new_hash = git::rev_parse(workdir, branch_name)?;

    msg::success(&format!(
        "Created commit {} on branch `{}`",
        repo::describe_commit(workdir, &new_hash),
        branch_name
    ));

    Ok(())
}

/// Resolve staging in patch mode: open the interactive hunk picker.
///
/// Once the picker has run, every staged path it did not return is saved aside
/// and unstaged, so it cannot leak into this commit. Returns that saved patch
/// for restoration after the commit. An exit without a selection — cancelled,
/// refused, or answered as a listing — sets nothing aside.
fn resolve_staging_patch<'a>(
    repo: &Repository,
    workdir: &'a Path,
    picker: &hunk_select::Picker,
    files: &[String],
    theme: &graph::Theme,
) -> Result<StagedAside<'a>> {
    let filter = staging::filter_paths(repo, files)?;
    let Some(picked) = staging::run_hunk_picker(
        repo,
        workdir,
        filter.as_deref(),
        picker,
        staging::LeftOut::KeptStaged,
        theme,
    )?
    else {
        return Err(msg::cancelled());
    };
    // Set aside every staged path the picker did not return, not just those
    // outside the filter: one with no hunk to show, such as a mode-only change,
    // was never offered, so it must not join the commit.
    // What the pick left out stays staged too, though not in the commit.
    let paths: Vec<&str> = picked.paths.iter().map(String::as_str).collect();
    Ok(staging::save_and_unstage_other_staged(repo, workdir, &paths)?.absorb(picked.left_out))
}

/// Resolve staging from the file arguments: an empty list uses the index
/// as-is, `zz` stages everything, and named files are staged after any other
/// pre-existing staged file is saved aside and unstaged so it cannot leak into
/// this commit. Returns that saved patch for later restoration.
fn resolve_staging<'a>(
    repo: &Repository,
    workdir: &'a Path,
    files: &[String],
) -> Result<StagedAside<'a>> {
    if files.is_empty() {
        return Ok(StagedAside::none(workdir));
    }

    if files.iter().any(|f| f == "zz") {
        git::stage_all(workdir)?;
        return Ok(StagedAside::none(workdir));
    }

    let resolved_paths = resolve_file_args(repo, files)?;
    let path_refs: Vec<&str> = resolved_paths.iter().map(|s| s.as_str()).collect();
    let saved_staged = staging::save_and_unstage_other_staged(repo, workdir, &path_refs)?;
    git::stage_files(workdir, &path_refs)?;
    Ok(saved_staged)
}

/// Resolve a slice of user file arguments to repo-relative paths.
fn resolve_file_args(repo: &Repository, files: &[String]) -> Result<Vec<String>> {
    files
        .iter()
        .map(|arg| repo::resolve_file_arg(repo, arg))
        .collect()
}

/// Resolve the target branch: explicit name/shortID, or interactive picker.
///
/// Returns `(branch_name, is_new)` — `is_new` is true when the branch was
/// created by this call (only newly-created branches are deleted on rollback).
///
/// `explicit` is `check_explicit_branch`'s answer for `-b`: a new name is
/// created at the merge-base here.
fn resolve_branch_target(
    repo: &Repository,
    info: &repo::RepoInfo,
    workdir: &std::path::Path,
    explicit: Option<(String, bool)>,
) -> Result<(String, bool)> {
    match explicit {
        Some((name, is_new)) => {
            if is_new {
                create_branch_at_merge_base(workdir, &name, info.upstream.merge_base_oid)?;
            }
            Ok((name, is_new))
        }
        None => pick_branch(repo, info, workdir),
    }
}

/// Every refusal `-b` can get, without creating anything: the branch name and
/// whether it is new.
fn check_explicit_branch(
    repo: &Repository,
    info: &repo::RepoInfo,
    workdir: &std::path::Path,
    branch: &str,
) -> Result<(String, bool)> {
    match repo::resolve_arg(repo, branch, &[repo::TargetKind::Branch]) {
        Ok(target) => {
            let name = target.expect_branch()?;
            if info.branches.iter().any(|b| b.name == name) {
                Ok((name, false))
            } else {
                bail!("Branch '{}' is not woven into the integration branch", name)
            }
        }
        Err(_) => {
            let name = branch.trim().to_string();
            if name.is_empty() {
                bail!("Branch name cannot be empty");
            }
            git::branch_validate_name(workdir, &name)?;

            if repo.find_branch(&name, git2::BranchType::Local).is_ok() {
                bail!(
                    "Branch '{}' exists but is not woven into the integration branch",
                    name
                );
            }
            Ok((name, true))
        }
    }
}

/// The `-m <message>` a replay hint repeats. Only agent mode reads one, and it
/// guarantees `-m`, so the placeholder fills a string nothing prints.
fn message_arg(message: &Option<String>) -> String {
    hunk_select::quoted(message.as_deref().unwrap_or("<message>"))
}

/// Ask for the target branch: pick a woven one, or type a new name.
///
/// In agent mode this always errors, answering with the prompt (`msg::input`).
fn ask_branch_name(info: &repo::RepoInfo, hint: &str) -> Result<String> {
    let branch_names: Vec<String> = info.branches.iter().map(|b| b.name.clone()).collect();

    let not_empty = |s: &str| {
        if s.trim().is_empty() {
            Err("Branch name cannot be empty")
        } else {
            Ok(())
        }
    };

    if branch_names.is_empty() {
        msg::input("Branch name", hint, not_empty)
    } else {
        msg::select_or_input("Select target branch", branch_names, hint, not_empty)
    }
}

/// Interactive branch picker: select an existing woven branch or type a new name.
fn pick_branch(
    repo: &Repository,
    info: &repo::RepoInfo,
    workdir: &std::path::Path,
) -> Result<(String, bool)> {
    let hint = "re-run with: loom commit -b <branch> -m <message> [files...] \
                (a new name creates the branch), or -i for the integration branch itself";
    let name = ask_branch_name(info, hint)?;

    let name = name.trim().to_string();

    // If user typed a name that isn't an existing woven branch, create it
    if !info.branches.iter().any(|b| b.name == name) {
        git::branch_validate_name(workdir, &name)?;
        repo::ensure_branch_not_exists(repo, &name)?;
        create_branch_at_merge_base(workdir, &name, info.upstream.merge_base_oid)?;
        return Ok((name, true));
    }

    Ok((name, false))
}

/// Check if a branch points to the merge-base commit (i.e., has no commits of its own).
fn is_branch_at_merge_base(
    repo: &Repository,
    branch_name: &str,
    merge_base_oid: git2::Oid,
) -> Result<bool> {
    let branch = repo.find_branch(branch_name, git2::BranchType::Local)?;
    let branch_oid = branch.get().target().context("Branch has no target")?;
    Ok(branch_oid == merge_base_oid)
}

/// Create a new branch at the merge-base.
///
/// The branch is not yet woven — weaving happens after the commit is created,
/// in the main `run` flow via Weave.
fn create_branch_at_merge_base(
    workdir: &std::path::Path,
    name: &str,
    merge_base_oid: git2::Oid,
) -> Result<()> {
    let merge_base_hash = merge_base_oid.to_string();

    git::branch_create(workdir, name, &merge_base_hash)?;

    msg::success(&format!(
        "Created branch `{}` at `{}`",
        name,
        git::short_hash(&merge_base_hash)
    ));

    Ok(())
}

#[cfg(test)]
#[path = "commit_test.rs"]
mod tests;
