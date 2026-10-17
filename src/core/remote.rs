//! Remotes and the forges behind them: remote-type detection, push-remote
//! resolution, GitHub remote URLs and the `gh` CLI (Spec 011).

use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{Result, anyhow};
use git2::Repository;

use crate::core::{msg, repo, trace_log};
use crate::git;

/// Remote type detected for the push operation.
#[derive(Debug, PartialEq, Eq)]
pub enum RemoteType {
    Plain,
    GitHub,
    GitLab,
    AzureDevOps,
    Gerrit { target_branch: String },
}

impl RemoteType {
    /// The `loom.remote-type` value naming this type.
    pub fn config_value(&self) -> &'static str {
        match self {
            RemoteType::Plain => "plain",
            RemoteType::GitHub => "github",
            RemoteType::GitLab => "gitlab",
            RemoteType::AzureDevOps => "azure",
            RemoteType::Gerrit { .. } => "gerrit",
        }
    }
}

/// The values `loom.remote-type` takes, with the name the menu shows for each.
const REMOTE_TYPES: [(&str, &str); 5] = [
    ("github", "GitHub"),
    ("gitlab", "GitLab"),
    ("azure", "Azure DevOps"),
    ("gerrit", "Gerrit"),
    ("plain", "Plain Git"),
];

fn parse_remote_type(value: &str, upstream_label: &str) -> Option<RemoteType> {
    Some(match value {
        "github" => RemoteType::GitHub,
        "gitlab" => RemoteType::GitLab,
        "azure" => RemoteType::AzureDevOps,
        "gerrit" => RemoteType::Gerrit {
            target_branch: extract_target_branch(upstream_label),
        },
        "plain" => RemoteType::Plain,
        _ => return None,
    })
}

/// The remote type `loom.remote-type` names; an unknown value is warned about
/// and treated as unset.
fn configured_remote_type(workdir: &Path, upstream_label: &str) -> Option<RemoteType> {
    let value = git::run_git_stdout(workdir, &["config", "--get", "loom.remote-type"]).ok()?;
    let value = value.trim();
    let remote_type = parse_remote_type(&value.to_lowercase(), upstream_label);
    if remote_type.is_none() {
        msg::warn(&format!(
            "Unknown loom.remote-type '{}' — falling back to auto-detection.\n\
             Valid values: github, gitlab, azure, gerrit, plain",
            value
        ));
    }
    remote_type
}

/// Guess the remote type from the remote URL, then from a Gerrit
/// `commit-msg` hook; `None` when neither tells.
fn auto_detect_remote_type(repo: &Repository, upstream_label: &str) -> Option<&'static str> {
    let remote_name = extract_remote_name(upstream_label);
    if let Ok(remote) = repo.find_remote(&remote_name)
        && let Ok(url) = remote.url()
    {
        if url.contains("github.com") {
            return Some("github");
        }
        if url.contains("gitlab") {
            return Some("gitlab");
        }
        if url.contains("dev.azure.com") {
            return Some("azure");
        }
        if on_gerrit_ssh_port(url) {
            return Some("gerrit");
        }
    }

    // Use repo.commondir() so this works in worktrees (where hooks are shared)
    let hook_path = repo.commondir().join("hooks").join("commit-msg");
    if let Ok(content) = std::fs::read_to_string(&hook_path)
        && content.to_lowercase().contains("gerrit")
    {
        return Some("gerrit");
    }
    None
}

/// The remote type without asking or saving anything: config, then
/// auto-detection, then Plain. For callers that cannot prompt.
fn detect_remote_type(repo: &Repository, workdir: &Path, upstream_label: &str) -> RemoteType {
    configured_remote_type(workdir, upstream_label)
        .or_else(|| {
            auto_detect_remote_type(repo, upstream_label)
                .and_then(|value| parse_remote_type(value, upstream_label))
        })
        .unwrap_or(RemoteType::Plain)
}

/// The remote type `loom push` uses: `loom.remote-type`, else what
/// auto-detection finds, else the user's pick from every type. A detected or
/// picked type is saved as `loom.remote-type`, so it is settled once per
/// repository.
pub fn resolve_remote_type(
    repo: &Repository,
    workdir: &Path,
    upstream_label: &str,
) -> Result<RemoteType> {
    if let Some(remote_type) = configured_remote_type(workdir, upstream_label) {
        return Ok(remote_type);
    }
    let value = match auto_detect_remote_type(repo, upstream_label) {
        Some(value) => value,
        None => ask_remote_type(upstream_label)?,
    };
    git::run_git(workdir, &["config", "loom.remote-type", value])?;
    Ok(parse_remote_type(value, upstream_label).expect("a known remote type"))
}

/// Ask which kind of server the upstream remote is, offering every type.
fn ask_remote_type(upstream_label: &str) -> Result<&'static str> {
    let picked = msg::select(
        &format!(
            "Which kind of remote is `{}`? (saved as `loom.remote-type`)",
            extract_remote_name(upstream_label)
        ),
        REMOTE_TYPES
            .iter()
            .map(|(_, name)| name.to_string())
            .collect(),
        "set `git config loom.remote-type <github|gitlab|azure|gerrit|plain>`, then re-run",
    )?;
    REMOTE_TYPES
        .iter()
        .find(|(_, name)| *name == picked)
        .map(|(value, _)| *value)
        .ok_or_else(|| anyhow!("Unknown remote type `{}`", picked))
}

/// Extract the remote name from an upstream label like "origin/main" → "origin".
pub fn extract_remote_name(upstream_label: &str) -> String {
    upstream_label
        .split('/')
        .next()
        .unwrap_or("origin")
        .to_string()
}

/// Extract `owner/repo` from a git remote URL for `gh --repo`, or `None` when
/// the remote is missing or the URL is unrecognised.
pub fn extract_gh_repo(repo: &Repository, remote: &str) -> Option<String> {
    let remote = repo.find_remote(remote).ok()?;
    let url = remote.url().ok()?;

    // SCP-style SSH URLs: [git@]<host>:owner/repo[.git], including bare aliases
    // like `github-work:owner/repo`. Told from real URLs by having no '://' and
    // no '/' before the ':'.
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

    // Any scheme on github.com, with or without a user or port:
    // https://github.com/owner/repo.git, ssh://git@github.com:22/owner/repo
    let (_, rest) = url.split_once("://")?;
    let (_, path) = rest.split_once('/')?;
    let host = host_port(url).split(':').next()?;
    host.eq_ignore_ascii_case("github.com")
        .then(|| path.trim_end_matches(".git").to_string())
}

/// Whether `remote`'s URL points at github.com itself.
pub fn on_github_com(repo: &Repository, remote: &str) -> bool {
    let Some(url) = repo
        .find_remote(remote)
        .ok()
        .and_then(|r| r.url().ok().map(str::to_string))
    else {
        return false;
    };
    let host = host_port(&url).split(':').next().unwrap_or("");
    host.eq_ignore_ascii_case("github.com")
}

/// Whether `url` is an `ssh://` URL on 29418, the port Gerrit serves SSH on.
fn on_gerrit_ssh_port(url: &str) -> bool {
    url.starts_with("ssh://")
        && host_port(url)
            .rsplit_once(':')
            .is_some_and(|(_, port)| port == "29418")
}

/// The `host[:port]` of a remote URL, without scheme, user or path; for an
/// scp-style `[user@]host:path`, `host:` and the path's first segment.
fn host_port(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or("");
    authority.rsplit_once('@').map_or(authority, |(_, h)| h)
}

/// The repository `origin_repo` (`owner/repo` on github.com) is a fork of,
/// as `(owner/repo, default branch)`; `None` when it is none or `gh` cannot
/// tell.
pub fn github_fork_parent(workdir: &Path, origin_repo: &str) -> Option<(String, String)> {
    let gh = Gh::new(workdir, origin_repo.split('/').next());
    let parent = run_gh(
        &gh,
        &[
            "api",
            "--hostname",
            "github.com",
            &format!("repos/{}", origin_repo),
            "--jq",
            r#".parent | select(.) | "\(.full_name) \(.default_branch)""#,
        ],
        None,
    )?;
    let (name, branch) = parent.trim().split_once(' ')?;
    Some((name.to_string(), branch.to_string()))
}

/// Extract the target branch from an upstream label like "origin/main" → "main".
pub fn extract_target_branch(upstream_label: &str) -> String {
    let branch = repo::upstream_local_branch(upstream_label);
    if branch.is_empty() {
        "main".to_string()
    } else {
        branch
    }
}

/// The remote `loom push` sends to: `loom.push-remote` if set, else `origin`
/// when the integration branch tracks `upstream` (GitHub fork convention),
/// else the integration branch's own remote.
pub fn resolve_push_remote(
    repo: &Repository,
    workdir: &Path,
    upstream_label: &str,
    remote_type: &RemoteType,
) -> String {
    if let Ok(push_remote) = git::run_git_stdout(workdir, &["config", "--get", "loom.push-remote"])
    {
        let remote = push_remote.trim();
        if !remote.is_empty() && repo.find_remote(remote).is_ok() {
            return remote.to_string();
        }
    }

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

/// The remote `loom push` sends feature branches to, when it differs from the
/// remote the integration branch tracks — i.e. a fork workflow.
///
/// Returns `None` for the common single-remote setup, where both are the same.
pub fn fork_push_remote(repo: &Repository, workdir: &Path, upstream_label: &str) -> Option<String> {
    let remote_type = detect_remote_type(repo, workdir, upstream_label);
    let push_remote = resolve_push_remote(repo, workdir, upstream_label, &remote_type);
    (push_remote != extract_remote_name(upstream_label)).then_some(push_remote)
}

/// The `gh` CLI in `workdir`, acting as the account that owns the head
/// repository when `gh` is logged in to it, else as its active account.
pub struct Gh<'a> {
    workdir: &'a Path,
    /// Passed as `GH_TOKEN` to every call.
    token: Option<String>,
}

impl<'a> Gh<'a> {
    /// A `GH_TOKEN`/`GITHUB_TOKEN` set by the user wins (Spec 011, gh account).
    pub fn new(workdir: &'a Path, head_owner: Option<&str>) -> Self {
        let mut gh = Gh {
            workdir,
            token: None,
        };
        let user_token = ["GH_TOKEN", "GITHUB_TOKEN"]
            .iter()
            .any(|var| std::env::var_os(var).is_some());
        if let Some(owner) = head_owner.filter(|_| !user_token) {
            gh.token = run_gh(&gh, &["auth", "token", "--user", owner], None)
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty());
        }
        gh
    }
}

/// Run `gh` with `args`, trace-log it, and return stdout on
/// success. Failures are logged (stderr goes to the trace) and yield `None`.
pub fn run_gh(gh: &Gh, args: &[&str], stdin: Option<&str>) -> Option<String> {
    use std::process::Stdio;

    let start = Instant::now();
    let mut cmd = Command::new("gh");
    cmd.current_dir(gh.workdir).args(args);
    if let Some(token) = &gh.token {
        cmd.env("GH_TOKEN", token);
    }
    // An inherited stdin would race `loom tui` for its keys.
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(input.as_bytes());
    }
    let output = child.wait_with_output().ok()?;
    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr);
    trace_log::log_command(
        "gh",
        &args.join(" "),
        duration_ms,
        output.status.success(),
        &stderr,
    );
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[path = "remote_test.rs"]
mod tests;
