use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use git2::Repository;

use crate::core::agent_mode;
use crate::core::graph;
use crate::core::msg;
use crate::core::repo;
use crate::core::repo::RemoteStatus;
use crate::git;
use crate::status;
use crate::trace as loom_trace;

/// Remote type detected for the push operation.
#[derive(Debug, PartialEq, Eq)]
enum RemoteType {
    Plain,
    GitHub,
    GitLab,
    AzureDevOps,
    Gerrit { target_branch: String },
}

/// One pushed branch and the branch its PR targets: the branch below it in
/// the stack, or the upstream target branch for the bottom layer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Layer {
    branch: String,
    base: String,
}

/// Everything one `loom push` touches.
///
/// A stacked branch cannot be reviewed without the branches it is built on,
/// so the requested branch is pushed together with its downstack. Published
/// branches above it whose remote is stale were rewritten by the same rebase
/// and are re-pushed too, otherwise the stack breaks on the server. Branches
/// above it that were never pushed are only reported.
#[derive(Debug, Default, PartialEq, Eq)]
struct PushPlan {
    /// Base of the bottom layer: the upstream branch (e.g. `main`).
    target_branch: String,
    /// Bottom-to-top chain ending with the requested branch.
    layers: Vec<Layer>,
    /// Published upstack branches whose remote is stale, nearest first.
    republish: Vec<Layer>,
    /// Upstack branches that were never pushed, nearest first: this push
    /// leaves them alone and only reports them.
    not_pushed: Vec<String>,
}

impl PushPlan {
    /// A plan for one branch with no PR bookkeeping (Gerrit `--no-pr`, tests).
    fn single(branch: &str) -> Self {
        PushPlan {
            layers: vec![Layer {
                branch: branch.to_string(),
                base: String::new(),
            }],
            ..Default::default()
        }
    }

    /// The branch the user asked to push.
    fn requested(&self) -> &str {
        &self
            .layers
            .last()
            .expect("a plan has at least one layer")
            .branch
    }

    /// True when the requested branch depends on other feature branches.
    fn is_stacked(&self) -> bool {
        self.layers.len() > 1
    }

    /// True when this push looks after more than one branch's PR, so those
    /// PRs form a chain. A re-published upstack layer makes a chain of a
    /// branch that is not itself stacked: its PR targets the one below.
    fn is_chain(&self) -> bool {
        self.layers.len() + self.republish.len() > 1
    }

    /// Every branch to push, downstack first, then re-published upstack.
    fn branches(&self) -> Vec<&str> {
        self.layers
            .iter()
            .chain(&self.republish)
            .map(|l| l.branch.as_str())
            .collect()
    }

    /// Layers whose PRs are looked after, downstack first; the flag marks the
    /// re-published upstack ones, which never get a PR created.
    fn pr_layers(&self) -> impl Iterator<Item = (&Layer, bool)> {
        self.layers
            .iter()
            .map(|l| (l, false))
            .chain(self.republish.iter().map(|l| (l, true)))
    }
}

/// Remote tracking status of a feature branch by name, `None` when it was
/// never pushed or the name is not a feature branch.
fn remote_status<'a>(info: &'a repo::RepoInfo, name: &str) -> Option<&'a RemoteStatus> {
    info.branches
        .iter()
        .find(|b| b.name == name)
        .and_then(|b| b.remote.as_ref())
}

/// Work out which branches `loom push <branch>` pushes and what each PR
/// targets, from the stack shape the status graph sees.
fn plan_push(info: &repo::RepoInfo, branch: &str, target_branch: &str) -> PushPlan {
    // A lower branch whose remote ref is gone had its PR merged or closed and
    // the branch deleted: pushing it would resurrect it on the server and aim
    // the layer above at a dead base. Drop it and let the layer above take its
    // place. The requested branch is pushed whatever its remote status.
    let chain: Vec<String> = graph::downstack(info, branch)
        .into_iter()
        .filter(|name| {
            name == branch || !matches!(remote_status(info, name), Some(RemoteStatus::Gone))
        })
        .collect();
    let layers: Vec<Layer> = chain
        .iter()
        .enumerate()
        .map(|(i, b)| Layer {
            branch: b.clone(),
            base: if i == 0 {
                target_branch.to_string()
            } else {
                chain[i - 1].clone()
            },
        })
        .collect();

    let mut republish = Vec::new();
    let mut not_pushed = Vec::new();
    // Walking up the stack: the nearest branch below that this push leaves on
    // the remote, which is what a re-published layer's PR targets.
    let mut base = chain
        .last()
        .cloned()
        .unwrap_or_else(|| target_branch.to_string());
    for name in graph::upstack(info, branch) {
        match remote_status(info, &name) {
            // Published and out of step with the remote: re-push it, or what
            // is on the server stops matching the stack it belongs to.
            Some(RemoteStatus::Different) => {
                republish.push(Layer {
                    base: base.clone(),
                    branch: name.clone(),
                });
                base = name;
            }
            // Never pushed: publishing it is the user's call, not this push's.
            None => not_pushed.push(name),
            // On the remote unchanged: nothing to push, but a layer above can
            // still target it.
            Some(RemoteStatus::Synced) => base = name,
            // Its PR was merged or closed and the ref is off the remote.
            Some(RemoteStatus::Gone) => {}
        }
    }

    PushPlan {
        target_branch: target_branch.to_string(),
        layers,
        republish,
        not_pushed,
    }
}

fn backticked(names: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    names
        .into_iter()
        .map(|n| format!("`{}`", n.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Success line for a completed `git push` of the plan's branches.
fn pushed_message(remote: &str, plan: &PushPlan) -> String {
    let mut message = format!(
        "Pushed {} to `{}`",
        backticked(plan.layers.iter().map(|l| &l.branch)),
        remote
    );
    if !plan.republish.is_empty() {
        message.push_str(&format!(
            "\nRe-pushed above `{}`: {}",
            plan.requested(),
            backticked(plan.republish.iter().map(|l| &l.branch))
        ));
    }
    message
}

/// Point out upstack branches this push does not touch.
fn report_not_pushed(plan: &PushPlan) {
    // The stack is linear, so the topmost one publishes every branch below it.
    if let Some(top) = plan.not_pushed.last() {
        msg::warn(&format!(
            "Not pushed above `{}`: {}\nRun `loom push {}` to publish them",
            plan.requested(),
            backticked(&plan.not_pushed),
            top
        ));
    }
}

/// Refuse a stacked branch on Azure DevOps.
///
/// Azure has no stacked pull requests, and `az repos pr update` cannot even
/// retarget an existing one, so a stack would land as PRs whose base says
/// nothing a reviewer can rely on. Every other remote type stacks freely.
fn refuse_stacked_azure(remote_type: &RemoteType, plan: &PushPlan) -> Result<()> {
    if *remote_type != RemoteType::AzureDevOps || !plan.is_stacked() {
        return Ok(());
    }
    bail!(
        "`{}` is stacked on {} — Azure DevOps has no stacked pull requests\n\
         Land the branches below it first, or push without a PR (`--no-pr`)",
        plan.requested(),
        backticked(plan.layers.iter().rev().skip(1).map(|l| &l.branch))
    )
}

/// Push a feature branch to remote.
///
/// Detects the remote type (plain, GitHub, Gerrit) and dispatches to the
/// appropriate push strategy. Accepts an optional branch argument (name or
/// shortID); if omitted, shows an interactive picker.
///
/// When `no_pr` is true, skips PR/review creation for all remote types.
/// For Gerrit, branches without a `wip/` prefix get a confirmation prompt.
///
/// When `force` is true, pushes with `--force` instead of the default
/// `--force-with-lease --force-if-includes`.
pub fn run(branch: Option<String>, no_pr: bool, force: bool) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "push")?.to_path_buf();
    let mut info = repo::gather_repo_info(&repo, false, 1)?;
    let hide_pattern = status::hide_pattern(&repo);

    if info.branches.is_empty() {
        bail!("No woven branches to push\nCreate a branch with `git loom branch` first");
    }
    if info
        .branches
        .iter()
        .all(|b| status::is_hidden(&b.name, &hide_pattern))
    {
        bail!(
            "Every woven branch is hidden by `loom.hideBranchPattern` (`{}`)\n\
             Rename one, or change the pattern to publish it",
            hide_pattern
        );
    }

    let branch_name = match branch {
        Some(b) => resolve_branch(&repo, &info, &b)?,
        None => pick_branch(&info, &hide_pattern)?,
    };
    refuse_hidden(&info, &branch_name, &hide_pattern)?;
    // Hidden branches leave the graph here, as they do in `loom status`. A
    // visible branch stacked above one goes with it: its parent link is gone,
    // so it is neither re-pushed nor hinted — and `refuse_hidden` above turns
    // down a direct push of it too, that being a push of the hidden commits.
    status::apply_hidden_branches(&repo, &mut info);

    let mut remote_type = detect_remote_type(&repo, &workdir, &info.upstream.label)?;
    if remote_type == RemoteType::Plain && looks_like_gerrit(&repo, &info.upstream.label) {
        remote_type = confirm_gerrit(&workdir, &info.upstream.label)?;
    }
    let remote_name = resolve_push_remote(&repo, &workdir, &info.upstream.label, &remote_type);

    let target_branch = extract_target_branch(&info.upstream.label);
    let plan = plan_push(&info, &branch_name, &target_branch);

    if no_pr {
        return match remote_type {
            RemoteType::Gerrit { .. } => {
                push_gerrit_no_pr(&workdir, &remote_name, &branch_name, force)
            }
            _ => push_plain(&workdir, &remote_name, &plan, force),
        };
    }

    refuse_stacked_azure(&remote_type, &plan)?;

    match remote_type {
        RemoteType::Plain => push_plain(&workdir, &remote_name, &plan, force),
        RemoteType::GitHub => push_github(
            &repo,
            &workdir,
            &remote_name,
            &plan,
            &info,
            &info.upstream.label,
            force,
        ),
        RemoteType::GitLab => push_gitlab(&workdir, &remote_name, &plan, force),
        RemoteType::AzureDevOps => push_azure(&repo, &workdir, &remote_name, &plan, &info, force),
        // Gerrit uploads the whole ancestry as a relation chain by itself.
        RemoteType::Gerrit { target_branch } => {
            push_gerrit(&workdir, &remote_name, &branch_name, &target_branch)
        }
    }
}

fn resolve_branch(repo: &Repository, info: &repo::RepoInfo, branch_arg: &str) -> Result<String> {
    let name = repo::resolve_arg(repo, branch_arg, &[repo::TargetKind::Branch])?.expect_branch()?;
    if info.branches.iter().any(|b| b.name == name) {
        Ok(name)
    } else {
        bail!("Branch '{}' is not woven into the integration branch", name)
    }
}

/// Refuse to publish a hidden branch (`loom.hideBranchPattern`, spec 001):
/// the requested branch itself, or one it is stacked on and would push along.
fn refuse_hidden(info: &repo::RepoInfo, branch: &str, hide_pattern: &str) -> Result<()> {
    let hidden = hidden_downstack(info, branch, hide_pattern);
    if hidden.is_empty() {
        return Ok(());
    }
    let hint = "Rename it, or change `loom.hideBranchPattern` to publish it";
    if hidden.iter().any(|h| h == branch) {
        bail!(
            "Branch `{}` is hidden and is never pushed\n{}",
            branch,
            hint
        );
    }
    bail!(
        "Branch `{}` is stacked on hidden branch {}, which is never pushed\n{}",
        branch,
        backticked(&hidden),
        hint
    );
}

/// The hidden branches in `branch`'s downstack, itself included, bottom first.
fn hidden_downstack(info: &repo::RepoInfo, branch: &str, hide_pattern: &str) -> Vec<String> {
    graph::downstack(info, branch)
        .into_iter()
        .filter(|b| status::is_hidden(b, hide_pattern))
        .collect()
}

fn pick_branch(info: &repo::RepoInfo, hide_pattern: &str) -> Result<String> {
    let items: Vec<String> = info
        .branches
        .iter()
        .filter(|b| !status::is_hidden(&b.name, hide_pattern))
        .map(|b| b.name.clone())
        .collect();
    msg::select(
        "Select branch to push",
        items,
        "re-run with: loom push <branch>",
    )
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
    if let Ok(config_value) = git::run_git_stdout(workdir, &["config", "--get", "loom.remote-type"])
    {
        let value = config_value.trim().to_lowercase();
        if value == "github" {
            return Ok(RemoteType::GitHub);
        }
        if value == "gitlab" {
            return Ok(RemoteType::GitLab);
        }
        if value == "azure" {
            return Ok(RemoteType::AzureDevOps);
        }
        if value == "gerrit" {
            let target_branch = extract_target_branch(upstream_label);
            return Ok(RemoteType::Gerrit { target_branch });
        }
        if value == "plain" {
            return Ok(RemoteType::Plain);
        }
        msg::warn(&format!(
            "Unknown loom.remote-type '{}' — falling back to auto-detection.\n\
             Valid values: github, gitlab, azure, gerrit, plain",
            config_value.trim()
        ));
    }

    let remote_name = extract_remote_name(upstream_label);
    if let Ok(remote) = repo.find_remote(&remote_name)
        && let Ok(url) = remote.url()
    {
        if url.contains("github.com") {
            return Ok(RemoteType::GitHub);
        }
        if url.contains("gitlab") {
            return Ok(RemoteType::GitLab);
        }
        if url.contains("dev.azure.com") {
            return Ok(RemoteType::AzureDevOps);
        }
    }

    // Use repo.commondir() so this works in worktrees (where hooks are shared)
    let hook_path = repo.commondir().join("hooks").join("commit-msg");
    if let Ok(content) = std::fs::read_to_string(&hook_path)
        && content.to_lowercase().contains("gerrit")
    {
        let target_branch = extract_target_branch(upstream_label);
        return Ok(RemoteType::Gerrit { target_branch });
    }

    Ok(RemoteType::Plain)
}

/// Heuristics that suggest — but don't prove — a Gerrit remote: the remote URL
/// uses Gerrit's standard SSH port (29418), or a recent commit carries a
/// `Change-Id:` trailer added by Gerrit's commit-msg hook.
///
/// These are only hints (the hook string check in [`detect_remote_type`] can
/// miss, e.g. when pre-commit manages the commit-msg hook), so callers should
/// confirm with the user before treating the remote as Gerrit.
fn looks_like_gerrit(repo: &Repository, upstream_label: &str) -> bool {
    let remote_name = extract_remote_name(upstream_label);
    if let Ok(remote) = repo.find_remote(&remote_name)
        && let Ok(url) = remote.url()
        && url.contains(":29418/")
    {
        return true;
    }
    recent_commits_have_change_id(repo)
}

/// Whether any of the last 20 commits reachable from HEAD carries a
/// `Change-Id:` trailer.
fn recent_commits_have_change_id(repo: &Repository) -> bool {
    let Ok(mut revwalk) = repo.revwalk() else {
        return false;
    };
    if revwalk.push_head().is_err() {
        return false;
    }
    revwalk.take(20).flatten().any(|oid| {
        repo.find_commit(oid)
            .ok()
            .and_then(|c| c.message().ok().map(|m| m.contains("\nChange-Id: I")))
            .unwrap_or(false)
    })
}

/// Ask the user to confirm a suspected Gerrit remote and persist the answer.
///
/// The answer is saved as `loom.remote-type` (`gerrit` or `plain`) in the repo
/// config so the question is asked at most once per repository.
fn confirm_gerrit(workdir: &Path, upstream_label: &str) -> Result<RemoteType> {
    let is_gerrit = msg::confirm(
        "This remote looks like Gerrit (SSH port 29418 or Change-Id trailers). Is it a Gerrit remote?",
        "set `git config loom.remote-type gerrit` (or plain), then re-run: loom push <branch>",
    )?;
    let value = if is_gerrit { "gerrit" } else { "plain" };
    git::run_git(workdir, &["config", "loom.remote-type", value])?;
    if is_gerrit {
        Ok(RemoteType::Gerrit {
            target_branch: extract_target_branch(upstream_label),
        })
    } else {
        Ok(RemoteType::Plain)
    }
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
    let url = remote.url().ok()?;

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
    let branch = repo::upstream_local_branch(upstream_label);
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
pub(crate) fn fork_push_remote(
    repo: &Repository,
    workdir: &Path,
    upstream_label: &str,
) -> Option<String> {
    let remote_type = detect_remote_type(repo, workdir, upstream_label).ok()?;
    let push_remote = resolve_push_remote(repo, workdir, upstream_label, &remote_type);
    (push_remote != extract_remote_name(upstream_label)).then_some(push_remote)
}

/// Run a `git push …`, trace-log it, bail on failure, and return its stderr.
///
/// stderr carries the server's `remote:` messages — GitLab MR links, Gerrit
/// review URLs, GitHub "create a pull request" hints — so callers surface them
/// to the user via [`append_remote_urls`].
fn run_push_capture(workdir: &Path, args: &[&str]) -> Result<String> {
    let start = Instant::now();
    let output = Command::new("git")
        .current_dir(workdir)
        .args(args)
        .output()?;

    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    loom_trace::log_command(
        "git",
        &args.join(" "),
        duration_ms,
        output.status.success(),
        &stderr,
    );

    if !output.status.success() {
        bail!("git push failed");
    }

    Ok(stderr)
}

/// Append `remote:` URLs found in git push stderr to `message`.
///
/// Servers print MR/review links as `remote:   https://…` lines. Each such URL
/// is added as an indented continuation line. A trailing `[tag]` (Gerrit) is
/// wrapped in backticks.
fn append_remote_urls(message: &mut String, stderr: &str) {
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
}

/// Flags that let a push overwrite the remote branch.
///
/// `--force` is what the user asked for explicitly; otherwise use the safe
/// pair that refuses to overwrite commits we haven't seen.
fn force_args(force: bool) -> &'static [&'static str] {
    if force {
        &["--force"]
    } else {
        &["--force-with-lease", "--force-if-includes"]
    }
}

/// Push `branches` to `remote` in one go (one lease check, one round trip)
/// and return the server's stderr for [`append_remote_urls`]. A stack goes
/// out atomically: a lease refused on one branch leaves the others untouched.
fn git_push(workdir: &Path, remote: &str, branches: &[&str], force: bool) -> Result<String> {
    run_push_capture(workdir, &push_args(remote, branches, force))
}

/// The `git push` argv for the plan's branches. A stack carries `--atomic`,
/// which is what makes a refused lease leave every branch untouched.
fn push_args<'a>(remote: &'a str, branches: &[&'a str], force: bool) -> Vec<&'a str> {
    let mut args = vec!["push"];
    args.extend_from_slice(force_args(force));
    if branches.len() > 1 {
        args.push("--atomic");
    }
    args.extend_from_slice(&["-u", remote]);
    args.extend_from_slice(branches);
    args
}

/// The commits a PR from `branch` onto `base` contains, oldest first: every
/// branch from `base` (exclusive) up to `branch`.
///
/// For a layer stacked straight on its base that is the branch's own commits.
/// When the base is further down — a fork's trunk-targeted PR, or a layer
/// dropped because its remote branch is gone — the PR's diff spans the
/// branches in between, and the description has to span them too.
fn commits_in_pr(info: &repo::RepoInfo, branch: &str, base: &str) -> Vec<git2::Oid> {
    let chain = graph::downstack(info, branch);
    let from = chain
        .iter()
        .position(|name| name == base)
        .map_or(0, |i| i + 1);
    chain[from..]
        .iter()
        .flat_map(|name| {
            let mut oids = graph::commits_in_branch(info, name);
            oids.reverse();
            oids
        })
        .collect()
}

/// Collect the commits a PR contains, oldest first, as `(subject, body)`
/// pairs where `body` is everything after the first line (may be empty).
fn gather_branch_commits(
    repo: &Repository,
    info: &repo::RepoInfo,
    branch_name: &str,
    base: &str,
) -> Result<Vec<(String, String)>> {
    let oids = commits_in_pr(info, branch_name, base);
    oids.iter()
        .map(|oid| {
            let commit = repo.find_commit(*oid)?;
            let subject = repo::commit_subject(&commit);
            let body = commit.body().ok().flatten().unwrap_or("").to_string();
            Ok((subject, body))
        })
        .collect()
}

/// Build a PR title and description from the commits the PR contains.
///
/// - **Single commit**: title = commit subject, description = commit body.
/// - **Multiple commits**: prompts the user for a title, then concatenates all
///   commit messages (oldest → newest) as the description.
fn pr_title_and_description(
    repo: &Repository,
    info: &repo::RepoInfo,
    branch_name: &str,
    base: &str,
) -> Result<(String, String)> {
    let commits = gather_branch_commits(repo, info, branch_name, base)?;

    if commits.is_empty() {
        return Ok((branch_name.to_string(), String::new()));
    }

    if commits.len() == 1 {
        let (subject, body) = &commits[0];
        return Ok((subject.clone(), body.clone()));
    }

    let title = msg::input(
        &format!("PR title for `{}`", branch_name),
        "PR creation is skipped in agent mode — this prompt is unreachable there",
        |s| {
            if s.is_empty() {
                Err("Title cannot be empty")
            } else {
                Ok(())
            }
        },
    )?;

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

/// Push every branch in the plan and report what went out.
fn push_plain(workdir: &Path, remote: &str, plan: &PushPlan, force: bool) -> Result<()> {
    let stderr = git_push(workdir, remote, &plan.branches(), force)?;
    let mut message = pushed_message(remote, plan);
    append_remote_urls(&mut message, &stderr);
    msg::success(&message);
    report_not_pushed(plan);
    Ok(())
}

/// Run `gh` with `args` in `workdir`, trace-log it, and return stdout on
/// success. Failures are logged (stderr goes to the trace) and yield `None`.
fn run_gh(workdir: &Path, args: &[&str], stdin: Option<&str>) -> Option<String> {
    use std::process::Stdio;

    let start = Instant::now();
    let mut cmd = Command::new("gh");
    cmd.current_dir(workdir).args(args);
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(input.as_bytes());
    }
    let output = child.wait_with_output().ok()?;
    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr);
    loom_trace::log_command(
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

/// A pull request as `gh` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GhPr {
    number: u64,
    url: String,
    /// The branch the PR merges into.
    base: String,
}

/// The PR number at the end of a GitHub PR URL.
fn pr_number_from_url(url: &str) -> Option<u64> {
    url.trim().rsplit('/').next()?.parse().ok()
}

/// Pick our PR out of the `gh pr list --json
/// number,url,baseRefName,headRepositoryOwner` output.
///
/// `gh pr list --head` matches the branch name across every fork of the
/// repository, so the PR whose head lives in `head_owner`'s repository is the
/// one; with no known owner the first match is taken.
fn parse_gh_pr_list(json: &str, head_owner: Option<&str>) -> Option<GhPr> {
    let prs: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    let pr = prs.as_array()?.iter().find(|pr| {
        let Some(owner) = head_owner else {
            return true;
        };
        pr.get("headRepositoryOwner")
            .and_then(|o| o.get("login"))
            .and_then(|l| l.as_str())
            .is_some_and(|login| login.eq_ignore_ascii_case(owner))
    })?;
    Some(GhPr {
        number: pr.get("number")?.as_u64()?,
        url: pr.get("url")?.as_str()?.to_string(),
        base: pr.get("baseRefName")?.as_str()?.to_string(),
    })
}

/// Find the open GitHub PR for `branch` whose head is in `head_owner`'s
/// repository, if any.
fn find_existing_github_pr(
    workdir: &Path,
    gh_repo: &str,
    branch: &str,
    head_owner: Option<&str>,
) -> Option<GhPr> {
    let stdout = run_gh(
        workdir,
        &[
            "pr",
            "list",
            "--head",
            branch,
            "--repo",
            gh_repo,
            "--json",
            "number,url,baseRefName,headRepositoryOwner",
            "--limit",
            "30",
        ],
        None,
    )?;
    parse_gh_pr_list(&stdout, head_owner)
}

/// Create a PR without opening the browser; returns it when `gh` succeeded.
fn create_github_pr(
    workdir: &Path,
    gh_repo: &str,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Option<GhPr> {
    let stdout = run_gh(
        workdir,
        &[
            "pr", "create", "--head", head, "--base", base, "--repo", gh_repo, "--title", title,
            "--body", body,
        ],
        None,
    )?;
    let url = stdout.lines().rev().find(|l| l.contains("/pull/"))?.trim();
    Some(GhPr {
        number: pr_number_from_url(url)?,
        url: url.to_string(),
        base: base.to_string(),
    })
}

/// Open the browser on GitHub's PR creation page, today's single-branch flow.
fn create_github_pr_web(
    workdir: &Path,
    gh_repo: &str,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Result<()> {
    // Inherits stdio so the browser opens.
    let args = vec![
        "pr", "create", "--web", "--head", head, "--base", base, "--repo", gh_repo, "--title",
        title, "--body", body,
    ];

    let start = Instant::now();
    let status = Command::new("gh")
        .current_dir(workdir)
        .args(&args)
        .status()?;

    let duration_ms = start.elapsed().as_millis();
    loom_trace::log_command("gh", &args.join(" "), duration_ms, status.success(), "");

    if !status.success() {
        msg::warn("PR creation may have failed — check your browser");
    }
    Ok(())
}

/// Point an existing PR at a different base branch.
fn retarget_github_pr(workdir: &Path, gh_repo: &str, number: u64, base: &str) -> bool {
    let number = number.to_string();
    run_gh(
        workdir,
        &["pr", "edit", &number, "--repo", gh_repo, "--base", base],
        None,
    )
    .is_some()
}

/// How the chain of PRs relates to GitHub's Stack objects.
#[derive(Debug, PartialEq, Eq)]
enum StackAction {
    /// No PR is in a stack yet: create one from the whole chain.
    Create,
    /// The bottom PRs are already in `stack`; append the rest, from index `from`.
    Extend { stack: u64, from: usize },
    /// Every PR is already in `stack`.
    Complete { stack: u64 },
    /// The chain spans several stacks, or a lower PR was unstacked: leave it.
    Conflict,
}

/// Decide what to do from each PR's stack membership, bottom to top.
fn plan_stack_registration(memberships: &[Option<u64>]) -> StackAction {
    let Some(Some(stack)) = memberships.first() else {
        return if memberships.iter().all(Option::is_none) {
            StackAction::Create
        } else {
            StackAction::Conflict
        };
    };
    let from = memberships
        .iter()
        .position(|m| *m != Some(*stack))
        .unwrap_or(memberships.len());
    if from == memberships.len() {
        StackAction::Complete { stack: *stack }
    } else if memberships[from..].iter().all(Option::is_none) {
        StackAction::Extend {
            stack: *stack,
            from,
        }
    } else {
        StackAction::Conflict
    }
}

/// The `stack.number` of a PR resource, `Some(None)` when it is in no stack,
/// `None` when the lookup failed.
fn github_pr_stack(workdir: &Path, gh_repo: &str, number: u64) -> Option<Option<u64>> {
    let path = format!("repos/{}/pulls/{}", gh_repo, number);
    let stdout = run_gh(workdir, &["api", &path, "--jq", ".stack"], None)?;
    Some(parse_stack_number(&stdout))
}

/// Parse the `.stack` object of a PR resource into its stack number.
fn parse_stack_number(json: &str) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    value.get("number")?.as_u64()
}

/// Body for the Stacks API: PR numbers ordered bottom to top.
fn stack_request_body(numbers: &[u64]) -> String {
    serde_json::json!({ "pull_requests": numbers }).to_string()
}

/// The PR numbers a GitHub Stack can link, bottom to top: the run of layers
/// from the bottom that all have a PR and each target the layer below. The
/// Stacks API requires every PR's base to be the previous PR's head, so a
/// layer skipped in between (an upstack branch that was already in sync)
/// ends the run.
fn stack_pr_numbers(chain: &[(Layer, Option<u64>)]) -> Vec<u64> {
    let mut numbers = Vec::new();
    for (i, (layer, number)) in chain.iter().enumerate() {
        let Some(number) = number else { break };
        if i > 0 && layer.base != chain[i - 1].0.branch {
            break;
        }
        numbers.push(*number);
    }
    numbers
}

/// Link the chain of PRs (bottom to top) into a GitHub Stack.
///
/// Nothing is remembered locally: the stack membership of each PR is read
/// back through the API on every push and reconciled.
fn register_github_stack(workdir: &Path, gh_repo: &str, numbers: &[u64]) {
    let memberships: Option<Vec<Option<u64>>> = numbers
        .iter()
        .map(|n| github_pr_stack(workdir, gh_repo, *n))
        .collect();
    let Some(memberships) = memberships else {
        msg::warn(
            "Could not read the GitHub stack of these PRs — left as they are\n\
             Stacked pull requests may be unavailable on this host — see `loom trace`\n\
             Link them on GitHub, or with `gh extension install github/gh-stack`",
        );
        return;
    };

    let (path, body, verb) = match plan_stack_registration(&memberships) {
        StackAction::Create => (
            format!("repos/{}/stacks", gh_repo),
            stack_request_body(numbers),
            "registered",
        ),
        StackAction::Extend { stack, from } => (
            format!("repos/{}/stacks/{}/add", gh_repo, stack),
            stack_request_body(&numbers[from..]),
            "extended",
        ),
        StackAction::Complete { stack } => {
            msg::success(&format!(
                "Stack #{} already links these {} PRs",
                stack,
                numbers.len()
            ));
            return;
        }
        StackAction::Conflict => {
            msg::warn(
                "These PRs belong to different GitHub stacks — left as they are\n\
                 Fix the stack on GitHub or with `gh stack`",
            );
            return;
        }
    };

    match run_gh(
        workdir,
        &["api", "--method", "POST", &path, "--input", "-"],
        Some(&body),
    ) {
        Some(stdout) => {
            let stack = match parse_stack_number(&stdout) {
                Some(n) => format!("Stack #{}", n),
                None => "Stack".to_string(),
            };
            msg::success(&format!("{} {} with {} PRs", stack, verb, numbers.len()));
        }
        None => msg::warn(
            "Could not register the GitHub stack (the PR bases are set)\n\
             Stacked pull requests may be unavailable on this host — see `loom trace`\n\
             Link them on GitHub, or with `gh extension install github/gh-stack`",
        ),
    }
}

/// Push to GitHub: push the plan's branches, then look after their PRs.
///
/// Supports fork workflow where the integration branch tracks the upstream
/// repository and the branch is pushed to a fork remote. The PR is created
/// against the integration branch's remote (usually the upstream/main repo)
/// with the head pointing to the push remote. GitHub cannot stack PRs across
/// forks, so there every PR targets the upstream branch.
///
/// If the branch being pushed is the upstream target branch itself (e.g.
/// pushing `main` when tracking `origin/main`), skip PR creation and fall
/// back to a plain force-with-lease push.
///
/// For each layer, bottom to top: an existing PR is retargeted if its base is
/// wrong and reported; a missing one is created. A lone branch keeps the
/// `gh pr create --web` flow; a stack creates PRs directly, because the stack
/// can only be registered once every PR exists. Re-published upstack branches
/// never get a PR created.
fn push_github(
    repo: &Repository,
    workdir: &Path,
    remote: &str,
    plan: &PushPlan,
    info: &repo::RepoInfo,
    upstream_label: &str,
    force: bool,
) -> Result<()> {
    push_plain(workdir, remote, plan, force)?;

    // Skip PR creation when pushing the upstream target branch itself
    if plan.requested() == plan.target_branch {
        return Ok(());
    }

    let start = Instant::now();
    let gh_check = Command::new("gh").arg("--version").output();
    let gh_available = gh_check.as_ref().is_ok_and(|o| o.status.success());
    let duration_ms = start.elapsed().as_millis();
    loom_trace::log_command("gh", "--version", duration_ms, gh_available, "");

    if !gh_available {
        msg::warn("Install 'gh' CLI to create pull requests: https://cli.github.com");
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
    // Owner of the repository the branches land in; PR lookups are pinned to
    // it because `gh pr list --head` matches the branch name across forks.
    let head_owner =
        extract_gh_repo(repo, remote).and_then(|r| r.split('/').next().map(str::to_string));
    // In fork workflow, --head needs "fork-owner:branch" prefix
    let head_for = |branch: &str| match (&head_owner, is_fork) {
        (Some(owner), true) => format!("{}:{}", owner, branch),
        _ => branch.to_string(),
    };

    let mut layers: Vec<(Layer, bool)> = plan
        .pr_layers()
        .map(|(l, republish)| (l.clone(), republish))
        .collect();
    if is_fork && layers.iter().any(|(l, _)| l.base != plan.target_branch) {
        msg::warn(&format!(
            "Stacked pull requests are not supported across forks — PRs target `{}`",
            plan.target_branch
        ));
        for (layer, _) in &mut layers {
            layer.base = plan.target_branch.clone();
        }
    }
    // One branch keeps the browser flow; a chain needs every PR to exist
    // before it can be linked, and only a same-repo chain can be registered.
    let web = !plan.is_chain();
    let register = plan.is_chain() && !is_fork;

    // Each layer with its PR number, bottom to top, for the stack registration.
    let mut chain: Vec<(Layer, Option<u64>)> = Vec::new();
    for (layer, republish) in &layers {
        let pr = match find_existing_github_pr(
            workdir,
            &pr_target_repo,
            &layer.branch,
            head_owner.as_deref(),
        ) {
            Some(pr) if pr.base != layer.base => {
                if retarget_github_pr(workdir, &pr_target_repo, pr.number, &layer.base) {
                    msg::success(&format!("PR retargeted to `{}`: {}", layer.base, pr.url));
                    Some(pr)
                } else {
                    msg::warn(&format!(
                        "Could not retarget {} to `{}` — see `loom trace`",
                        pr.url, layer.base
                    ));
                    // Its base on the server is not the layer below, so the
                    // stack run has to end here rather than be registered
                    // over a chain GitHub will reject.
                    None
                }
            }
            Some(pr) => {
                msg::success(&format!("PR updated: {}", pr.url));
                Some(pr)
            }
            None if *republish => None,
            None if agent_mode::enabled() => {
                // Post-mutation: the branch is already pushed, and PR creation
                // opens a browser or prompts for a title — never do that behind
                // an agent's back (see spec 019).
                msg::warn(&format!(
                    "Skipped creating a PR for `{}` (agent mode)\n\
                     Create it on GitHub, or run `loom push {}` interactively",
                    layer.branch, layer.branch
                ));
                None
            }
            None => {
                let (title, body) =
                    pr_title_and_description(repo, info, &layer.branch, &layer.base)?;
                let head = head_for(&layer.branch);
                if web {
                    create_github_pr_web(
                        workdir,
                        &pr_target_repo,
                        &head,
                        &layer.base,
                        &title,
                        &body,
                    )?;
                    None
                } else {
                    let pr = create_github_pr(
                        workdir,
                        &pr_target_repo,
                        &head,
                        &layer.base,
                        &title,
                        &body,
                    );
                    match &pr {
                        Some(pr) => msg::success(&format!("PR created: {}", pr.url)),
                        None => msg::warn(&format!(
                            "Could not create a PR for `{}` — see `loom trace`",
                            layer.branch
                        )),
                    }
                    pr
                }
            }
        };
        chain.push((layer.clone(), pr.map(|pr| pr.number)));
    }

    let numbers = stack_pr_numbers(&chain);
    if register && numbers.len() >= 2 {
        register_github_stack(workdir, &pr_target_repo, &numbers);
    }

    Ok(())
}

/// Push to GitLab: push each branch with `merge_request.create` push options so
/// GitLab creates (or points to) a merge request, then surface the MR URL it
/// prints in the push output. Re-published upstack layers only carry the
/// target option, which updates an existing MR without creating one.
///
/// Push options apply to the whole push, and each layer of a stack targets a
/// different branch, so the layers go out one push at a time, bottom first.
///
/// If the branch being pushed is the upstream target branch itself, skip the
/// MR push options and fall back to a plain push.
fn push_gitlab(workdir: &Path, remote: &str, plan: &PushPlan, force: bool) -> Result<()> {
    if plan.requested() == plan.target_branch {
        return push_plain(workdir, remote, plan, force);
    }

    for (layer, republish) in plan.pr_layers() {
        let target_opt = format!("merge_request.target={}", layer.base);
        let mut args = vec!["push"];
        args.extend_from_slice(force_args(force));
        if !republish {
            args.extend_from_slice(&["-o", "merge_request.create"]);
        }
        args.extend_from_slice(&["-o", &target_opt, "-u", remote, &layer.branch]);
        let stderr = run_push_capture(workdir, &args)?;
        let mut message = format!("Pushed `{}` to `{}`", layer.branch, remote);
        append_remote_urls(&mut message, &stderr);
        msg::success(&message);
    }
    report_not_pushed(plan);
    Ok(())
}

/// Azure DevOps coordinates parsed from a git remote URL.
struct AzureRemote {
    /// Organization URL, e.g. `https://dev.azure.com/<org>`.
    org_url: String,
    /// Team project name, when it can be read from the URL.
    project: Option<String>,
    /// Repository name, when it can be read from the URL.
    repository: Option<String>,
}

/// Extract the Azure DevOps organization URL and project from a remote URL.
///
/// Supports:
/// - HTTPS:  `https://dev.azure.com/<org>/<project>/...`    → org + project
/// - SSH:    `git@ssh.dev.azure.com:v3/<org>/<project>/...` → org + project
/// - Legacy: `https://<org>.visualstudio.com/...`           → org only
///
/// The project matters because `az repos pr create` only auto-detects it from
/// the git remote for the HTTPS form; with an SSH remote it must be passed
/// explicitly via `--project`.
///
/// Returns `None` if the URL is unrecognised.
fn extract_azure_remote(repo: &Repository, remote: &str) -> Option<AzureRemote> {
    let remote = repo.find_remote(remote).ok()?;
    let url = remote.url().ok()?;

    let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());

    // Repository is the segment after `_git` (HTTPS form), or the last segment
    // when there is no `_git` marker (SSH form: `<org>/<project>/<repo>`).
    let repository = |parts: &[&str]| {
        parts
            .iter()
            .position(|p| *p == "_git")
            .and_then(|i| parts.get(i + 1))
            .or_else(|| parts.get(2))
            .and_then(|r| opt(r.trim_end_matches(".git")))
    };

    let split_org_project = |rest: &str| {
        let parts: Vec<&str> = rest.split('/').collect();
        let org = opt(parts.first()?)?;
        let project = parts.get(1).and_then(|p| opt(p));
        Some((org, project, repository(&parts)))
    };

    if let Some(rest) = url.strip_prefix("https://dev.azure.com/") {
        let (org, project, repository) = split_org_project(rest)?;
        return Some(AzureRemote {
            org_url: format!("https://dev.azure.com/{}", org),
            project,
            repository,
        });
    }

    if let Some(rest) = url.strip_prefix("git@ssh.dev.azure.com:v3/") {
        let (org, project, repository) = split_org_project(rest)?;
        return Some(AzureRemote {
            org_url: format!("https://dev.azure.com/{}", org),
            project,
            repository,
        });
    }

    if let Some(rest) = url.strip_prefix("https://")
        && let Some((host, path)) = rest.split_once('/')
        && host.ends_with(".visualstudio.com")
    {
        // Legacy collection URLs are ambiguous about the project segment, so
        // leave it to az auto-detection rather than guessing.
        let parts: Vec<&str> = path.split('/').collect();
        return Some(AzureRemote {
            org_url: format!("https://{}", host),
            project: None,
            repository: repository(&parts),
        });
    }

    None
}

/// `az` arguments locating the repository: `--org` (plus `--project` and
/// `--repository`, which az stops auto-detecting once the org is explicit)
/// when the remote URL could be parsed, `--detect` otherwise.
fn azure_location_args(azure: Option<&AzureRemote>) -> Vec<&str> {
    let Some(azure) = azure else {
        return vec!["--detect"];
    };
    let mut args = vec!["--org", azure.org_url.as_str()];
    if let Some(project) = &azure.project {
        args.extend(["--project", project.as_str()]);
    }
    if let Some(repository) = &azure.repository {
        args.extend(["--repository", repository.as_str()]);
    }
    args
}

/// Build a `Command` for the Azure CLI.
///
/// On Windows `az` is normally a batch script, which `CreateProcess` cannot
/// start directly: the MSI installer ships `az.cmd` and `pip install
/// azure-cli` ships `az.bat`. Naming the script explicitly makes the standard
/// library run it through `cmd.exe` with every argument escaped for cmd, where
/// a hand-rolled `cmd /C az` would let cmd expand `%VAR%` and reparse quotes
/// inside PR titles. Installs that ship an `az.exe` are found by the plain
/// name.
fn az_command() -> Command {
    Command::new(az_program())
}

fn az_program() -> &'static str {
    static PROGRAM: OnceLock<&'static str> = OnceLock::new();
    PROGRAM.get_or_init(|| {
        if !cfg!(windows) {
            return "az";
        }
        ["az.cmd", "az.bat"]
            .into_iter()
            .find(|name| on_path(name))
            .unwrap_or("az")
    })
}

/// True when some `PATH` directory holds `file`.
fn on_path(file: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(file).is_file()))
}

/// Push to Azure DevOps: push the plan's branches, then look after their PRs.
///
/// An existing PR is reported as is (`az repos pr update` cannot change its
/// target); a missing one is created with `az repos pr create`, which opens
/// the browser on it. A stacked branch never reaches here — see
/// [`refuse_stacked_azure`].
fn push_azure(
    repo: &Repository,
    workdir: &Path,
    remote: &str,
    plan: &PushPlan,
    info: &repo::RepoInfo,
    force: bool,
) -> Result<()> {
    push_plain(workdir, remote, plan, force)?;

    let start = Instant::now();
    let az_check = az_command().arg("--version").output();
    let az_available = az_check.as_ref().is_ok_and(|o| o.status.success());
    let duration_ms = start.elapsed().as_millis();
    loom_trace::log_command("az", "--version", duration_ms, az_available, "");

    if !az_available {
        msg::warn(
            "Install 'az' CLI to create pull requests: \
             https://learn.microsoft.com/cli/azure/install-azure-cli",
        );
        return Ok(());
    }

    // Extract the org URL so we can pass --org explicitly rather than relying
    // on --detect, which produces a misleading "need to login" error when it
    // fails to infer the organisation from the remote URL.
    let azure = extract_azure_remote(repo, remote);

    for (layer, republish) in plan.pr_layers() {
        if let Some(pr_url) = find_existing_azure_pr(workdir, &layer.branch, azure.as_ref()) {
            msg::success(&format!("PR updated: {}", pr_url));
            continue;
        }
        if republish {
            continue;
        }

        // Post-mutation: the branch is already pushed, and PR creation opens a
        // browser or prompts for a title — never do that behind an agent's
        // back (see spec 019).
        if agent_mode::enabled() {
            msg::warn(&format!(
                "Skipped creating a PR for `{}` (agent mode)\n\
                 Create it on Azure DevOps, or run `loom push {}` interactively",
                layer.branch, layer.branch
            ));
            continue;
        }

        let (title, description) =
            pr_title_and_description(repo, info, &layer.branch, &layer.base)?;
        create_azure_pr(workdir, layer, &title, &description, azure.as_ref())?;
    }

    Ok(())
}

/// Run `az repos pr create` for one layer, with `--open` so the browser
/// shows the new PR.
fn create_azure_pr(
    workdir: &Path,
    layer: &Layer,
    title: &str,
    description: &str,
    azure: Option<&AzureRemote>,
) -> Result<()> {
    // Write description to a temp file and pass `--description @<path>` to az,
    // so lines that start with `-` (e.g. `---` separators) are never taken
    // for options.
    let mut desc_file = tempfile::Builder::new()
        .suffix(".txt")
        .tempfile()
        .context("Failed to create temp file for PR description")?;
    write!(desc_file, "{}", description).context("Failed to write PR description")?;
    let desc_path = desc_file.path().to_string_lossy().into_owned();
    let desc_arg = format!("@{}", desc_path);

    // `--open` shows the new PR in the browser, so nothing reads stdout back.
    let mut args: Vec<&str> = vec![
        "repos",
        "pr",
        "create",
        "--open",
        "--source-branch",
        &layer.branch,
        "--target-branch",
        &layer.base,
        "--title",
        title,
    ];

    args.extend(azure_location_args(azure));

    if !description.is_empty() {
        args.push("--description");
        args.push(&desc_arg);
    }

    let start = Instant::now();
    let output = az_command().current_dir(workdir).args(&args).output()?;
    let duration_ms = start.elapsed().as_millis();
    loom_trace::log_command(
        "az",
        &args.join(" "),
        duration_ms,
        output.status.success(),
        &String::from_utf8_lossy(&output.stderr),
    );

    if !output.status.success() {
        msg::warn(&format!(
            "Could not create a PR for `{}` — see `loom trace`",
            layer.branch
        ));
    }

    Ok(())
}

/// Browser URL of an Azure DevOps PR, built from its structured fields since
/// the API URL uses GUIDs and lacks the `/pullrequest/` path.
fn azure_pr_url(pr: &serde_json::Value) -> Option<String> {
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

/// Check if an Azure DevOps PR already exists for the given source branch.
///
/// Returns the PR web URL if found, or `None` if no PR exists or the check fails.
fn find_existing_azure_pr(
    workdir: &Path,
    branch: &str,
    azure: Option<&AzureRemote>,
) -> Option<String> {
    let mut cmd = az_command();
    cmd.current_dir(workdir);
    cmd.args([
        "repos",
        "pr",
        "list",
        "--source-branch",
        branch,
        "--output",
        "json",
    ]);
    cmd.args(azure_location_args(azure));
    let output = cmd.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let prs: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    azure_pr_url(prs.get(0)?)
}

/// Push to Gerrit without creating a review (plain force push).
///
/// If the branch is already prefixed with `wip/`, pushes directly.
/// Otherwise, warns the user that a Gerrit admin will be needed to delete
/// the remote branch later, and asks them to choose:
///   - Push as-is
///   - Push as `wip/<branch>` instead (no admin needed to delete)
///   - Cancel
fn push_gerrit_no_pr(workdir: &Path, remote: &str, branch: &str, force: bool) -> Result<()> {
    if branch.starts_with("wip/") {
        return push_plain(workdir, remote, &PushPlan::single(branch), force);
    }

    let opt_as_is = format!("Push as `{}` (admin required to delete it later)", branch);
    let opt_wip = format!("Push as `wip/{}` instead", branch);

    let choice = msg::select(
        &format!(
            "Branch `{}` is not prefixed with `wip/` — a Gerrit admin will be needed to delete the remote branch later",
            branch
        ),
        vec![opt_as_is.clone(), opt_wip.clone(), "Cancel".to_string()],
        "no flag answers this — ask the user; rename with `loom reword <branch> -m wip/<branch>` or re-run interactively",
    )?;

    if choice == opt_as_is {
        push_plain(workdir, remote, &PushPlan::single(branch), force)
    } else if choice == opt_wip {
        let wip_name = format!("wip/{}", branch);
        let refspec = format!("{}:{}", branch, wip_name);
        let mut args = vec!["push"];
        args.extend_from_slice(force_args(force));
        args.extend_from_slice(&[remote, &refspec]);
        git::run_git(workdir, &args)?;
        msg::success(&format!(
            "Pushed `{}` to `{}` as `{}`",
            branch, remote, wip_name
        ));
        Ok(())
    } else {
        bail!("Cancelled")
    }
}

/// Push to Gerrit with the refs/for/ refspec.
///
/// Captures stderr from the push command and extracts Gerrit review URLs
/// (lines starting with `remote:` that contain `http://` or `https://`).
fn push_gerrit(workdir: &Path, remote: &str, branch: &str, target_branch: &str) -> Result<()> {
    let refspec = format!("{}:refs/for/{}", branch, target_branch);

    let stderr = run_push_capture(workdir, &["push", remote, &refspec])?;

    let mut message = format!(
        "Pushed `{}` to `{}` (Gerrit: `refs/for/{}`)",
        branch, remote, target_branch
    );
    append_remote_urls(&mut message, &stderr);
    msg::success(&message);

    Ok(())
}

#[cfg(test)]
#[path = "push_test.rs"]
mod tests;
