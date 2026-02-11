use std::path::Path;

use super::run_git;

/// Rename a branch using git branch -m.
///
/// Wraps `git branch -m <old_name> <new_name>`.
pub fn rename(
    workdir: &Path,
    old_name: &str,
    new_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    run_git(workdir, &["branch", "-m", old_name, new_name])
        .map_err(|e| format!("Failed to rename branch: {}", e))?;

    Ok(())
}
