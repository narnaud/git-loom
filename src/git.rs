use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use git2::{BranchType, Repository, StatusOptions};

use crate::git_commands;

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

    // Delete branches that weren't in the snapshot
    for name in current_branches.keys() {
        if !snapshot.contains_key(name) && Some(name.as_str()) != head_branch.as_deref() {
            let _ = git_commands::run_git(workdir, &["branch", "-D", name]);
        }
    }

    // Restore branches to their snapshot OIDs
    for (name, oid) in snapshot {
        if Some(name.as_str()) == head_branch.as_deref() {
            continue; // HEAD's branch is handled by reset --hard
        }
        let oid_str = oid.to_string();
        let _ = git_commands::run_git(workdir, &["branch", "-f", name, &oid_str]);
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
}

impl RepoInfo {
    /// Build a list of entities for shortid allocation.
    /// Returns entities in the order: Unstaged, Branches, Commits, Files.
    pub fn collect_entities(&self) -> Vec<crate::shortid::Entity> {
        let mut entities = vec![crate::shortid::Entity::Unstaged];

        for branch in &self.branches {
            entities.push(crate::shortid::Entity::Branch(branch.name.clone()));
        }

        for commit in &self.commits {
            entities.push(crate::shortid::Entity::Commit(commit.oid));
        }

        for file in &self.working_changes {
            entities.push(crate::shortid::Entity::File(file.path.clone()));
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

/// A local branch whose tip falls within the upstream..HEAD range.
#[derive(Debug)]
pub struct BranchInfo {
    pub name: String,
    pub tip_oid: git2::Oid,
}

/// A file with staged or unstaged changes in the working tree.
#[derive(Debug)]
pub struct FileChange {
    pub path: String,
    /// Index (staged) status: ' ', 'A', 'M', 'D', 'R', or '?'
    pub index: char,
    /// Worktree (unstaged) status: ' ', 'M', 'D', 'R', or '?'
    pub worktree: char,
}

/// Collect all data needed for the status display: walk commits from HEAD to the
/// upstream tracking branch, detect feature branches, and gather working tree status.
///
/// When `show_files` is true, each commit will include the list of files it touches.
pub fn gather_repo_info(repo: &Repository, show_files: bool) -> Result<RepoInfo> {
    let head = repo.head()?;

    if !head.is_branch() {
        bail!("HEAD is detached. git-loom requires being on a branch.");
    }

    let head_oid = head.target().context("HEAD does not point to a commit")?;

    let branch_name = head.shorthand().unwrap_or("HEAD").to_string();

    let local_branch = repo
        .find_branch(&branch_name, BranchType::Local)
        .with_context(|| format!("branch '{}' not found — are you on a branch?", branch_name))?;

    let upstream = local_branch.upstream().with_context(|| {
        format!(
            "branch '{}' has no upstream tracking branch.\n\
             Set one with: git branch --set-upstream-to=origin/main {}",
            branch_name, branch_name
        )
    })?;

    let upstream_name = upstream
        .name()?
        .context("upstream branch name is not valid UTF-8")?
        .to_string();

    let upstream_oid = upstream
        .get()
        .target()
        .context("upstream does not point to a commit")?;

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
        .context("base commit short_id is not valid UTF-8")?
        .to_string();
    let base_message = base_commit.summary().unwrap_or("").to_string();
    let base_time = base_commit.time();
    let base_date = format_epoch(base_time.seconds());

    Ok(RepoInfo {
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
    })
}

/// Resolve a target identifier to a commit, branch, or file.
///
/// This function tries multiple resolution strategies in order:
///
/// # Resolution Strategy
///
/// 1. **Local branch names** - Exact match for local branch names
///    - Branch names resolve to `Target::Branch(name)`
///    - Example: "feature-a" → Target::Branch("feature-a")
/// 2. **Git references** - Any valid git reference (hash, HEAD, etc.)
///    - All other references resolve to `Target::Commit(hash)`
///    - Example: "abc123" → Target::Commit(full_hash)
/// 3. **ShortIDs** - Searches branches, commits, files in order
///    - Branch shortids resolve to `Target::Branch(name)`
///    - Commit shortids resolve to `Target::Commit(hash)`
///    - File shortids resolve to `Target::File(path)`
///
/// This prioritization ensures branch names are always treated as branches,
/// allowing intuitive operations like "git-loom reword feature-a -m new-name".
/// To reference the commit at a branch tip, use its hash or commit shortid.
///
/// # Arguments
///
/// * `repo` - The git repository
/// * `target` - The target identifier (branch name, git hash, shortid, etc.)
///
/// # Returns
///
/// Returns a `Target` enum indicating what the identifier resolved to:
/// - `Target::Branch(name)` - A branch name (from full name or branch shortid)
/// - `Target::Commit(hash)` - A commit hash (from git ref or commit shortid)
/// - `Target::File(path)` - A file path (from file shortid only)
pub fn resolve_target(repo: &Repository, target: &str) -> Result<Target> {
    // First, check if it's a local branch name
    // This allows intuitive branch operations: "git-loom reword feature-a -m new-name"
    if let Ok(branch) = repo.find_branch(target, BranchType::Local) {
        let branch_name = branch
            .name()?
            .context("Branch name is not valid UTF-8")?
            .to_string();
        return Ok(Target::Branch(branch_name));
    }

    // Try parsing as git reference (commit hash, HEAD, tag, etc.)
    if let Ok(obj) = repo.revparse_single(target)
        && let Ok(commit) = obj.peel_to_commit()
    {
        return Ok(Target::Commit(commit.id().to_string()));
    }

    // Not a valid git reference - try as shortid
    // This requires building the full graph (needs upstream)
    resolve_shortid(repo, target)
}

/// Resolve a shortid to a commit, branch, or file by rebuilding the graph.
fn resolve_shortid(repo: &Repository, shortid: &str) -> Result<Target> {
    // Gather repo info with files enabled so commit file shortids are resolvable
    let info = gather_repo_info(repo, true)?;

    // Build entities using the shared method
    let entities = info.collect_entities();
    let allocator = crate::shortid::IdAllocator::new(entities);

    // Check if it matches the unstaged entity
    if allocator.get_unstaged() == shortid {
        return Ok(Target::Unstaged);
    }

    // Search for matching shortid in branches
    for branch in &info.branches {
        if allocator.get_branch(&branch.name) == shortid {
            return Ok(Target::Branch(branch.name.clone()));
        }
    }

    // Search for matching shortid in commits
    for commit in &info.commits {
        if allocator.get_commit(commit.oid) == shortid {
            return Ok(Target::Commit(commit.oid.to_string()));
        }
    }

    // Search for matching shortid in working change files
    for file in &info.working_changes {
        if allocator.get_file(&file.path) == shortid {
            return Ok(Target::File(file.path.clone()));
        }
    }

    // Search for commit file shortids (format: "commit_sid:index")
    if let Some((commit_part, index_part)) = shortid.split_once(':')
        && let Ok(index) = index_part.parse::<usize>()
    {
        for commit in &info.commits {
            if allocator.get_commit(commit.oid) == commit_part {
                if let Some(file) = commit.files.get(index) {
                    return Ok(Target::CommitFile {
                        commit: commit.oid.to_string(),
                        path: file.path.clone(),
                    });
                }
                bail!(
                    "Commit has no file at index {}. Run 'git-loom status -f' to see available IDs.",
                    index
                );
            }
        }
    }

    bail!(
        "No commit, branch, file, or target with shortid '{}'. Run 'git-loom status' to see available IDs.",
        shortid
    )
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

/// Format a Unix epoch timestamp as YYYY-MM-DD.
fn format_epoch(epoch: i64) -> String {
    DateTime::<Utc>::from_timestamp(epoch, 0)
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
            .context("short_id is not valid UTF-8")?
            .to_string();
        let message = commit.summary().unwrap_or("").to_string();
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
        if let Some(tip_oid) = branch.get().target()
            && (commit_set.contains(&tip_oid) || tip_oid == merge_base_oid)
        {
            branches.push(BranchInfo { name, tip_oid });
        }
    }

    Ok(branches)
}

fn get_working_changes(repo: &Repository) -> Result<Vec<FileChange>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);

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

        let index = if status.is_wt_new() {
            '?'
        } else if status.is_index_new() {
            'A'
        } else if status.is_index_modified() {
            'M'
        } else if status.is_index_deleted() {
            'D'
        } else if status.is_index_renamed() {
            'R'
        } else {
            ' '
        };

        let worktree = if status.is_wt_new() {
            '?'
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
#[path = "git_test.rs"]
mod tests;
