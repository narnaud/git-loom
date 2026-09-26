use std::path::Path;

use anyhow::{Result, bail};

/// Amend the current commit, optionally replacing its message
/// (`git commit --quiet --allow-empty --amend --only [-m msg]`). `--only` keeps
/// staged changes out, `--quiet` suppresses the summary on the editor path, and
/// a `None` message inherits stdio so git can open the user's editor.
pub fn commit_amend(workdir: &Path, message: Option<&str>) -> Result<()> {
    if let Some(msg) = message {
        super::run_git(
            workdir,
            &[
                "commit",
                "--quiet",
                "--allow-empty",
                "--amend",
                "--only",
                "-m",
                msg,
            ],
        )
    } else {
        super::run_git_interactive(
            workdir,
            &["commit", "--quiet", "--allow-empty", "--amend", "--only"],
        )
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
/// [`commit_opts`]: loom reads the result below and rewrites on it, which an
/// uncaptured run cannot report. Errs when git printed instead of committing,
/// which it can do while exiting 0.
pub fn commit_amend_no_edit(workdir: &Path, opts: &[&str]) -> Result<()> {
    // Taken only with forwarded arguments: without one git cannot be told to
    // do anything but amend.
    let before = if opts.is_empty() {
        None
    } else {
        Some((
            super::rev_parse(workdir, "HEAD")?,
            super::rev_parse(workdir, "HEAD^{tree}")?,
        ))
    };

    let mut args = vec!["commit"];
    args.extend(opts);
    args.extend(["--amend", "--no-edit", "--allow-empty"]);
    super::run_git(workdir, &args)?;

    if let Some((head, tree)) = before {
        // Every fold amend has something to commit, so HEAD's tree has to come
        // out different; git printing instead of committing leaves it alone.
        // Read from HEAD on both sides, so a `pre-commit` or `post-commit` hook
        // that stages something of its own cannot fail an amend that happened.
        if super::rev_parse(workdir, "HEAD^{tree}")? == tree {
            bail!(
                "`git commit --amend` left the commit as it was, so nothing was amended\n\
                 Either an argument after `--` kept git from committing, or what was staged \
                 already matched the commit"
            );
        }
        // An amend replaces HEAD. Loom's own `--amend` comes last and wins, so
        // this only catches a git that stops resolving the pair that way.
        if super::rev_parse(workdir, "HEAD^").is_ok_and(|parent| parent == head) {
            bail!("`git commit --amend` committed on top of the target instead of amending it");
        }
    }
    Ok(())
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
            path,
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
    let mut args = vec!["add"];
    args.extend(opts);
    args.push("--");
    args.extend(&to_add);
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
    // `:(literal)` for the reason [`super::ls_files`] gives.
    let literal: Vec<String> = files.iter().map(|f| format!(":(literal){f}")).collect();
    let source = format!("--source={source}");
    let mut args = vec!["restore", "--staged", source.as_str(), "--"];
    args.extend(literal.iter().map(|f| f.as_str()));
    super::run_git(workdir, &args)
}

/// Create a commit with a message (`git commit -m <message>`).
pub fn commit(workdir: &Path, message: &str) -> Result<()> {
    commit_captured(workdir, message, &[])
}

/// Create a commit with forwarded options, always captured — unlike
/// [`commit_opts`], which steps back once the user forwards anything.
///
/// For a commit loom makes as one step of a longer operation (`fold`): loom
/// reads the result itself and rewrites on it, and the rebase that follows
/// needs the terminal.
/// Unlike [`commit_amend_no_edit`] this does not check that git committed —
/// what proves it differs per caller — so a caller that then rewrites history
/// must check for itself (see `fold::committed_onto`).
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
    if message.is_none() || runs_uncaptured(opts) {
        super::run_git_interactive(workdir, &args)
    } else {
        super::run_git(workdir, &args)
    }
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
    super::run_git(workdir, &["reset", "--hard", target])
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

/// Create a commit by opening the user's editor for the message (`git commit`,
/// no `-m`). Inherits stdio so the editor reaches the terminal.
pub fn commit_with_editor(workdir: &Path) -> Result<()> {
    commit_opts(workdir, None, &[])
}

#[cfg(test)]
#[path = "git_commit_test.rs"]
mod tests;
