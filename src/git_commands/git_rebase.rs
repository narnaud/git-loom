use std::path::Path;

/// Continue an in-progress rebase.
/// If continuation fails, automatically aborts the rebase.
pub fn continue_rebase(workdir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = super::run_git(workdir, &["rebase", "--continue"]) {
        let _ = abort(workdir);
        return Err(format!("Git rebase --continue failed. Rebase aborted:\n{}", e).into());
    }
    Ok(())
}

/// Rebase commits between `upstream` and HEAD onto `newbase`.
///
/// Runs `git rebase --onto <newbase> <upstream> --update-refs`.
/// The `--update-refs` flag keeps any branch refs in the rebased range up to date.
#[cfg(test)]
pub fn rebase_onto(
    workdir: &Path,
    newbase: &str,
    upstream: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    super::run_git(
        workdir,
        &[
            "rebase",
            "--onto",
            newbase,
            upstream,
            "--autostash",
            "--update-refs",
        ],
    )
}

/// Abort an in-progress rebase.
pub fn abort(workdir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    super::run_git(workdir, &["rebase", "--abort"])
}
