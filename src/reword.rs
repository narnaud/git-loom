use git2::Repository;
use std::process::Command;

/// Reword a commit message or rename a branch.
pub fn run(target: String, message: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let repo = Repository::discover(cwd)?;

    let resolved = crate::git::resolve_target(&repo, &target)?;

    match resolved {
        crate::git::Target::Commit(hash) => reword_commit(&repo, &hash, message),
        crate::git::Target::Branch(name) => {
            let new_name = message.ok_or("Branch renaming requires -m flag with new name")?;
            reword_branch(&repo, &name, &new_name)
        }
        crate::git::Target::File(_) => {
            Err("Cannot reword a file. Use 'git add' to stage file changes.".into())
        }
    }
}

/// Reword a commit message using git's native rebase commands.
///
/// Approach:
/// 1. git rebase --interactive --autostash --keep-empty --no-autosquash --rebase-merges [--root | <hash>^]
/// 2. git commit --allow-empty --amend --only [-m "message"]
/// 3. git rebase --continue
fn reword_commit(
    repo: &Repository,
    commit_hash: &str,
    message: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = repo
        .workdir()
        .ok_or("Cannot reword in bare repository")?;

    // Check if this is a root commit (has no parent)
    let commit = repo.revparse_single(commit_hash)?.peel_to_commit()?;
    let is_root = commit.parent_count() == 0;

    // Step 1: Start interactive rebase
    let short_hash = &commit_hash[..7.min(commit_hash.len())];

    // Use git-loom itself as the sequence editor to replace 'pick' with 'edit'
    let self_exe = loom_exe_path()?;
    let sequence_editor = format!(
        "\"{}\" internal-sequence-edit {}",
        self_exe.display(),
        short_hash
    );

    let mut rebase_cmd = Command::new("git");
    rebase_cmd
        .current_dir(workdir)
        .args([
            "rebase",
            "--interactive",
            "--autostash",
            "--keep-empty",
            "--no-autosquash",
            "--rebase-merges",
        ])
        .env("GIT_SEQUENCE_EDITOR", sequence_editor);

    // For root commits, use --root; otherwise rebase to parent
    if is_root {
        rebase_cmd.arg("--root");
    } else {
        rebase_cmd.arg(format!("{}^", commit_hash));
    }

    let status = rebase_cmd.status()?;
    if !status.success() {
        return Err("Git rebase failed to start".into());
    }

    // Step 2: Amend the commit message
    let mut amend_cmd = Command::new("git");
    amend_cmd
        .current_dir(workdir)
        .args(["commit", "--allow-empty", "--amend", "--only"]);

    if let Some(msg) = message {
        amend_cmd.args(["-m", &msg]);
    }

    let status = amend_cmd.status()?;
    if !status.success() {
        // Abort the rebase on failure
        let _ = Command::new("git")
            .current_dir(workdir)
            .args(["rebase", "--abort"])
            .status();
        return Err("Git commit --amend failed".into());
    }

    // Step 3: Continue the rebase
    let status = Command::new("git")
        .current_dir(workdir)
        .args(["rebase", "--continue"])
        .status()?;

    if !status.success() {
        // Abort the rebase on failure for consistency
        let _ = Command::new("git")
            .current_dir(workdir)
            .args(["rebase", "--abort"])
            .status();
        return Err("Git rebase --continue failed. Rebase aborted.".into());
    }

    Ok(())
}

/// Rename a branch using git branch -m.
fn reword_branch(
    repo: &Repository,
    old_name: &str,
    new_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let workdir = repo
        .workdir()
        .ok_or("Cannot rename branch in bare repository")?;

    let output = Command::new("git")
        .current_dir(workdir)
        .args(["branch", "-m", old_name, new_name])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to rename branch: {}", stderr).into());
    }

    println!("Renamed branch '{}' to '{}'", old_name, new_name);
    Ok(())
}

/// Resolve the path to the git-loom binary.
///
/// During `cargo test`, `current_exe()` returns the test harness binary in
/// `target/<profile>/deps/`. The actual git-loom binary lives one level up
/// in `target/<profile>/`. This function detects that case and returns the
/// correct path.
fn loom_exe_path() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    if let Some(parent) = exe.parent()
        && parent.file_name().and_then(|n| n.to_str()) == Some("deps")
    {
        let bin_name = if cfg!(windows) {
            "git-loom.exe"
        } else {
            "git-loom"
        };
        if let Some(profile_dir) = parent.parent() {
            let actual = profile_dir.join(bin_name);
            if actual.exists() {
                return Ok(actual);
            }
        }
    }
    Ok(exe)
}

#[cfg(test)]
#[path = "reword_test.rs"]
mod tests;
