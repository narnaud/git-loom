use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::DateTime;
use git2::{BranchType, Repository, StatusOptions};

use crate::core::msg;
use crate::git;

/// Open a `Repository` by discovering it from the current working directory.
pub fn open_repo() -> Result<Repository> {
    let cwd = std::env::current_dir()?;
    Ok(Repository::discover(cwd)?)
}

/// Return the working directory of the repository, or error if bare.
///
/// `operation` is a verb phrase used in the error message (e.g. "commit", "fold").
pub fn require_workdir<'a>(repo: &'a Repository, operation: &str) -> Result<&'a Path> {
    repo.workdir()
        .with_context(|| format!("Cannot {operation} in bare repository"))
}

/// Return the OID that HEAD points to.
pub fn head_oid(repo: &Repository) -> Result<git2::Oid> {
    repo.head()?.target().context("HEAD has no target")
}

/// Return the subject line (first line) of a commit message.
pub fn commit_subject(commit: &git2::Commit) -> String {
    commit.summary().unwrap_or("").to_string()
}

/// Capture all local branch name→OID mappings for rollback.
pub fn snapshot_branch_refs(repo: &Repository) -> Result<HashMap<String, git2::Oid>> {
    let mut refs = HashMap::new();
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        if let Some(name) = branch.name()?
            && let Some(oid) = branch.get().target()
        {
            refs.insert(name.to_string(), oid);
        }
    }
    Ok(refs)
}

/// Restore branches to snapshot OIDs, deleting any branches not in the snapshot.
pub fn restore_branch_refs(workdir: &Path, snapshot: &HashMap<String, git2::Oid>) -> Result<()> {
    let repo = Repository::discover(workdir)?;

    // Collect current branches
    let mut current_branches: HashMap<String, git2::Oid> = HashMap::new();
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        if let Some(name) = branch.name()?
            && let Some(oid) = branch.get().target()
        {
            current_branches.insert(name.to_string(), oid);
        }
    }

    // Get the current branch name so we skip it (can't force-update HEAD's branch)
    let head_branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()));

    let mut failures: Vec<String> = Vec::new();

    // Delete branches that weren't in the snapshot
    for name in current_branches.keys() {
        if !snapshot.contains_key(name)
            && Some(name.as_str()) != head_branch.as_deref()
            && let Err(e) = git::branch_delete(workdir, name)
        {
            failures.push(format!("delete '{}': {}", name, e));
        }
    }

    // Restore branches to their snapshot OIDs
    for (name, oid) in snapshot {
        if Some(name.as_str()) == head_branch.as_deref() {
            continue; // HEAD's branch is handled by reset --hard
        }
        let oid_str = oid.to_string();
        if let Err(e) = git::branch_force_create(workdir, name, &oid_str) {
            failures.push(format!("restore '{}': {}", name, e));
        }
    }

    if !failures.is_empty() {
        msg::warn(&format!(
            "Partial rollback — some branch refs could not be restored:\n{}",
            failures.join("\n")
        ));
    }

    Ok(())
}

/// Error if a local branch with the given name already exists.
pub fn ensure_branch_not_exists(repo: &Repository, name: &str) -> Result<()> {
    if repo.find_branch(name, BranchType::Local).is_ok() {
        bail!("Branch '{name}' already exists");
    }
    Ok(())
}

/// Default prefix for branches hidden from status display.
pub(crate) const DEFAULT_HIDE_PATTERN: &str = "local-";

/// Read the hidden branch prefix from git config `loom.hideBranchPattern`.
/// Returns `None` if the config key is not set.
pub fn hide_branch_pattern(repo: &Repository) -> Option<String> {
    repo.config()
        .ok()?
        .get_string("loom.hideBranchPattern")
        .ok()
}

/// Extract the local branch name from a remote tracking ref.
///
/// e.g. `"origin/main"` → `"main"`, `"origin/feat/foo"` → `"feat/foo"`.
pub fn upstream_local_branch(upstream_ref: &str) -> String {
    upstream_ref
        .split('/')
        .skip(1)
        .collect::<Vec<_>>()
        .join("/")
}

/// What a target identifier resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A commit (full or partial hash).
    Commit(String),
    /// A branch name.
    Branch(String),
    /// A file path (working tree change).
    File(String),
    /// A file within a specific commit (e.g. short ID `02:0`).
    CommitFile { commit: String, path: String },
    /// The unstaged working directory (short ID: `zz`).
    Unstaged,
}

impl Target {
    /// Unwrap a `Branch` variant, or bail with a descriptive error.
    pub fn expect_branch(self) -> Result<String> {
        match self {
            Target::Branch(name) => Ok(name),
            Target::Commit(_) => bail!("Target must be a branch, not a commit"),
            Target::File(_) => bail!("Target must be a branch, not a file"),
            Target::Unstaged => bail!("Target must be a branch"),
            Target::CommitFile { .. } => bail!("Target must be a branch, not a commit file"),
        }
    }
}

/// Which kinds of targets `resolve_arg` should try matching.
/// The order in the `accept` slice determines priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    File,
    Branch,
    Commit,
    CommitFile,
    Unstaged,
}

impl std::fmt::Display for TargetKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetKind::File => write!(f, "file"),
            TargetKind::Branch => write!(f, "branch"),
            TargetKind::Commit => write!(f, "commit"),
            TargetKind::CommitFile => write!(f, "commit file"),
            TargetKind::Unstaged => write!(f, "unstaged changes"),
        }
    }
}

/// Resolve a user-provided argument to a `Target`.
///
/// Only the resolution strategies for the kinds listed in `accept` are
/// attempted, in the order given.  The first match wins.  If nothing
/// matches, a generic error lists the accepted types.
pub fn resolve_arg(repo: &Repository, arg: &str, accept: &[TargetKind]) -> Result<Target> {
    // Phase 1: direct checks (cheap, no graph building)
    for kind in accept {
        let result = match kind {
            TargetKind::File => try_resolve_file(repo, arg)?,
            TargetKind::Branch => try_resolve_branch(repo, arg)?,
            TargetKind::Commit => try_resolve_commit(repo, arg)?,
            TargetKind::CommitFile | TargetKind::Unstaged => None,
        };
        if let Some(target) = result {
            return Ok(target);
        }
    }

    // Phase 2: shortid resolution (expensive, builds the graph)
    if let Some(target) = try_resolve_shortid(repo, arg, accept)? {
        return Ok(target);
    }

    let types: Vec<_> = accept.iter().map(|k| k.to_string()).collect();
    bail!("'{}' did not resolve to a {}", arg, types.join(" or "))
}

/// Reject a commit if it is a merge commit.
fn reject_merge_commit(repo: &Repository, oid: git2::Oid) -> Result<()> {
    let commit = repo.find_commit(oid)?;
    if commit.parent_count() > 1 {
        bail!("Cannot operate on a merge commit");
    }
    Ok(())
}

/// Try to resolve `arg` as a file path (CWD-relative → repo-relative).
fn try_resolve_file(repo: &Repository, arg: &str) -> Result<Option<Target>> {
    let repo_path = cwd_to_repo_path(repo, arg).unwrap_or_else(|_| arg.to_string());
    let workdir = match repo.workdir() {
        Some(w) => w,
        None => return Ok(None),
    };
    let full_path = workdir.join(&repo_path);
    if full_path.exists() {
        return Ok(Some(Target::File(repo_path)));
    }
    // Check for deleted files (removed from disk but changed vs HEAD)
    let diff = crate::git::diff_head_file(workdir, &repo_path)?;
    if !diff.is_empty() {
        return Ok(Some(Target::File(repo_path)));
    }
    Ok(None)
}

/// Try to resolve `arg` as a local branch name.
fn try_resolve_branch(repo: &Repository, arg: &str) -> Result<Option<Target>> {
    if let Ok(branch) = repo.find_branch(arg, BranchType::Local) {
        let name = branch
            .name()?
            .context("Branch name is not valid UTF-8")?
            .to_string();
        return Ok(Some(Target::Branch(name)));
    }
    Ok(None)
}

/// Try to resolve `arg` as a git reference (hash, HEAD, tag, etc.).
/// Rejects merge commits. Does NOT resolve local branch names as commits —
/// use `try_resolve_branch` for that.
fn try_resolve_commit(repo: &Repository, arg: &str) -> Result<Option<Target>> {
    // Explicitly exclude local branch names so that branch and commit targets
    // remain distinct when the caller specifies only TargetKind::Commit.
    if repo.find_branch(arg, BranchType::Local).is_ok() {
        return Ok(None);
    }
    if let Ok(obj) = repo.revparse_single(arg)
        && let Ok(commit) = obj.peel_to_commit()
    {
        let oid = commit.id();
        reject_merge_commit(repo, oid)?;
        return Ok(Some(Target::Commit(oid.to_string())));
    }
    Ok(None)
}

/// Try to resolve `arg` via the shortid allocator, but only return
/// results matching one of the `accept` kinds.
fn try_resolve_shortid(
    repo: &Repository,
    arg: &str,
    accept: &[TargetKind],
) -> Result<Option<Target>> {
    let needs_files = arg.contains(':');
    let info = gather_repo_info(repo, needs_files, 1)?;
    let entities = info.collect_entities();
    let allocator = crate::core::shortid::IdAllocator::new(entities);

    for kind in accept {
        match kind {
            TargetKind::Unstaged => {
                if allocator.get_unstaged() == arg {
                    return Ok(Some(Target::Unstaged));
                }
            }
            TargetKind::Branch => {
                for branch in &info.branches {
                    if allocator.get_branch(&branch.name) == arg {
                        return Ok(Some(Target::Branch(branch.name.clone())));
                    }
                }
            }
            TargetKind::Commit => {
                for commit in &info.commits {
                    if allocator.get_commit(commit.oid) == arg {
                        reject_merge_commit(repo, commit.oid)?;
                        return Ok(Some(Target::Commit(commit.oid.to_string())));
                    }
                }
            }
            TargetKind::File => {
                for file in &info.working_changes {
                    if allocator.get_file(&file.path) == arg || file.path == arg {
                        return Ok(Some(Target::File(file.path.clone())));
                    }
                }
            }
            TargetKind::CommitFile => {
                if let Some((commit_part, index_part)) = arg.split_once(':')
                    && let Ok(index) = index_part.parse::<usize>()
                {
                    for commit in &info.commits {
                        if allocator.get_commit(commit.oid) == commit_part {
                            if let Some(file) = commit.files.get(index) {
                                return Ok(Some(Target::CommitFile {
                                    commit: commit.oid.to_string(),
                                    path: file.path.clone(),
                                }));
                            }
                            bail!(
                                "Commit has no file at index {}\nRun `loom status -f` to see available IDs",
                                index
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Convert a CWD-relative path to a repo-relative path.
///
/// If CWD is `<repo>/src/` and `arg` is `"git.rs"`, returns `"src/git.rs"`.
fn cwd_to_repo_path(repo: &Repository, arg: &str) -> Result<String> {
    let prefix = cwd_relative_to_repo(repo)?;
    if prefix.is_empty() {
        return Ok(arg.to_string());
    }
    Ok(format!("{}/{}", prefix, arg))
}

/// Compute the CWD relative to the repo root as a forward-slash string.
///
/// Returns an empty string when CWD is the repo root.
pub fn cwd_relative_to_repo(repo: &Repository) -> Result<String> {
    let workdir = repo
        .workdir()
        .context("Repository has no working directory")?;
    let cwd = std::env::current_dir()?;
    // Canonicalize both paths to normalize platform-specific representations
    // (e.g., drive letter case and path separator style on Windows).
    let workdir_canonical =
        std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
    let cwd_canonical = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    let rel = cwd_canonical
        .strip_prefix(&workdir_canonical)
        .unwrap_or(std::path::Path::new(""));
    let s = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    Ok(s)
}

/// Convert a repo-relative path to a CWD-relative display string given
/// a pre-computed CWD prefix (as returned by [`cwd_relative_to_repo`]).
///
/// - Empty prefix → path returned unchanged (CWD is repo root).
/// - Path starts with prefix → strip it (file is under CWD).
/// - Otherwise → prepend the appropriate number of `../` components.
pub fn cwd_relative_path(repo_path: &str, cwd_prefix: &str) -> String {
    if cwd_prefix.is_empty() {
        return repo_path.to_string();
    }
    let cwd_parts: Vec<&str> = cwd_prefix.split('/').collect();
    let file_parts: Vec<&str> = repo_path.split('/').collect();
    let common = cwd_parts
        .iter()
        .zip(file_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let ups = cwd_parts.len() - common;
    let remaining = &file_parts[common..];
    let mut result: Vec<&str> = vec![".."; ups];
    result.extend_from_slice(remaining);
    result.join("/")
}

/// Info about the upstream tracking branch and the merge-base with HEAD.
#[derive(Debug)]
pub struct UpstreamInfo {
    /// Full name of the upstream ref (e.g. "origin/main").
    pub label: String,
    /// Full OID of the merge-base commit.
    pub merge_base_oid: git2::Oid,
    /// Short hash of the merge-base commit.
    pub base_short_id: String,
    /// First line of the merge-base commit message.
    pub base_message: String,
    /// Date of the merge-base commit (YYYY-MM-DD).
    pub base_date: String,
    /// How many commits upstream is ahead of the merge-base (0 = up-to-date).
    pub commits_ahead: usize,
}

/// All data needed to render the status: commits between HEAD and the upstream
/// tracking branch, detected feature branches, and working tree status.
#[derive(Debug)]
pub struct RepoInfo {
    /// Name of the current (integration) branch.
    pub branch_name: String,
    /// Upstream tracking branch info (merge-base, ahead count, etc.).
    pub upstream: UpstreamInfo,
    /// Non-merge commits in topological order (newest first) between HEAD
    /// and the merge-base. Merge commits are filtered out.
    pub commits: Vec<CommitInfo>,
    /// Local branches whose tip is in the commit range (excluding the
    /// current integration branch).
    pub branches: Vec<BranchInfo>,
    /// Staged and unstaged working tree changes.
    pub working_changes: Vec<FileChange>,
    /// Context commits before the base (for history context display).
    pub context_commits: Vec<ContextCommit>,
}

impl RepoInfo {
    /// Build a list of entities for shortid allocation.
    /// Returns entities in the order: Unstaged, Branches, Commits, Files.
    pub fn collect_entities(&self) -> Vec<crate::core::shortid::Entity> {
        let mut entities = vec![crate::core::shortid::Entity::Unstaged];

        for branch in &self.branches {
            entities.push(crate::core::shortid::Entity::Branch(branch.name.clone()));
        }

        for commit in &self.commits {
            entities.push(crate::core::shortid::Entity::Commit(commit.oid));
        }

        for file in &self.working_changes {
            entities.push(crate::core::shortid::Entity::File(file.path.clone()));
        }

        entities
    }
}

/// A single non-merge commit in the range upstream..HEAD.
#[derive(Debug)]
pub struct CommitInfo {
    pub oid: git2::Oid,
    /// Abbreviated hash respecting the repo's core.abbrev setting.
    pub short_id: String,
    /// First line of the commit message.
    pub message: String,
    /// Parent commit OID (None for root commits). Always a single parent
    /// since merge commits are excluded.
    pub parent_oid: Option<git2::Oid>,
    /// Files changed in this commit (only populated when `-f` is active).
    pub files: Vec<FileChange>,
}

/// Remote tracking status for a feature branch.
#[derive(Debug, Clone)]
pub enum RemoteStatus {
    /// Remote tracking ref exists and local tip matches it.
    Synced,
    /// Remote tracking ref exists but local is ahead.
    Ahead,
    /// Upstream was configured but the remote ref no longer exists.
    Gone,
}

/// A local branch whose tip falls within the upstream..HEAD range.
#[derive(Debug)]
pub struct BranchInfo {
    pub name: String,
    pub tip_oid: git2::Oid,
    pub remote: Option<RemoteStatus>,
}

/// A context commit shown below the upstream base for history context.
/// These are display-only (no short ID, not actionable).
#[derive(Debug)]
pub struct ContextCommit {
    pub short_hash: String,
    pub message: String,
    pub date: String,
}

/// A file with staged or unstaged changes in the working tree.
#[derive(Debug)]
pub struct FileChange {
    pub path: String,
    /// Index (staged) status: ' ', 'A', 'M', 'D', 'R', or '?'
    pub index: char,
    /// Worktree (unstaged) status: ' ', 'M', 'D', 'R', '?', or '!' (conflict)
    pub worktree: char,
}

/// Collect all data needed for the status display: walk commits from HEAD to the
/// upstream tracking branch, detect feature branches, and gather working tree status.
///
/// When `show_files` is true, each commit will include the list of files it touches.
pub fn gather_repo_info(repo: &Repository, show_files: bool, context: usize) -> Result<RepoInfo> {
    let head = repo.head()?;

    if !head.is_branch() {
        bail!("HEAD is detached\nSwitch to an integration branch");
    }

    let head_oid = head.target().context("HEAD does not point to a commit")?;

    let branch_name = head.shorthand().unwrap_or("HEAD").to_string();

    let local_branch = repo
        .find_branch(&branch_name, BranchType::Local)
        .with_context(|| format!("Branch '{}' not found — are you on a branch?", branch_name))?;

    let upstream = local_branch.upstream().with_context(|| {
        format!(
            "Branch '{}' has no upstream tracking branch\n\
             Set one with: git branch --set-upstream-to=<upstream> {}",
            branch_name, branch_name
        )
    })?;

    let upstream_name = upstream
        .name()?
        .context("Upstream branch name is not valid UTF-8")?
        .to_string();

    let upstream_oid = upstream
        .get()
        .target()
        .context("Upstream does not point to a commit")?;

    let merge_base_oid = repo.merge_base(head_oid, upstream_oid)?;

    let commits = walk_commits(repo, head_oid, merge_base_oid, show_files)?;
    let commit_set: std::collections::HashSet<git2::Oid> = commits.iter().map(|c| c.oid).collect();
    let branches = find_branches_in_range(
        repo,
        &commit_set,
        merge_base_oid,
        &branch_name,
        &upstream_name,
    )?;
    let working_changes = get_working_changes(repo)?;

    // Count how many commits upstream is ahead of the merge-base
    let commits_ahead = count_commits(repo, upstream_oid, merge_base_oid)?;

    // Get merge-base commit info
    let base_commit = repo.find_commit(merge_base_oid)?;
    let base_short_id = base_commit
        .as_object()
        .short_id()?
        .as_str()
        .context("Base commit short_id is not valid UTF-8")?
        .to_string();
    let base_message = commit_subject(&base_commit);
    let base_time = base_commit.time();
    let base_date = format_epoch(base_time.seconds());

    let context_commits = walk_context_commits(repo, merge_base_oid, context)?;

    Ok(RepoInfo {
        branch_name,
        upstream: UpstreamInfo {
            label: upstream_name,
            merge_base_oid,
            base_short_id,
            base_message,
            base_date,
            commits_ahead,
        },
        commits,
        branches,
        working_changes,
        context_commits,
    })
}

/// Check if a path (file or directory) has staged or unstaged changes.
pub fn path_has_changes(repo: &Repository, path: &str) -> Result<bool> {
    let mut opts = StatusOptions::new();
    opts.pathspec(path)
        .include_untracked(true)
        .recurse_untracked_dirs(true);

    let statuses = repo.statuses(Some(&mut opts))?;
    Ok(!statuses.is_empty())
}

/// Collect file paths that have staged (index) changes.
pub fn get_staged_files(repo: &Repository) -> Result<Vec<String>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    let mut paths = Vec::new();
    for entry in statuses.iter() {
        let status = entry.status();
        if (status.is_index_new()
            || status.is_index_modified()
            || status.is_index_deleted()
            || status.is_index_renamed()
            || status.is_index_typechange())
            && let Some(path) = entry.path()
        {
            paths.push(path.to_string());
        }
    }
    Ok(paths)
}

/// Count the number of commits reachable from `from` but not from `hide`.
fn count_commits(repo: &Repository, from: git2::Oid, hide: git2::Oid) -> Result<usize> {
    if from == hide {
        return Ok(0);
    }
    let mut revwalk = repo.revwalk()?;
    revwalk.push(from)?;
    revwalk.hide(hide)?;
    let mut count = 0usize;
    for oid_result in revwalk {
        oid_result?;
        count += 1;
    }
    Ok(count)
}

/// Walk N-1 commits before the merge-base to provide history context.
/// Returns an empty vec when `count` is 0 or 1 (the base itself is already
/// shown in the upstream section).
fn walk_context_commits(
    repo: &Repository,
    merge_base_oid: git2::Oid,
    count: usize,
) -> Result<Vec<ContextCommit>> {
    if count <= 1 {
        return Ok(vec![]);
    }
    let base_commit = repo.find_commit(merge_base_oid)?;
    let mut commits = Vec::new();
    let mut current = base_commit.parent(0).ok();
    let remaining = count - 1;
    while let Some(commit) = current {
        if commits.len() >= remaining {
            break;
        }
        let short_hash = commit
            .as_object()
            .short_id()?
            .as_str()
            .context("Context commit short_id is not valid UTF-8")?
            .to_string();
        let message = commit_subject(&commit);
        let date = format_epoch(commit.time().seconds());
        commits.push(ContextCommit {
            short_hash,
            message,
            date,
        });
        current = commit.parent(0).ok();
    }
    Ok(commits)
}

/// Format a Unix epoch timestamp as YYYY-MM-DD.
fn format_epoch(epoch: i64) -> String {
    DateTime::from_timestamp(epoch, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "????-??-??".to_string())
}

/// Walk commits from HEAD to the merge-base in topological order,
/// skipping merge commits.
fn walk_commits(
    repo: &Repository,
    head_oid: git2::Oid,
    stop_oid: git2::Oid,
    show_files: bool,
) -> Result<Vec<CommitInfo>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push(head_oid)?;
    revwalk.hide(stop_oid)?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL)?;

    let mut commits = Vec::new();
    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        // Skip merge commits
        if commit.parent_count() > 1 {
            continue;
        }
        let short_id = commit
            .as_object()
            .short_id()?
            .as_str()
            .context("Commit short_id is not valid UTF-8")?
            .to_string();
        let message = commit_subject(&commit);
        let parent_oid = commit.parent_id(0).ok();
        let files = if show_files {
            get_commit_files(repo, &commit)?
        } else {
            vec![]
        };
        commits.push(CommitInfo {
            oid,
            short_id,
            message,
            parent_oid,
            files,
        });
    }

    Ok(commits)
}

/// Return the file paths changed in a commit.
pub fn commit_file_paths(repo: &Repository, oid: git2::Oid) -> Result<Vec<String>> {
    let commit = repo.find_commit(oid)?;
    let files = get_commit_files(repo, &commit)?;
    Ok(files.into_iter().map(|f| f.path).collect())
}

/// Get the files changed in a commit by diffing against its parent tree.
/// For root commits (no parent), diffs against an empty tree.
fn get_commit_files(repo: &Repository, commit: &git2::Commit) -> Result<Vec<FileChange>> {
    let commit_tree = commit.tree()?;
    let parent_tree = if commit.parent_count() > 0 {
        Some(commit.parent(0)?.tree()?)
    } else {
        None
    };

    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)?;

    let mut files = Vec::new();
    for delta in diff.deltas() {
        let status = match delta.status() {
            git2::Delta::Added => 'A',
            git2::Delta::Modified => 'M',
            git2::Delta::Deleted => 'D',
            git2::Delta::Renamed => 'R',
            _ => '?',
        };
        let path = delta
            .new_file()
            .path()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();
        files.push(FileChange {
            path,
            index: status,
            worktree: ' ',
        });
    }

    Ok(files)
}

/// Find all local branches whose tip is in the commit range or at the
/// merge-base, excluding the current (integration) branch and branches
/// that track the same upstream remote.
fn find_branches_in_range(
    repo: &Repository,
    commit_set: &std::collections::HashSet<git2::Oid>,
    merge_base_oid: git2::Oid,
    current_branch: &str,
    upstream_name: &str,
) -> Result<Vec<BranchInfo>> {
    let mut branches = Vec::new();
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        let Some(name) = branch.name()? else {
            // Skip branches with non-UTF-8 names
            continue;
        };
        let name = name.to_string();
        // Skip the current (integration) branch itself
        if name == current_branch {
            continue;
        }
        // Skip branches that track the same upstream (e.g. main tracking origin/main)
        if let Ok(up) = branch.upstream()
            && let Ok(Some(up_name)) = up.name()
            && up_name == upstream_name
        {
            continue;
        }
        if let Some(tip_oid) = branch.get().target() {
            // Skip the upstream's local counterpart sitting at merge-base
            // (e.g. local "main" when upstream is "origin/main", even if it
            // tracks a different remote)
            if tip_oid == merge_base_oid && name == upstream_local_branch(upstream_name) {
                continue;
            }
            if commit_set.contains(&tip_oid) || tip_oid == merge_base_oid {
                let remote = detect_remote_status(repo, &branch, &name, tip_oid);
                branches.push(BranchInfo {
                    name,
                    tip_oid,
                    remote,
                });
            }
        }
    }

    Ok(branches)
}

/// Determine the remote tracking status of a feature branch.
///
/// Returns `None` if the branch has never been pushed (no upstream configured).
fn detect_remote_status(
    repo: &Repository,
    branch: &git2::Branch,
    name: &str,
    tip_oid: git2::Oid,
) -> Option<RemoteStatus> {
    // Try to access the upstream ref directly via git2.
    if let Ok(upstream) = branch.upstream() {
        return Some(match upstream.get().target() {
            Some(upstream_oid) if upstream_oid == tip_oid => RemoteStatus::Synced,
            _ => RemoteStatus::Ahead,
        });
    }

    // upstream() failed — check if an upstream was ever configured (gone case).
    // branch.upstream() returns Err for both "no upstream" and "upstream gone",
    // so we must consult git config to distinguish the two.
    let config = repo.config().ok()?;
    let remote_key = format!("branch.{}.remote", name);
    let Ok(remote) = config.get_string(&remote_key) else {
        return None; // never had an upstream configured
    };
    let merge_key = format!("branch.{}.merge", name);
    let Ok(merge) = config.get_string(&merge_key) else {
        return None;
    };
    let branch_part = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
    let tracking_ref = format!("refs/remotes/{}/{}", remote, branch_part);
    if repo.find_reference(&tracking_ref).is_err() {
        Some(RemoteStatus::Gone)
    } else {
        None
    }
}

pub(crate) fn get_working_changes(repo: &Repository) -> Result<Vec<FileChange>> {
    get_working_changes_opts(repo, false)
}

/// Like `get_working_changes` but with the option to recurse into untracked directories
/// so that individual files are listed instead of the directory as a single entry.
pub(crate) fn get_working_changes_recurse(repo: &Repository) -> Result<Vec<FileChange>> {
    get_working_changes_opts(repo, true)
}

fn get_working_changes_opts(repo: &Repository, recurse_untracked: bool) -> Result<Vec<FileChange>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(recurse_untracked);

    let statuses = repo.statuses(Some(&mut opts))?;
    let mut changes = Vec::new();

    for entry in statuses.iter() {
        let path = match entry.path() {
            Some(p) => p.to_string(),
            None => {
                // Handle non-UTF-8 paths by using lossy conversion
                String::from_utf8_lossy(entry.path_bytes()).into_owned()
            }
        };
        let status = entry.status();

        let index = if status.is_conflicted() {
            '!'
        } else if status.is_index_new() {
            'A'
        } else if status.is_index_modified() {
            'M'
        } else if status.is_index_deleted() {
            'D'
        } else if status.is_index_renamed() {
            'R'
        } else if status.is_wt_new() {
            '?'
        } else {
            ' '
        };

        let worktree = if status.is_wt_new() {
            '?'
        } else if status.is_conflicted() {
            '!'
        } else if status.is_wt_modified() {
            'M'
        } else if status.is_wt_deleted() {
            'D'
        } else if status.is_wt_renamed() {
            'R'
        } else {
            ' '
        };

        changes.push(FileChange {
            path,
            index,
            worktree,
        });
    }

    Ok(changes)
}

#[cfg(test)]
#[path = "repo_test.rs"]
mod tests;
