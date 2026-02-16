use std::path::Path;
use std::process::Command;

/// Amend the current commit, optionally replacing its message.
///
/// Wraps `git commit --allow-empty --amend --only [-m msg]`.
/// Uses `--only` so that staged changes are not accidentally included.
pub fn amend(workdir: &Path, message: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::new("git");
    cmd.current_dir(workdir)
        .args(["commit", "--allow-empty", "--amend", "--only"]);

    if let Some(msg) = message {
        cmd.args(["-m", msg]);
    }

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git commit --amend failed:\n{}", stderr).into());
    }

    Ok(())
}

/// Amend the current commit, keeping its message and including staged changes.
///
/// Wraps `git commit --amend --no-edit`.
/// Unlike `amend()`, this does NOT use `--only`, so staged changes are included.
pub fn amend_no_edit(workdir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .current_dir(workdir)
        .args(["commit", "--amend", "--no-edit"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git commit --amend --no-edit failed:\n{}", stderr).into());
    }

    Ok(())
}

/// Stage specific files.
///
/// Wraps `git add <files>`.
pub fn stage_files(workdir: &Path, files: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let mut args = vec!["add", "--"];
    args.extend(files);
    super::run_git(workdir, &args)
}

/// Create a commit with a message.
///
/// Wraps `git commit -m <message>`.
pub fn commit(workdir: &Path, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    super::run_git(workdir, &["commit", "-m", message])
}
