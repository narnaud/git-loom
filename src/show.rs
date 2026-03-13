use anyhow::Result;

use crate::git::{self, Target};
use crate::git_commands;

/// Show the diff and metadata for a commit (like `git show`), using short IDs.
pub fn run(target: String) -> Result<()> {
    let repo = git::open_repo()?;
    let resolved = git::resolve_arg(
        &repo,
        &target,
        &[git::TargetKind::Commit, git::TargetKind::Branch],
    )?;

    let git_ref = match resolved {
        Target::Commit(hash) => hash,
        Target::Branch(name) => name,
        _ => unreachable!(),
    };

    let workdir = git::require_workdir(&repo, "show")?;
    git_commands::run_git_interactive(workdir, &["show", &git_ref])
}

#[cfg(test)]
#[path = "show_test.rs"]
mod tests;
