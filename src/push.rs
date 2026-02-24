use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};
use git2::Repository;

use crate::git::{self, Target};
use crate::git_commands;
use crate::msg;

/// Remote type detected for the push operation.
#[derive(Debug, PartialEq, Eq)]
enum RemoteType {
    Plain,
    GitHub,
    Gerrit { target_branch: String },
}

/// Push a feature branch to remote.
///
/// Detects the remote type (plain, GitHub, Gerrit) and dispatches to the
/// appropriate push strategy. Accepts an optional branch argument (name or
/// shortID); if omitted, shows an interactive picker.
pub fn run(branch: Option<String>) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "push")?.to_path_buf();
    let info = git::gather_repo_info(&repo, false)?;

    if info.branches.is_empty() {
        bail!("No woven branches to push. Create a branch with 'git loom branch' first.");
    }

    let branch_name = match branch {
        Some(b) => resolve_branch(&repo, &b)?,
        None => pick_branch(&repo)?,
    };

    let remote_type = detect_remote_type(&repo, &workdir, &info.upstream.label)?;
    let remote_name = resolve_push_remote(&repo, &info.upstream.label, &remote_type);

    match remote_type {
        RemoteType::Plain => push_plain(&workdir, &remote_name, &branch_name),
        RemoteType::GitHub => push_github(&workdir, &remote_name, &branch_name),
        RemoteType::Gerrit { target_branch } => {
            push_gerrit(&workdir, &remote_name, &branch_name, &target_branch)
        }
    }
}

/// Resolve an explicit branch argument to a woven branch name.
fn resolve_branch(repo: &Repository, branch_arg: &str) -> Result<String> {
    let info = git::gather_repo_info(repo, false)?;

    match git::resolve_target(repo, branch_arg) {
        Ok(Target::Branch(name)) => {
            if info.branches.iter().any(|b| b.name == name) {
                Ok(name)
            } else {
                bail!(
                    "Branch '{}' is not woven into the integration branch.",
                    name
                )
            }
        }
        Ok(Target::Commit(_)) => bail!("Target must be a branch, not a commit."),
        Ok(Target::File(_)) => bail!("Target must be a branch, not a file."),
        Ok(Target::Unstaged) => bail!("Target must be a branch."),
        Ok(Target::CommitFile { .. }) => bail!("Target must be a branch, not a commit file."),
        Err(e) => Err(e),
    }
}

/// Interactive branch picker: list woven branches.
fn pick_branch(repo: &Repository) -> Result<String> {
    let info = git::gather_repo_info(repo, false)?;

    let mut select = cliclack::select("Select branch to push");
    for branch in &info.branches {
        select = select.item(branch.name.clone(), &branch.name, "");
    }

    let selection: String = select.interact()?;
    Ok(selection)
}

/// Detect the remote type from config, URL heuristics, or hook inspection.
///
/// Priority: git config `loom.remote-type` → URL contains `github.com` →
/// `.git/hooks/commit-msg` contains "gerrit" → Plain fallback.
fn detect_remote_type(
    repo: &Repository,
    workdir: &Path,
    upstream_label: &str,
) -> Result<RemoteType> {
    // 1. Check explicit config override
    if let Ok(config_value) =
        git_commands::run_git_stdout(workdir, &["config", "--get", "loom.remote-type"])
    {
        let value = config_value.trim().to_lowercase();
        if value == "github" {
            return Ok(RemoteType::GitHub);
        }
        if value == "gerrit" {
            let target_branch = extract_target_branch(upstream_label);
            return Ok(RemoteType::Gerrit { target_branch });
        }
    }

    // 2. Check remote URL for github.com
    let remote_name = extract_remote_name(upstream_label);
    if let Ok(remote) = repo.find_remote(&remote_name)
        && let Some(url) = remote.url()
        && url.contains("github.com")
    {
        return Ok(RemoteType::GitHub);
    }

    // 3. Check for Gerrit commit-msg hook
    if let Some(git_dir) = workdir.join(".git").is_dir().then(|| workdir.join(".git")) {
        let hook_path = git_dir.join("hooks").join("commit-msg");
        if let Ok(content) = std::fs::read_to_string(&hook_path)
            && content.to_lowercase().contains("gerrit")
        {
            let target_branch = extract_target_branch(upstream_label);
            return Ok(RemoteType::Gerrit { target_branch });
        }
    }

    // 4. Default to plain
    Ok(RemoteType::Plain)
}

/// Extract the remote name from an upstream label like "origin/main" → "origin".
fn extract_remote_name(upstream_label: &str) -> String {
    upstream_label
        .split('/')
        .next()
        .unwrap_or("origin")
        .to_string()
}

/// Extract the target branch from an upstream label like "origin/main" → "main".
fn extract_target_branch(upstream_label: &str) -> String {
    upstream_label
        .split_once('/')
        .map(|x| x.1)
        .unwrap_or("main")
        .to_string()
}

/// Determine the push remote for the given upstream label and remote type.
///
/// In the GitHub fork workflow, the integration branch tracks `upstream/main`
/// but feature branches should be pushed to `origin` (the user's fork) so
/// they can open a PR from the fork to the original repository.
fn resolve_push_remote(
    repo: &Repository,
    upstream_label: &str,
    remote_type: &RemoteType,
) -> String {
    let remote_name = extract_remote_name(upstream_label);
    if *remote_type == RemoteType::GitHub
        && remote_name == "upstream"
        && repo.find_remote("origin").is_ok()
    {
        "origin".to_string()
    } else {
        remote_name
    }
}

/// Push using plain git with force-with-lease.
fn push_plain(workdir: &Path, remote: &str, branch: &str) -> Result<()> {
    git_commands::run_git(
        workdir,
        &[
            "push",
            "--force-with-lease",
            "--force-if-includes",
            "-u",
            remote,
            branch,
        ],
    )?;

    msg::success(&format!("Pushed '{}' to {}", branch, remote));
    Ok(())
}

/// Push to GitHub: push the branch, then open `gh pr create --web`.
fn push_github(workdir: &Path, remote: &str, branch: &str) -> Result<()> {
    git_commands::run_git(workdir, &["push", "-u", remote, branch])?;

    msg::success(&format!("Pushed '{}' to {}", branch, remote));

    // Check if gh CLI is available
    let gh_available = Command::new("gh")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());

    if !gh_available {
        println!("Install 'gh' CLI to create pull requests: https://cli.github.com");
        return Ok(());
    }

    // Open PR creation in browser (inherits stdio so browser opens)
    let status = Command::new("gh")
        .current_dir(workdir)
        .args(["pr", "create", "--web", "--head", branch])
        .status()?;

    if !status.success() {
        // gh prints its own messages (e.g., PR already exists) — not fatal
    }

    Ok(())
}

/// Push to Gerrit with topic and refs/for/ refspec.
fn push_gerrit(workdir: &Path, remote: &str, branch: &str, target_branch: &str) -> Result<()> {
    let refspec = format!("{}:refs/for/{}", branch, target_branch);
    let topic_opt = format!("topic={}", branch);

    git_commands::run_git(workdir, &["push", "-o", &topic_opt, remote, &refspec])?;

    msg::success(&format!(
        "Pushed '{}' to {} (Gerrit: refs/for/{})",
        branch, remote, target_branch
    ));
    Ok(())
}

#[cfg(test)]
#[path = "push_test.rs"]
mod tests;
