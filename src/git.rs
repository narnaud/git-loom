use git2::{BranchType, Repository, StatusOptions};

/// All data needed to render the log: commits between HEAD and the upstream
/// tracking branch, detected feature branches, and working tree status.
pub struct RepoInfo {
    /// Short hash of the upstream tracking branch tip (e.g. "ff1b247").
    pub upstream_short_id: String,
    /// Full name of the upstream ref (e.g. "origin/main").
    pub upstream_label: String,
    /// Non-merge commits in topological order (newest first) between HEAD
    /// and the upstream tip. Merge commits are filtered out.
    pub commits: Vec<CommitInfo>,
    /// Local branches whose tip is in the commit range (excluding the
    /// current integration branch).
    pub branches: Vec<BranchInfo>,
    /// Staged and unstaged working tree changes.
    pub working_changes: Vec<FileChange>,
}

/// A single non-merge commit in the range upstream..HEAD.
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
pub struct BranchInfo {
    pub name: String,
    pub tip_oid: git2::Oid,
}

/// A file with staged or unstaged changes in the working tree.
pub struct FileChange {
    pub path: String,
    /// One of: 'A' (added), 'M' (modified), 'D' (deleted), 'R' (renamed),
    /// '?' (unknown/untracked).
    pub status: char,
}

/// Collect all data needed for the log display: walk commits from HEAD to the
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
        .map_err(|_| {
            git2::Error::from_str(&format!(
                "branch '{}' not found — are you on a branch?",
                branch_name
            ))
        })?;

    let upstream = local_branch.upstream().map_err(|_| {
        git2::Error::from_str(&format!(
            "branch '{}' has no upstream tracking branch.\n\
             Set one with: git branch --set-upstream-to=origin/main {}",
            branch_name, branch_name
        ))
    })?;

    let upstream_name = upstream.name()?.unwrap_or("upstream").to_string();

    let upstream_oid = upstream
        .get()
        .target()
        .ok_or_else(|| git2::Error::from_str("upstream does not point to a commit"))?;

    let commits = walk_commits(repo, head_oid, upstream_oid)?;
    let branches = find_branches_in_range(repo, &commits, upstream_oid, &branch_name)?;
    let working_changes = get_working_changes(repo)?;

    let upstream_commit = repo.find_commit(upstream_oid)?;
    let upstream_short_id = upstream_commit
        .as_object()
        .short_id()?
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(RepoInfo {
        upstream_short_id,
        upstream_label: upstream_name,
        commits,
        branches,
        working_changes,
    })
}

/// Walk commits from HEAD to the upstream tip in topological order,
/// skipping merge commits.
fn walk_commits(
    repo: &Repository,
    head_oid: git2::Oid,
    upstream_oid: git2::Oid,
) -> Result<Vec<CommitInfo>, git2::Error> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push(head_oid)?;
    revwalk.hide(upstream_oid)?;
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
            .unwrap_or("")
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

/// Find all local branches whose tip is in the commit range, excluding
/// the current (integration) branch.
fn find_branches_in_range(
    repo: &Repository,
    commits: &[CommitInfo],
    upstream_oid: git2::Oid,
    current_branch: &str,
) -> Result<Vec<BranchInfo>, git2::Error> {
    let commit_set: std::collections::HashSet<git2::Oid> = commits.iter().map(|c| c.oid).collect();

    let mut branches = Vec::new();
    for branch_result in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch_result?;
        let name = branch.name()?.unwrap_or("").to_string();
        // Skip the current (integration) branch itself
        if name == current_branch {
            continue;
        }
        if let Some(tip_oid) = branch.get().target()
            && tip_oid != upstream_oid
            && commit_set.contains(&tip_oid)
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
        let path = entry.path().unwrap_or("").to_string();
        let status = entry.status();

        let status_char = if status.is_index_new() || status.is_wt_new() {
            'A'
        } else if status.is_index_modified() || status.is_wt_modified() {
            'M'
        } else if status.is_index_deleted() || status.is_wt_deleted() {
            'D'
        } else if status.is_index_renamed() || status.is_wt_renamed() {
            'R'
        } else {
            '?'
        };

        changes.push(FileChange {
            path,
            status: status_char,
        });
    }

    Ok(changes)
}
