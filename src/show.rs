use anyhow::Result;

use crate::core::repo::{self, Target};
use crate::git;

/// Show the diff and metadata for a commit (like `git show`), using short IDs.
///
/// With no target, shows the last commit on the current branch (like `git show`).
pub fn run(target: Option<String>) -> Result<()> {
    let repo = repo::open_repo()?;

    let git_ref = match target {
        None => "HEAD".to_string(),
        Some(target) => {
            let resolved = repo::resolve_arg(
                &repo,
                &target,
                &[repo::TargetKind::Commit, repo::TargetKind::Branch],
            )?;

            match resolved {
                Target::Commit(hash) => hash,
                Target::Branch(name) => name,
                _ => unreachable!(),
            }
        }
    };

    let workdir = repo::require_workdir(&repo, "show")?;
    git::run_git_interactive(workdir, &["show", &git_ref])
}

#[cfg(test)]
#[path = "show_test.rs"]
mod tests;
