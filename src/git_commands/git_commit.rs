use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};

/// Amend the current commit, optionally replacing its message.
///
/// Wraps `git commit --allow-empty --amend --only [-m msg]`.
/// Uses `--only` so that staged changes are not accidentally included.
/// When `message` is `None`, inherits stdio so git can open the user's editor.
pub fn amend(workdir: &Path, message: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.current_dir(workdir)
        .args(["commit", "--allow-empty", "--amend", "--only"]);

    if let Some(msg) = message {
        cmd.args(["-m", msg]);
        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Git commit --amend failed:\n{}", stderr);
        }
    } else {
        // No message provided — open editor with inherited stdio
        let status = cmd.status()?;
        if !status.success() {
            bail!("Git commit --amend failed (editor aborted or empty message).");
        }
    }

    Ok(())
}

/// Amend the current commit, keeping its message and including staged changes.
///
/// Wraps `git commit --amend --no-edit`.
/// Unlike `amend()`, this does NOT use `--only`, so staged changes are included.
pub fn amend_no_edit(workdir: &Path) -> Result<()> {
    let output = Command::new("git")
        .current_dir(workdir)
        .args(["commit", "--amend", "--no-edit"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Git commit --amend --no-edit failed:\n{}", stderr);
    }

    Ok(())
}

/// Stage specific files.
///
/// Wraps `git add <files>`.
pub fn stage_files(workdir: &Path, files: &[&str]) -> Result<()> {
    let mut args = vec!["add", "--"];
    args.extend(files);
    super::run_git(workdir, &args)
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
    let status = Command::new("git")
        .current_dir(workdir)
        .arg("commit")
        .status()?;

    if !status.success() {
        bail!("Git commit failed (editor aborted or empty message).");
    }

    Ok(())
}
