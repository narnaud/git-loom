use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{Result, bail};
use git2::Repository;

use crate::git;
use crate::git_commands;
use crate::msg;
use crate::trace as loom_trace;

/// Remote type detected for the push operation.
#[derive(Debug, PartialEq, Eq)]
enum RemoteType {
    Plain,
    GitHub,
    AzureDevOps,
    Gerrit { target_branch: String },
}

/// Push a feature branch to remote.
///
/// Detects the remote type (plain, GitHub, Gerrit) and dispatches to the
/// appropriate push strategy. Accepts an optional branch argument (name or
/// shortID); if omitted, shows an interactive picker.
///
/// When `no_pr` is true, skips PR/review creation for all remote types.
/// For Gerrit, branches without a `wip/` prefix get a confirmation prompt.
pub fn run(branch: Option<String>, no_pr: bool) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "push")?.to_path_buf();
    let info = git::gather_repo_info(&repo, false, 1)?;

    if info.branches.is_empty() {
        bail!("No woven branches to push\nCreate a branch with `git loom branch` first");
    }

    let branch_name = match branch {
        Some(b) => resolve_branch(&repo, &info, &b)?,
        None => pick_branch(&info)?,
    };

    let remote_type = detect_remote_type(&repo, &workdir, &info.upstream.label)?;
    let remote_name = resolve_push_remote(&repo, &info.upstream.label, &remote_type);

    let target_branch = extract_target_branch(&info.upstream.label);

    if no_pr {
        return match remote_type {
            RemoteType::Gerrit { .. } => push_gerrit_no_pr(&workdir, &remote_name, &branch_name),
            _ => push_plain(&workdir, &remote_name, &branch_name),
        };
    }

    match remote_type {
        RemoteType::Plain => push_plain(&workdir, &remote_name, &branch_name),
        RemoteType::GitHub => {
            push_github(&repo, &workdir, &remote_name, &branch_name, &target_branch)
        }
        RemoteType::AzureDevOps => push_azure(&workdir, &remote_name, &branch_name, &target_branch),
        RemoteType::Gerrit { target_branch } => {
            push_gerrit(&workdir, &remote_name, &branch_name, &target_branch)
        }
    }
}

/// Resolve an explicit branch argument to a woven branch name.
fn resolve_branch(repo: &Repository, info: &git::RepoInfo, branch_arg: &str) -> Result<String> {
    let name = git::resolve_target(repo, branch_arg)?.expect_branch()?;
    if info.branches.iter().any(|b| b.name == name) {
        Ok(name)
    } else {
        bail!("Branch '{}' is not woven into the integration branch", name)
    }
}

/// Interactive branch picker: list woven branches.
fn pick_branch(info: &git::RepoInfo) -> Result<String> {
    let items: Vec<String> = info.branches.iter().map(|b| b.name.clone()).collect();
    msg::select("Select branch to push", items)
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
        if value == "azure" {
            return Ok(RemoteType::AzureDevOps);
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

    // 2b. Check remote URL for dev.azure.com
    if let Ok(remote) = repo.find_remote(&remote_name)
        && let Some(url) = remote.url()
        && url.contains("dev.azure.com")
    {
        return Ok(RemoteType::AzureDevOps);
    }

    // 3. Check for Gerrit commit-msg hook
    let git_dir = workdir.join(".git");
    if git_dir.is_dir() {
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

/// Extract `owner/repo` from a git remote URL for use with `gh --repo`.
///
/// Handles both SSH (`git@github.com:owner/repo.git`) and HTTPS
/// (`https://github.com/owner/repo.git`) URLs. Returns `None` if the
/// remote doesn't exist or the URL can't be parsed.
fn extract_gh_repo(repo: &Repository, remote: &str) -> Option<String> {
    let remote = repo.find_remote(remote).ok()?;
    let url = remote.url()?;

    // SSH: git@github.com:owner/repo.git
    if let Some(path) = url.strip_prefix("git@github.com:") {
        return Some(path.trim_end_matches(".git").to_string());
    }
    // HTTPS: https://github.com/owner/repo.git
    if let Some(path) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
    {
        return Some(path.trim_end_matches(".git").to_string());
    }

    None
}

/// Extract the target branch from an upstream label like "origin/main" → "main".
fn extract_target_branch(upstream_label: &str) -> String {
    let branch = git::upstream_local_branch(upstream_label);
    if branch.is_empty() {
        "main".to_string()
    } else {
        branch
    }
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

/// Force-with-lease push a branch to remote.
fn git_push(workdir: &Path, remote: &str, branch: &str) -> Result<()> {
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
    )
}

/// Push using plain git with force-with-lease.
fn push_plain(workdir: &Path, remote: &str, branch: &str) -> Result<()> {
    git_push(workdir, remote, branch)?;
    msg::success(&format!("Pushed `{}` to `{}`", branch, remote));
    Ok(())
}

/// Push to GitHub: push the branch, then open `gh pr create --web`.
///
/// If the branch being pushed is the upstream target branch itself (e.g.
/// pushing `main` when tracking `origin/main`), skip PR creation and fall
/// back to a plain force-with-lease push.
///
/// If a PR already exists for the branch, prints the PR URL instead of
/// opening the browser.
fn push_github(
    repo: &Repository,
    workdir: &Path,
    remote: &str,
    branch: &str,
    target_branch: &str,
) -> Result<()> {
    if branch == target_branch {
        return push_plain(workdir, remote, branch);
    }

    git_push(workdir, remote, branch)?;

    // Check if gh CLI is available
    let start = Instant::now();
    let gh_check = Command::new("gh").arg("--version").output();
    let gh_available = gh_check.as_ref().is_ok_and(|o| o.status.success());
    let duration_ms = start.elapsed().as_millis();
    loom_trace::log_command("gh", "--version", duration_ms, gh_available, "");

    if !gh_available {
        println!("Install 'gh' CLI to create pull requests: https://cli.github.com");
        return Ok(());
    }

    // Determine PR target repo: prefer "upstream" remote (fork workflow),
    // fall back to push remote (non-fork).
    let (pr_target_remote, gh_repo) = extract_gh_repo(repo, "upstream")
        .map(|r| ("upstream", r))
        .or_else(|| extract_gh_repo(repo, remote).map(|r| (remote, r)))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not determine target repository for PR creation\n\
                 Run `gh repo set-default` to select a default remote repository"
            )
        })?;

    let is_fork = remote != pr_target_remote;

    // In fork workflow, --head needs "fork-owner:branch" prefix
    let head_arg = if is_fork {
        extract_gh_repo(repo, remote)
            .and_then(|r| r.split('/').next().map(|s| format!("{}:{}", s, branch)))
            .unwrap_or_else(|| branch.to_string())
    } else {
        branch.to_string()
    };

    // If a PR already exists, show its URL instead of opening the browser
    if let Some(pr_url) = find_existing_github_pr(workdir, &gh_repo, &head_arg) {
        msg::success(&format!("PR updated: {}", pr_url));
        return Ok(());
    }

    // Open PR creation in browser (inherits stdio so browser opens)
    let args = vec![
        "pr",
        "create",
        "--web",
        "--head",
        &head_arg,
        "--base",
        target_branch,
        "--repo",
        &gh_repo,
    ];

    let start = Instant::now();
    let status = Command::new("gh")
        .current_dir(workdir)
        .args(&args)
        .status()?;

    let duration_ms = start.elapsed().as_millis();
    loom_trace::log_command("gh", &args.join(" "), duration_ms, status.success(), "");

    if !status.success() {
        // gh prints its own messages — not fatal
    }

    Ok(())
}

/// Check if a GitHub PR already exists for the given branch.
///
/// Returns the PR URL if found, or `None` if no PR exists or the check fails.
fn find_existing_github_pr(workdir: &Path, gh_repo: &str, head_arg: &str) -> Option<String> {
    let output = Command::new("gh")
        .current_dir(workdir)
        .args([
            "pr", "list", "--head", head_arg, "--repo", gh_repo, "--json", "url", "--limit", "1",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed == "[]" {
        return None;
    }

    // Extract "url" value from JSON: [{"url":"<URL>"}]
    extract_json_string_field(trimmed, "url")
}

/// Push to Azure DevOps: push the branch, then open `az repos pr create --open`.
///
/// If a PR already exists for the branch, prints the PR URL instead of
/// opening the browser.
fn push_azure(workdir: &Path, remote: &str, branch: &str, target_branch: &str) -> Result<()> {
    git_push(workdir, remote, branch)?;

    let start = Instant::now();
    let az_check = Command::new("az").arg("--version").output();
    let az_available = az_check.as_ref().is_ok_and(|o| o.status.success());
    let duration_ms = start.elapsed().as_millis();
    loom_trace::log_command("az", "--version", duration_ms, az_available, "");

    if !az_available {
        println!(
            "Install 'az' CLI to create pull requests: \
             https://learn.microsoft.com/cli/azure/install-azure-cli"
        );
        return Ok(());
    }

    // If a PR already exists, show its URL instead of opening the browser
    if let Some(pr_url) = find_existing_azure_pr(workdir, branch) {
        msg::success(&format!("PR updated: {}", pr_url));
        return Ok(());
    }

    let args = vec![
        "repos",
        "pr",
        "create",
        "--open",
        "--source-branch",
        branch,
        "--target-branch",
        target_branch,
        "--detect",
    ];

    let start = Instant::now();
    let status = Command::new("az")
        .current_dir(workdir)
        .args(&args)
        .status()?;
    let duration_ms = start.elapsed().as_millis();
    loom_trace::log_command("az", &args.join(" "), duration_ms, status.success(), "");

    if !status.success() {
        // az prints its own messages — not fatal
    }

    Ok(())
}

/// Check if an Azure DevOps PR already exists for the given source branch.
///
/// Returns the PR web URL if found, or `None` if no PR exists or the check fails.
fn find_existing_azure_pr(workdir: &Path, branch: &str) -> Option<String> {
    let output = Command::new("az")
        .current_dir(workdir)
        .args([
            "repos",
            "pr",
            "list",
            "--detect",
            "--source-branch",
            branch,
            "--output",
            "json",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed == "[]" {
        return None;
    }

    // Extract web URL: look for a URL containing "/pullrequest/" in the JSON
    // (from _links.web.href, which is the browser-accessible URL)
    extract_pullrequest_url(trimmed)
}

/// Extract a PR web URL containing "/pullrequest/" from a JSON string.
fn extract_pullrequest_url(json: &str) -> Option<String> {
    let pos = json.find("/pullrequest/")?;
    let before = &json[..pos];
    let start = before.rfind("https://")?;
    let url_start = &json[start..];
    let end = url_start.find('"').unwrap_or(url_start.len());
    Some(url_start[..end].to_string())
}

/// Extract the first occurrence of `"field":"value"` from a JSON string.
fn extract_json_string_field(json: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", field);
    let pos = json.find(&needle)?;
    let after = &json[pos + needle.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// Push to Gerrit without creating a review (plain force push).
///
/// If the branch is already prefixed with `wip/`, pushes directly.
/// Otherwise, warns the user that a Gerrit admin will be needed to delete
/// the remote branch later, and asks them to choose:
///   - Push as-is
///   - Push as `wip/<branch>` instead (no admin needed to delete)
///   - Cancel
fn push_gerrit_no_pr(workdir: &Path, remote: &str, branch: &str) -> Result<()> {
    if branch.starts_with("wip/") {
        return push_plain(workdir, remote, branch);
    }

    let opt_as_is = format!("Push as `{}` (admin required to delete it later)", branch);
    let opt_wip = format!("Push as `wip/{}` instead", branch);
    let opt_cancel = "Cancel".to_string();

    let choice = msg::select(
        &format!(
            "Branch `{}` is not prefixed with `wip/` — a Gerrit admin will be needed to delete the remote branch later",
            branch
        ),
        vec![opt_as_is.clone(), opt_wip.clone(), opt_cancel],
    )?;

    if choice == opt_as_is {
        push_plain(workdir, remote, branch)
    } else if choice == opt_wip {
        let wip_name = format!("wip/{}", branch);
        let refspec = format!("{}:{}", branch, wip_name);
        git_commands::run_git(
            workdir,
            &[
                "push",
                "--force-with-lease",
                "--force-if-includes",
                remote,
                &refspec,
            ],
        )?;
        msg::success(&format!(
            "Pushed `{}` to `{}` as `{}`",
            branch, remote, wip_name
        ));
        Ok(())
    } else {
        bail!("Push cancelled")
    }
}

/// Push to Gerrit with topic and refs/for/ refspec.
fn push_gerrit(workdir: &Path, remote: &str, branch: &str, target_branch: &str) -> Result<()> {
    let refspec = format!("{}:refs/for/{}", branch, target_branch);
    let topic_opt = format!("topic={}", branch);

    git_commands::run_git(workdir, &["push", "-o", &topic_opt, remote, &refspec])?;

    msg::success(&format!(
        "Pushed `{}` to `{}` (Gerrit: `refs/for/{}`)",
        branch, remote, target_branch
    ));
    Ok(())
}

#[cfg(test)]
#[path = "push_test.rs"]
mod tests;
