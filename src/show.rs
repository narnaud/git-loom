use anyhow::Result;

use crate::core::repo::{self, Target};
use crate::git;

/// Show the diff and metadata for a commit (like `git show`), using short IDs.
pub fn run(target: String) -> Result<()> {
    let repo = repo::open_repo()?;
    let resolved = repo::resolve_arg(
        &repo,
        &target,
        &[repo::TargetKind::Commit, repo::TargetKind::Branch],
    )?;

    let git_ref = match resolved {
        Target::Commit(hash) => hash,
        Target::Branch(name) => name,
        _ => unreachable!(),
    };

    let workdir = repo::require_workdir(&repo, "show")?;
    git::run_git_interactive(workdir, &["show", &git_ref])
}

#[cfg(test)]
#[path = "show_test.rs"]
mod tests;
