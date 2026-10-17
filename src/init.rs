use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Result, bail};
use git2::Repository;

use crate::core::agent_mode;
use crate::core::msg;
use crate::core::remote;
use crate::core::repo;
use crate::git::{self, git_worktree};

/// Initialize a new integration branch tracking a remote upstream.
///
/// Creates the branch at the upstream tip and switches to it; the default name
/// depends on the worktree (see [`default_name`]).
pub fn run(name: Option<String>) -> Result<()> {
    run_with(name, remote::github_fork_parent)
}

/// [`run`] with the GitHub fork lookup given, so tests never reach `gh`.
fn run_with(
    name: Option<String>,
    fork_lookup: impl Fn(&Path, &str) -> Option<(String, String)>,
) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "initialize")?;
    let main = main_worktree(&repo, workdir)?;

    let name = match name {
        Some(name) => name,
        None => default_name(workdir, main.as_ref()),
    };
    let name = name.trim().to_string();
    if name.is_empty() {
        bail!("Branch name cannot be empty");
    }

    git::branch_validate_name(workdir, &name)?;

    repo::ensure_branch_not_exists(&repo, &name)?;

    if let Some(fork) = fork_of_origin(&repo, workdir, fork_lookup) {
        offer_fork_upstream(workdir, &fork)?;
    }

    let upstream = repo::detect_upstream(&repo, main.as_ref().and_then(|m| m.branch.as_deref()))?;

    // `git switch` refuses to overwrite local changes; an upstream far ahead of
    // HEAD, such as a fork's parent, makes that likely.
    git::branch_switch_create_tracking(workdir, &name, &upstream).map_err(|e| {
        e.context(format!(
            "Could not create `{}` at `{}`\n\
             If local changes conflict with it, commit or stash them, then re-run `loom init`",
            name, upstream
        ))
    })?;

    msg::success(&format!(
        "Initialized integration branch `{}` tracking `{}`",
        name, upstream
    ));

    Ok(())
}

/// The GitHub repository `origin` is a fork of.
#[derive(Debug, PartialEq)]
struct Fork {
    /// `owner/repo`.
    parent: String,
    default_branch: String,
    /// `origin`'s URL with its `owner/repo` swapped for the parent's, so the
    /// protocol and credentials stay the user's.
    url: String,
}

/// The repository a github.com `origin` forks, when no remote names it yet:
/// none is called `upstream` (which [`repo::detect_upstream`] would track) and none
/// points at it under another name.
fn fork_of_origin(
    repo: &Repository,
    workdir: &Path,
    lookup: impl Fn(&Path, &str) -> Option<(String, String)>,
) -> Option<Fork> {
    if repo.find_remote("upstream").is_ok() || !remote::on_github_com(repo, "origin") {
        return None;
    }
    let origin_repo = remote::extract_gh_repo(repo, "origin")?;
    let (parent, default_branch) = lookup(workdir, &origin_repo)?;
    let remotes = repo.remotes().ok()?;
    // `extract_gh_repo` reads `owner/repo` off an scp-style URL on any host.
    if remotes.iter().flatten().flatten().any(|name| {
        remote::on_github_com(repo, name)
            && remote::extract_gh_repo(repo, name).is_some_and(|r| r.eq_ignore_ascii_case(&parent))
    }) {
        return None;
    }
    let origin_url = repo.find_remote("origin").ok()?.url().ok()?.to_string();
    let at = origin_url.rfind(&origin_repo)?;
    let url = format!(
        "{}{}{}",
        &origin_url[..at],
        parent,
        &origin_url[at + origin_repo.len()..]
    );
    Some(Fork {
        parent,
        default_branch,
        url,
    })
}

/// Offer to add `fork` as remote `upstream`, which [`repo::detect_upstream`] then
/// tracks. Where nobody can answer (agent mode, no terminal) the offer is a
/// note instead: it is optional, so it must not stop the `init`.
fn offer_fork_upstream(workdir: &Path, fork: &Fork) -> Result<()> {
    let shown_url = crate::core::trace_log::without_credentials(&fork.url);
    if agent_mode::enabled() || !std::io::stdin().is_terminal() {
        msg::warn(&format!(
            "`origin` is a fork of `{}`, which this branch does not track\n\
             To track it: `git remote add -f upstream {}`, `git branch -u upstream/{}`, then `loom update`",
            fork.parent, shown_url, fork.default_branch
        ));
        return Ok(());
    }
    let question = format!(
        "`origin` is a fork of `{}`: track it as remote `upstream`?\n\
         Adds `upstream` ({}) and fetches it",
        fork.parent, shown_url
    );
    if msg::confirm(&question, "")? {
        add_upstream(workdir, fork)?;
    }
    Ok(())
}

/// Fetch `fork` into `refs/remotes/upstream/`, then add it as remote
/// `upstream`. Fetching first means a failed or interrupted fetch leaves no
/// `upstream` behind: one without branches would stop the next `init` from
/// offering it, and leave nothing to track.
fn add_upstream(workdir: &Path, fork: &Fork) -> Result<()> {
    let spinner = msg::spinner();
    spinner.start(&format!("Fetching `{}`...", fork.parent));
    let refspec = "+refs/heads/*:refs/remotes/upstream/*";
    if git::run_git(workdir, &["fetch", "--no-progress", &fork.url, refspec]).is_err() {
        spinner.error("Fetch failed");
        bail!(
            "Could not fetch `{}`\n\
             Re-run `loom init` once it can be fetched, or decline to keep tracking `origin`",
            fork.parent
        );
    }
    if let Err(e) = git::run_git(workdir, &["remote", "add", "upstream", &fork.url]) {
        spinner.error("Could not add remote `upstream`");
        return Err(e);
    }
    // Offline, from what gh reported; on failure `try_github_upstream` falls
    // back to the usual default branch names.
    let _ = git::run_git(
        workdir,
        &["remote", "set-head", "upstream", &fork.default_branch],
    );
    // What `gh repo clone` sets on a fork: gh then targets the parent.
    let _ = git::run_git(workdir, &["config", "remote.upstream.gh-resolved", "base"]);
    spinner.stop(&format!("Added remote `upstream` for `{}`", fork.parent));
    Ok(())
}

/// The main worktree, when `workdir` is a linked one.
fn main_worktree(repo: &Repository, workdir: &Path) -> Result<Option<git::Worktree>> {
    if !repo.is_worktree() {
        return Ok(None);
    }
    Ok(git::list_worktrees(workdir)?.into_iter().next())
}

/// `integration` in the main worktree; `integration-<name>` in a linked one,
/// where `<name>` is the directory name minus the main worktree's name and a
/// dash (`<dir>-<name>`, as `worktree new` lays it out), else the whole
/// directory name.
fn default_name(workdir: &Path, main: Option<&git::Worktree>) -> String {
    let Some(main) = main else {
        return "integration".to_string();
    };
    let name = git_worktree::name_of(
        &git_worktree::dir_name(workdir),
        &git_worktree::dir_name(&main.path),
    );
    format!("integration-{}", name)
}

#[cfg(test)]
#[path = "init_test.rs"]
mod tests;
