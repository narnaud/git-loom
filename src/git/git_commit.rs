use std::path::Path;

use anyhow::Result;

/// Amend the current commit, optionally replacing its message.
///
/// Wraps `git commit --quiet --allow-empty --amend --only [-m msg]`.
/// Uses `--only` so that staged changes are not accidentally included, and
/// `--quiet` so the editor path doesn't print git's commit summary.
/// When `message` is `None`, inherits stdio so git can open the user's editor.
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

/// Amend the current commit, keeping its message and including staged changes.
///
/// Wraps `git commit --amend --no-edit --allow-empty`.
/// Unlike `amend()`, this does NOT use `--only`, so staged changes are included.
pub fn commit_amend_no_edit(workdir: &Path) -> Result<()> {
    super::run_git(
        workdir,
        &["commit", "--amend", "--no-edit", "--allow-empty"],
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
            path,
        ],
    )?;
    Ok(!out.trim().is_empty())
}

/// Stage specific files.
///
/// Wraps `git add <files>`. Files whose deletion is already staged are left
/// alone, since there is nothing left for `git add` to match.
pub fn stage_files(workdir: &Path, files: &[&str]) -> Result<()> {
    let mut to_add: Vec<&str> = Vec::with_capacity(files.len());
    for file in files {
        if !deletion_already_staged(workdir, file)? {
            to_add.push(file);
        }
    }
    if to_add.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add", "--"];
    args.extend(&to_add);
    super::run_git(workdir, &args)
}

/// Stage all changes for a specific path, including deletions.
///
/// Forwards to `stage_files`, so a path whose deletion is already staged is
/// left alone.
pub fn stage_path(workdir: &Path, path: &str) -> Result<()> {
    stage_files(workdir, &[path])
}

/// Create a commit with a message.
///
/// Wraps `git commit -m <message>`.
pub fn commit(workdir: &Path, message: &str) -> Result<()> {
    super::run_git(workdir, &["commit", "-m", message])
}

/// Mixed reset to a target ref (uncommit and unstage).
///
/// Wraps `git reset <target>`. Moves HEAD to the target while keeping
/// changes in the working directory as unstaged modifications.
pub fn reset_mixed(workdir: &Path, target: &str) -> Result<()> {
    super::run_git(workdir, &["reset", target])
}

/// Hard reset to a target ref (discard all changes).
///
/// Wraps `git reset --hard <target>`. Moves HEAD and discards all working
/// directory and index changes.
pub fn reset_hard(workdir: &Path, target: &str) -> Result<()> {
    super::run_git(workdir, &["reset", "--hard", target])
}

/// Stage all changes (staged, unstaged, and untracked).
///
/// Wraps `git add -A`.
pub fn stage_all(workdir: &Path) -> Result<()> {
    super::run_git(workdir, &["add", "-A"])
}

/// Create a commit by opening the user's editor for the message.
///
/// Wraps `git commit` (no -m flag). Inherits stdin/stdout so the editor
/// can interact with the terminal.
pub fn commit_with_editor(workdir: &Path) -> Result<()> {
    super::run_git_interactive(workdir, &["commit"])
}

#[cfg(test)]
#[path = "git_commit_test.rs"]
mod tests;
