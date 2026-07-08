use anyhow::Result;

use crate::core::repo::{self, Target};
use crate::git;
use crate::status;

/// Show the diff and metadata for a commit (like `git show`), using short IDs.
///
/// With no target, shows the commit at the top of `loom status` — the tip of
/// the integration line, skipping merge commits and hidden branches.
pub fn run(target: Option<String>) -> Result<()> {
    let repo = repo::open_repo()?;

    let git_ref = match target {
        // Fall back to HEAD outside an integration branch (e.g. plain repo)
        // or when the integration line has no commits of its own.
        None => match status::top_commit(&repo) {
            Ok(Some(oid)) => oid.to_string(),
            _ => "HEAD".to_string(),
        },
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
