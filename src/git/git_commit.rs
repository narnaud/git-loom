use std::path::Path;

use anyhow::Result;

use crate::core::msg;

/// Amend the current commit, optionally replacing its message
/// (`git commit --quiet [opts] --allow-empty --amend --only [-m msg]`).
/// `--only` keeps staged changes out, `--quiet` suppresses the summary on the
/// editor path, and a `None` message inherits stdio so git can open the user's
/// editor.
///
/// `opts` is what followed a `--` on a `reword` command line (Spec 021),
/// placed before loom's own arguments so git's last-wins parse keeps
/// `--amend` and `--only`.
pub fn commit_amend(workdir: &Path, message: Option<&str>, opts: &[&str]) -> Result<()> {
    let mut args = vec!["commit", "--quiet"];
    args.extend(opts);
    args.extend(["--allow-empty", "--amend", "--only"]);
    if let Some(msg) = message {
        args.extend(["-m", msg]);
        super::run_git(workdir, &args)
    } else {
        super::run_git_interactive(workdir, &args)
    }
}

/// Replace the current commit's message with `--no-verify`, so the
/// `pre-commit` and `commit-msg` hooks that already ran on the commit are not
/// run again by loom's own follow-up. Options forwarded to the first commit
/// are not repeated here, so the new object follows git config alone
/// (Spec 002).
pub fn commit_amend_message_unverified(workdir: &Path, message: &str) -> Result<()> {
    super::run_git(
        workdir,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "--amend",
            "--only",
            "--no-verify",
            // The message is stored text plus one trailer line: append it as is.
            "--cleanup=verbatim",
            "-m",
            message,
        ],
    )
}

/// Amend the current commit, keeping its message and including staged changes
/// (`git commit --amend --no-edit --allow-empty` — no `--only`, unlike
/// [`commit_amend`]).
///
/// `opts` is what followed a `--` on a `fold` command line (Spec 021),
/// forwarded verbatim and placed first, so git's last-wins parse keeps the
/// `--amend` loom asked for. Captured whatever it holds, unlike
/// [`commit_opts`]: it is one step of a rewrite that goes on from it.
pub fn commit_amend_no_edit(workdir: &Path, opts: &[&str]) -> Result<()> {
    let mut args = vec!["commit"];
    args.extend(opts);
    args.extend(["--amend", "--no-edit", "--allow-empty"]);
    super::run_git(workdir, &args)
}

/// Commit the index as it is under `source`'s message, author and date
/// (`git commit --allow-empty --no-verify -C <source>`): keeps a pick a
/// resolution emptied as an empty commit, so the step that lands on it still
/// can (Spec 014). `--no-verify` because a rebase pick skips `pre-commit` and
/// `commit-msg` too.
pub fn commit_empty_copy_of(workdir: &Path, source: &str) -> Result<()> {
    super::run_git(
        workdir,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "--no-verify",
            "-C",
            source,
        ],
    )
}

/// True when `path` is gone from both the working tree and the index because
/// its deletion is already staged.
///
/// Such a path matches nothing, so `git add` and `git rm` both fail with
/// "pathspec did not match any files" even though it is correctly staged.
fn deletion_already_staged(workdir: &Path, path: &str) -> Result<bool> {
    // symlink_metadata, not exists(): a broken symlink resolves to nothing but
    // is still a working tree entry that `git add` has to stage.
    if workdir.join(path).symlink_metadata().is_ok() {
        return Ok(false);
    }
    let out = super::run_git_stdout(
        workdir,
        &[
            "diff",
            "--cached",
            "--name-only",
            "--diff-filter=D",
            "--",
            &super::literal_pathspec(path),
        ],
    )?;
    Ok(!out.trim().is_empty())
}

/// Stage specific files (`git add <files>`). Files whose deletion is already
/// staged are left alone, since `git add` has nothing left to match.
pub fn stage_files(workdir: &Path, files: &[&str]) -> Result<()> {
    stage_files_opts(workdir, files, &[])
}

/// Stage specific files, with extra `git add` options from the user.
///
/// `opts` is what followed a `--` on the loom command line; it precedes the
/// pathspec so an option is still read as an option. Forwarded options run
/// uncaptured, since stdout is the whole output of `--dry-run` and `-v`.
pub fn stage_files_opts(workdir: &Path, files: &[&str], opts: &[&str]) -> Result<()> {
    let mut to_add: Vec<&str> = Vec::with_capacity(files.len());
    for file in files {
        if !deletion_already_staged(workdir, file)? {
            to_add.push(file);
        }
    }
    if to_add.is_empty() {
        return Ok(());
    }
    let literal = super::literal_pathspecs(&to_add);
    let mut args = vec!["add"];
    args.extend(opts);
    args.push("--");
    args.extend(literal.iter().map(String::as_str));
    run_add(workdir, &args, opts)
}

/// Stage all changes for a specific path, including deletions. Forwards to
/// [`stage_files`], so a path whose deletion is already staged is left alone.
pub fn stage_path(workdir: &Path, path: &str) -> Result<()> {
    stage_files(workdir, &[path])
}

/// Stage `files` as `source` records them, whatever the working tree holds
/// (`git restore --staged --source`). A path absent from `source` is dropped
/// from the index; one absent from both fails.
pub fn stage_from(workdir: &Path, source: &str, files: &[&str]) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let literal = super::literal_pathspecs(files);
    let source = format!("--source={source}");
    let mut args = vec!["restore", "--staged", source.as_str(), "--"];
    args.extend(literal.iter().map(|f| f.as_str()));
    super::run_git(workdir, &args)
}

/// Create a commit with forwarded options, always captured — unlike
/// [`commit_opts`], which steps back once the user forwards anything.
///
/// For a commit loom makes as one step of a longer operation (`fold`): loom
/// reads the result itself and rewrites on it, and the rebase that follows
/// needs the terminal.
pub fn commit_captured(workdir: &Path, message: &str, opts: &[&str]) -> Result<()> {
    let mut args = vec!["commit"];
    args.extend(opts);
    args.extend(["-m", message]);
    super::run_git(workdir, &args)
}

/// Create a commit, with extra `git commit` options from the user.
///
/// `opts` is what followed a `--` on the loom command line. Only the plain
/// `-m` case is captured: a captured command forces `GIT_EDITOR=true` and
/// swallows stdout, which would defeat a forwarded `-e` or `--interactive` and
/// hide the report a `--dry-run` exists to print. Without a message git opens
/// the user's editor for the same reason.
pub fn commit_opts(workdir: &Path, message: Option<&str>, opts: &[&str]) -> Result<()> {
    let mut args = vec!["commit"];
    args.extend(opts);
    if let Some(message) = message {
        args.extend(["-m", message]);
    }
    if commit_runs_uncaptured(message, opts) {
        super::run_git_interactive(workdir, &args)
    } else {
        super::run_git(workdir, &args)
    }
}

/// [`commit_opts`], committing again after a hook fixed files
/// ([`retry_after_hook_fixes`]) when git runs captured. Uncaptured, the user
/// saw the hook fail, and may have cancelled in the editor.
pub fn commit_opts_retrying(workdir: &Path, message: Option<&str>, opts: &[&str]) -> Result<()> {
    if commit_runs_uncaptured(message, opts) {
        return commit_opts(workdir, message, opts);
    }
    retry_after_hook_fixes(workdir, || commit_opts(workdir, message, opts))
}

/// Run `commit`, and once more if it failed after a hook rewrote fully staged
/// files in the working tree, as a formatter run from `pre-commit` does (Spec 006).
/// Only for a commit of the user's own work: the retry commits the hook's
/// version of those files.
pub fn retry_after_hook_fixes(workdir: &Path, commit: impl Fn() -> Result<()>) -> Result<()> {
    // Best effort: without the list there is just no retry.
    let Ok(fully_staged) = fully_staged_paths(workdir) else {
        return commit();
    };
    let Err(err) = commit() else {
        return Ok(());
    };
    // Only a file whose index matched its working tree is staged again, so no
    // unstaged work gets into the commit.
    if fully_staged.is_empty() {
        return Err(err);
    }
    let staged: Vec<&str> = fully_staged.iter().map(String::as_str).collect();
    let Ok(unstaged) = unstaged_paths(workdir, &staged) else {
        return Err(err);
    };
    let fixed: Vec<&str> = fully_staged
        .iter()
        .filter(|p| unstaged.contains(*p))
        .map(String::as_str)
        .collect();
    if fixed.is_empty() {
        return Err(err);
    }
    // The hook rewrote the working tree, so the index as the failed commit
    // left it holds the only copy of what was staged: put it back if the
    // retry fails.
    let Ok(entries) = index_entries(workdir, &fixed) else {
        return Err(err);
    };
    let names = path_list(&fixed);
    let restore = || {
        if let Err(e) = restore_index_entries(workdir, &entries) {
            let args: Vec<String> = entries
                .iter()
                .map(|entry| format!("--cacheinfo {}", super::shell_quote(entry)))
                .collect();
            msg::warn(&format!(
                "could not restore the staged version of {names}: {e}\n\
                 `git update-index {}` puts it back",
                args.join(" ")
            ));
        }
    };
    if stage_files(workdir, &fixed).is_err() {
        restore();
        return Err(err);
    }
    msg::warn(&format!(
        "a commit hook changed {names}; committing again with its changes"
    ));
    commit().inspect_err(|_| restore())
}

/// `paths` as backticked names, the first few only.
fn path_list(paths: &[&str]) -> String {
    const SHOWN: usize = 5;
    let mut list: Vec<String> = paths.iter().take(SHOWN).map(|p| format!("`{p}`")).collect();
    if paths.len() > SHOWN {
        list.push(format!("{} more", paths.len() - SHOWN));
    }
    list.join(", ")
}

/// Tracked paths whose index differs from HEAD and matches the working tree.
fn fully_staged_paths(workdir: &Path) -> Result<Vec<String>> {
    let out = super::run_git_stdout(
        workdir,
        &[
            "diff",
            "--cached",
            "--name-only",
            "-z",
            "--no-renames",
            "--ignore-submodules=all",
        ],
    )?;
    let staged: Vec<&str> = out.split('\0').filter(|p| !p.is_empty()).collect();
    if staged.is_empty() {
        return Ok(Vec::new());
    }
    // Scoped to the staged paths, so only they get a stat, not the whole tree.
    let unstaged = unstaged_paths(workdir, &staged)?;
    Ok(staged
        .into_iter()
        .filter(|p| !unstaged.contains(*p))
        .map(str::to_string)
        .collect())
}

/// Those of `paths` whose working tree differs from the index.
fn unstaged_paths(workdir: &Path, paths: &[&str]) -> Result<std::collections::HashSet<String>> {
    let literal = super::literal_pathspecs(paths);
    let mut args = vec!["diff", "--name-only", "-z", "--ignore-submodules=all", "--"];
    args.extend(literal.iter().map(String::as_str));
    let out = super::run_git_stdout(workdir, &args)?;
    Ok(out
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect())
}

/// `git ls-files -s` entries of `paths`, as `--cacheinfo` values.
fn index_entries(workdir: &Path, paths: &[&str]) -> Result<Vec<String>> {
    let literal = super::literal_pathspecs(paths);
    let mut args = vec!["ls-files", "-s", "-z", "--"];
    args.extend(literal.iter().map(String::as_str));
    let out = super::run_git_stdout(workdir, &args)?;
    Ok(out
        .split('\0')
        .filter_map(|entry| {
            let (info, path) = entry.split_once('\t')?;
            let mut fields = info.split(' ');
            let (mode, oid) = (fields.next()?, fields.next()?);
            Some(format!("{mode},{oid},{path}"))
        })
        .collect())
}

fn restore_index_entries(workdir: &Path, entries: &[String]) -> Result<()> {
    let mut args = vec!["update-index"];
    for entry in entries {
        args.extend(["--cacheinfo", entry.as_str()]);
    }
    super::run_git(workdir, &args)
}

/// Whether [`commit_opts`] runs git uncaptured: with no message, or with
/// forwarded options outside agent mode.
fn commit_runs_uncaptured(message: Option<&str>, opts: &[&str]) -> bool {
    message.is_none() || runs_uncaptured(opts)
}

/// Whether a git command carrying `opts` the user forwarded should run
/// uncaptured.
///
/// A forwarded option may exist purely to print (`--dry-run`, `-v`) or to open
/// something (`-e`, `--interactive`), and a captured command sends the report
/// to the trace log and answers the editor with `true`. Agent mode is the
/// exception: there is nobody to close an editor there, and hanging on one is
/// the failure `commit`'s own agent-mode guard exists to prevent.
fn runs_uncaptured(opts: &[&str]) -> bool {
    !opts.is_empty() && !crate::core::agent_mode::enabled()
}

/// Mixed reset to a target ref (`git reset <target>`): HEAD moves, and what the
/// commit held comes back as unstaged working-tree changes.
pub fn reset_mixed(workdir: &Path, target: &str) -> Result<()> {
    super::run_git(workdir, &["reset", target])
}

/// Soft reset to a target ref (`git reset --soft <target>`): undoing a commit
/// loom made itself, so what it held goes back to the index as it was staged.
pub fn reset_soft(workdir: &Path, target: &str) -> Result<()> {
    super::run_git(workdir, &["reset", "--soft", target])
}

/// Hard reset to a target ref (`git reset --hard <target>`), discarding all
/// working directory and index changes.
pub fn reset_hard(workdir: &Path, target: &str) -> Result<()> {
    // Never into submodules, whatever `submodule.recurse` says: no snapshot a
    // rollback replays holds a submodule's own work, which it would wipe.
    super::run_git(
        workdir,
        &["reset", "--hard", "--no-recurse-submodules", target],
    )
}

/// Stage all changes — staged, unstaged, and untracked (`git add -A`).
pub fn stage_all(workdir: &Path) -> Result<()> {
    stage_all_opts(workdir, &[])
}

/// Stage all changes, with extra `git add` options from the user.
///
/// `opts` is what followed a `--` on the loom command line.
pub fn stage_all_opts(workdir: &Path, opts: &[&str]) -> Result<()> {
    let mut args = vec!["add"];
    args.extend(opts);
    args.push("-A");
    run_add(workdir, &args, opts)
}

/// Run a `git add` loom assembled, capturing it unless the user's own `opts`
/// need to reach the terminal. See [`runs_uncaptured`].
fn run_add(workdir: &Path, args: &[&str], opts: &[&str]) -> Result<()> {
    if runs_uncaptured(opts) {
        super::run_git_interactive(workdir, args)
    } else {
        super::run_git(workdir, args)
    }
}

#[cfg(test)]
#[path = "git_commit_test.rs"]
mod tests;
