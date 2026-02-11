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
