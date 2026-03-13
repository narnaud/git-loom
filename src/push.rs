use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use git2::{BranchType, Repository, Sort};

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
    let remote_name = resolve_push_remote(&repo, &workdir, &info.upstream.label, &remote_type);

    let target_branch = extract_target_branch(&info.upstream.label);

    if no_pr {
        return match remote_type {
            RemoteType::Gerrit { .. } => push_gerrit_no_pr(&workdir, &remote_name, &branch_name),
            _ => push_plain(&workdir, &remote_name, &branch_name),
        };
    }

    let base_oid = info.upstream.merge_base_oid;

    match remote_type {
        RemoteType::Plain => push_plain(&workdir, &remote_name, &branch_name),
        RemoteType::GitHub => push_github(
            &repo,
            &workdir,
            &remote_name,
            &branch_name,
            &target_branch,
            base_oid,
            &info.upstream.label,
        ),
        RemoteType::AzureDevOps => push_azure(
            &repo,
            &workdir,
            &remote_name,
            &branch_name,
            &target_branch,
            base_oid,
        ),
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
    // Use repo.commondir() so this works in worktrees (where hooks are shared)
    {
        let hook_path = repo.commondir().join("hooks").join("commit-msg");
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
/// Handles SCP-style SSH URLs (with or without `git@` prefix) and HTTPS URLs:
/// - `git@github.com:owner/repo.git`
/// - `git@github-alias:owner/repo.git`
/// - `github-work:owner/repo` (bare alias, no `git@`)
/// - `https://github.com/owner/repo.git`
///
/// Returns `None` if the remote doesn't exist or the URL can't be parsed.
fn extract_gh_repo(repo: &Repository, remote: &str) -> Option<String> {
    let remote = repo.find_remote(remote).ok()?;
    let url = remote.url()?;

    // SCP-style SSH URLs: [git@]<hostname>:owner/repo[.git]
    // Covers git@github.com:owner/repo.git, git@github-alias:owner/repo.git,
    // and bare aliases like github-work:owner/repo (no git@ prefix).
    // Distinguish from URLs by requiring no '://' and no '/' before the ':'.
    let scp_url = url.strip_prefix("git@").unwrap_or(url);
    if !scp_url.contains("://")
        && let Some(colon_idx) = scp_url.find(':')
    {
        let host = &scp_url[..colon_idx];
        if !host.contains('/') {
            let path = &scp_url[colon_idx + 1..];
            return Some(path.trim_end_matches(".git").to_string());
        }
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
/// Priority:
/// 1. `git config loom.push-remote` — explicit override
/// 2. GitHub fork convention — if integration remote is `upstream` and `origin` exists, use `origin`
/// 3. Integration branch's remote — fallback
///
/// For non-standard fork setups (e.g., integration tracks `origin`, fork is `personal`),
/// set `git config loom.push-remote personal`.
fn resolve_push_remote(
    repo: &Repository,
    workdir: &Path,
    upstream_label: &str,
    remote_type: &RemoteType,
) -> String {
    // 1. Check explicit config override
    if let Ok(push_remote) =
        git_commands::run_git_stdout(workdir, &["config", "--get", "loom.push-remote"])
    {
        let remote = push_remote.trim();
        if !remote.is_empty() && repo.find_remote(remote).is_ok() {
            return remote.to_string();
        }
    }

    // 2. GitHub fork workflow: if upstream is "upstream" and origin exists, push to origin
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

/// Collect commits for a branch from oldest to newest, skipping merge commits.
///
/// Returns `(subject, body)` pairs where `body` is everything after the first
/// line of the commit message (may be empty).
fn gather_branch_commits(
    repo: &Repository,
    branch_name: &str,
    base_oid: git2::Oid,
) -> Result<Vec<(String, String)>> {
    let branch = repo.find_branch(branch_name, BranchType::Local)?;
    let tip_oid = branch
        .get()
        .target()
        .context("Branch does not point to a commit")?;

    let mut revwalk = repo.revwalk()?;
    revwalk.push(tip_oid)?;
    revwalk.hide(base_oid)?;
    revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;

    let mut commits = Vec::new();
    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        if commit.parent_count() > 1 {
            continue; // skip merge commits
        }
        let subject = commit.summary().unwrap_or("").to_string();
        let body = commit.body().unwrap_or("").to_string();
        commits.push((subject, body));
    }

    Ok(commits)
}

/// Build a PR title and description from the commits in a branch.
///
/// - **Single commit**: title = commit subject, description = commit body.
/// - **Multiple commits**: prompts the user for a title, then concatenates all
///   commit messages (oldest → newest) as the description.
fn pr_title_and_description(
    repo: &Repository,
    branch_name: &str,
    base_oid: git2::Oid,
) -> Result<(String, String)> {
    let commits = gather_branch_commits(repo, branch_name, base_oid)?;

    if commits.is_empty() {
        return Ok((branch_name.to_string(), String::new()));
    }

    if commits.len() == 1 {
        let (subject, body) = &commits[0];
        return Ok((subject.clone(), body.clone()));
    }

    // Multiple commits: ask the user for a title
    let title = msg::input("PR title", |s| {
        if s.is_empty() {
            Err("Title cannot be empty")
        } else {
            Ok(())
        }
    })?;

    let description = commits
        .iter()
        .map(|(subject, body)| {
            if body.is_empty() {
                subject.clone()
            } else {
                format!("{}\n\n{}", subject, body)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    Ok((title, description))
}

/// Push using plain git with force-with-lease.
fn push_plain(workdir: &Path, remote: &str, branch: &str) -> Result<()> {
    git_push(workdir, remote, branch)?;
    msg::success(&format!("Pushed `{}` to `{}`", branch, remote));
    Ok(())
}

/// Push to GitHub: push the branch, then open `gh pr create --web`.
///
/// Supports fork workflow where the integration branch tracks the upstream
/// repository and the branch is pushed to a fork remote. The PR is created
/// against the integration branch's remote (usually the upstream/main repo)
/// with the head pointing to the push remote.
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
    base_oid: git2::Oid,
    upstream_label: &str,
) -> Result<()> {
    git_push(workdir, remote, branch)?;

    // Skip PR creation when pushing the upstream target branch itself
    if branch == target_branch {
        msg::success(&format!("Pushed `{}` to `{}`", branch, remote));
        return Ok(());
    }

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

    // Determine PR target repo and head:
    // - For fork workflow: upstream branch's remote is the target (base of PR),
    //   push remote is the head (where the branch is pushed).
    // - For non-fork: both are the same.
    let integration_remote = extract_remote_name(upstream_label);
    let (pr_target_remote, pr_target_repo) = extract_gh_repo(repo, &integration_remote)
        .map(|r| (integration_remote.as_str(), r))
        .or_else(|| {
            // Fallback: try to extract from push remote if integration remote doesn't exist
            extract_gh_repo(repo, remote).map(|r| (remote, r))
        })
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
    if let Some(pr_url) = find_existing_github_pr(workdir, &pr_target_repo, branch) {
        msg::success(&format!("PR updated: {}", pr_url));
        return Ok(());
    }

    let (title, body) = pr_title_and_description(repo, branch, base_oid)?;

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
        &pr_target_repo,
        "--title",
        &title,
        "--body",
        &body,
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

    let prs: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    prs.get(0)?.get("url")?.as_str().map(str::to_string)
}

/// Build a `Command` for the Azure CLI.
///
/// On Windows `az` is a `.cmd` batch script which `CreateProcess` cannot
/// resolve directly, so we run it through `cmd /C`.
fn az_command() -> Command {
    if cfg!(windows) {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "az"]);
        cmd
    } else {
        Command::new("az")
    }
}

/// Push to Azure DevOps: push the branch, then open `az repos pr create --open`.
///
/// If a PR already exists for the branch, prints the PR URL instead of
/// opening the browser.
fn push_azure(
    repo: &Repository,
    workdir: &Path,
    remote: &str,
    branch: &str,
    target_branch: &str,
    base_oid: git2::Oid,
) -> Result<()> {
    git_push(workdir, remote, branch)?;

    let start = Instant::now();
    let az_check = az_command().arg("--version").output();
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

    let (title, description) = pr_title_and_description(repo, branch, base_oid)?;

    let mut args: Vec<&str> = vec![
        "repos",
        "pr",
        "create",
        "--open",
        "--source-branch",
        branch,
        "--target-branch",
        target_branch,
        "--detect",
        "--title",
        &title,
    ];

    // az CLI accepts multiple values after --description, one per line.
    // This avoids passing a single multiline argument which breaks
    // through cmd /C on Windows.
    let desc_lines: Vec<&str> = description.lines().collect();
    if !desc_lines.is_empty() {
        args.push("--description");
        for line in &desc_lines {
            args.push(line);
        }
    }

    let start = Instant::now();
    let output = az_command().current_dir(workdir).args(&args).output()?;
    let duration_ms = start.elapsed().as_millis();
    loom_trace::log_command(
        "az",
        &args.join(" "),
        duration_ms,
        output.status.success(),
        "",
    );

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr.trim());
    }

    Ok(())
}

/// Check if an Azure DevOps PR already exists for the given source branch.
///
/// Returns the PR web URL if found, or `None` if no PR exists or the check fails.
fn find_existing_azure_pr(workdir: &Path, branch: &str) -> Option<String> {
    let output = az_command()
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

    let prs: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let pr = prs.get(0)?;

    // Construct the web URL from the structured fields, since the API URL
    // uses GUIDs and does not contain the browser-accessible "/pullrequest/" path.
    let repo_url = pr["repository"]["url"].as_str()?;
    let org = repo_url
        .strip_prefix("https://dev.azure.com/")?
        .split('/')
        .next()?;
    let project = pr["repository"]["project"]["name"].as_str()?;
    let repo = pr["repository"]["name"].as_str()?;
    let pr_id = pr["pullRequestId"].as_u64()?;

    Some(format!(
        "https://dev.azure.com/{org}/{project}/_git/{repo}/pullrequest/{pr_id}"
    ))
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
///
/// Captures stderr from the push command and extracts Gerrit review URLs
/// (lines starting with `remote:` that contain `http://` or `https://`).
fn push_gerrit(workdir: &Path, remote: &str, branch: &str, target_branch: &str) -> Result<()> {
    let refspec = format!("{}:refs/for/{}", branch, target_branch);
    let topic_opt = format!("topic={}", branch);

    let args = ["push", "-o", &topic_opt, remote, &refspec];
    let start = Instant::now();
    let output = Command::new("git")
        .current_dir(workdir)
        .args(args)
        .output()?;

    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let cmd = args.join(" ");
    loom_trace::log_command("git", &cmd, duration_ms, output.status.success(), &stderr);

    if !output.status.success() {
        bail!("Git {} failed", cmd);
    }

    // Extract Gerrit review URLs from remote output
    let mut message = format!(
        "Pushed `{}` to `{}` (Gerrit: `refs/for/{}`)",
        branch, remote, target_branch
    );
    for line in stderr.lines() {
        if let Some(rest) = line.strip_prefix("remote:") {
            let trimmed = rest.trim();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                message.push('\n');
                if trimmed.ends_with(']') {
                    if let Some(pos) = trimmed.rfind('[') {
                        let (before, tag) = trimmed.split_at(pos);
                        message.push_str(&format!("{}`{}`", before, tag));
                    } else {
                        message.push_str(trimmed);
                    }
                } else {
                    message.push_str(trimmed);
                }
            }
        }
    }
    msg::success(&message);

    Ok(())
}

#[cfg(test)]
#[path = "push_test.rs"]
mod tests;
