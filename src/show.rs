use anyhow::{Result, bail};

use crate::git::{self, Target};
use crate::git_commands;

/// Show the diff and metadata for a commit (like `git show`), using short IDs.
pub fn run(target: String) -> Result<()> {
    let repo = git::open_repo()?;
    let resolved = git::resolve_target(&repo, &target)?;

    let hash = match resolved {
        Target::Commit(hash) => hash,
        Target::Branch(name) => {
            let branch = repo.find_branch(&name, git2::BranchType::Local)?;
            branch.get().peel_to_commit()?.id().to_string()
        }
        Target::File(_) => bail!("Cannot show a file\nRun `git-loom status` to see available IDs"),
        Target::Unstaged => bail!("Cannot show unstaged changes"),
        Target::CommitFile { .. } => bail!("Cannot show a commit file reference"),
    };

    let workdir = git::require_workdir(&repo, "show")?;
    git_commands::run_git_interactive(workdir, &["show", &hash])
}

#[cfg(test)]
#[path = "show_test.rs"]
mod tests;
