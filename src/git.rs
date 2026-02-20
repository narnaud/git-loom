use std::path::Path;

use chrono::{DateTime, Utc};
use git2::{BranchType, Repository, StatusOptions};

/// Open a `Repository` by discovering it from the current working directory.
pub fn open_repo() -> Result<Repository, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    Ok(Repository::discover(cwd)?)
}

/// Return the working directory of the repository, or error if bare.
///
/// `operation` is a verb phrase used in the error message (e.g. "commit", "fold").
pub fn require_workdir<'a>(
    repo: &'a Repository,
    operation: &str,
) -> Result<&'a Path, Box<dyn std::error::Error>> {
    repo.workdir()
        .ok_or_else(|| format!("Cannot {operation} in bare repository").into())
}

/// Return the OID that HEAD points to.
pub fn head_oid(repo: &Repository) -> Result<git2::Oid, Box<dyn std::error::Error>> {
    repo.head()?
        .target()
        .ok_or_else(|| "HEAD has no target".into())
}

/// Error if a local branch with the given name already exists.
pub fn ensure_branch_not_exists(
    repo: &Repository,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if repo.find_branch(name, BranchType::Local).is_ok() {
        return Err(format!("Branch '{name}' already exists").into());
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
    /// A file path.
    File(String),
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
pub fn gather_repo_info(repo: &Repository) -> Result<RepoInfo, git2::Error> {
    let head = repo.head()?;

    if !head.is_branch() {
        return Err(git2::Error::from_str(
            "HEAD is detached. git-loom requires being on a branch.",
        ));
    }

    let head_oid = head
        .target()
        .ok_or_else(|| git2::Error::from_str("HEAD does not point to a commit"))?;

    let branch_name = head.shorthand().unwrap_or("HEAD").to_string();

    let local_branch = repo
        .find_branch(&branch_name, BranchType::Local)
        .map_err(|e| {
            git2::Error::from_str(&format!(
                "branch '{}' not found — are you on a branch? ({})",
                branch_name, e
            ))
        })?;

    let upstream = local_branch.upstream().map_err(|e| {
        git2::Error::from_str(&format!(
            "branch '{}' has no upstream tracking branch.\n\
             Set one with: git branch --set-upstream-to=origin/main {}\n\
             Cause: {}",
            branch_name, branch_name, e
        ))
    })?;

    let upstream_name = upstream
        .name()?
        .ok_or_else(|| git2::Error::from_str("upstream branch name is not valid UTF-8"))?
        .to_string();

    let upstream_oid = upstream
        .get()
        .target()
        .ok_or_else(|| git2::Error::from_str("upstream does not point to a commit"))?;

    let merge_base_oid = repo.merge_base(head_oid, upstream_oid)?;

    let commits = walk_commits(repo, head_oid, merge_base_oid)?;
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
        .ok_or_else(|| git2::Error::from_str("base commit short_id is not valid UTF-8"))?
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
pub fn resolve_target(
    repo: &Repository,
    target: &str,
) -> Result<Target, Box<dyn std::error::Error>> {
    // First, check if it's a local branch name
    // This allows intuitive branch operations: "git-loom reword feature-a -m new-name"
    if let Ok(branch) = repo.find_branch(target, BranchType::Local) {
        let branch_name = branch
            .name()?
            .ok_or("Branch name is not valid UTF-8")?
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
fn resolve_shortid(repo: &Repository, shortid: &str) -> Result<Target, Box<dyn std::error::Error>> {
    // Gather repo info (this checks for upstream and builds the graph)
    let info = gather_repo_info(repo)?;

    // Build entities using the shared method
    let entities = info.collect_entities();
    let allocator = crate::shortid::IdAllocator::new(entities);

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

    // Search for matching shortid in files
    for file in &info.working_changes {
        if allocator.get_file(&file.path) == shortid {
            return Ok(Target::File(file.path.clone()));
        }
    }

    Err(format!(
        "No commit, branch, or file with shortid '{}'. Run 'git-loom status' to see available IDs.",
        shortid
    )
    .into())
}

/// Count the number of commits reachable from `from` but not from `hide`.
fn count_commits(
    repo: &Repository,
    from: git2::Oid,
    hide: git2::Oid,
) -> Result<usize, git2::Error> {
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
) -> Result<Vec<CommitInfo>, git2::Error> {
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
            .ok_or_else(|| git2::Error::from_str("short_id is not valid UTF-8"))?
            .to_string();
        let message = commit.summary().unwrap_or("").to_string();
        let parent_oid = commit.parent_id(0).ok();
        commits.push(CommitInfo {
            oid,
            short_id,
            message,
            parent_oid,
        });
    }

    Ok(commits)
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
) -> Result<Vec<BranchInfo>, git2::Error> {
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

fn get_working_changes(repo: &Repository) -> Result<Vec<FileChange>, git2::Error> {
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
