use git2::{BranchType, Repository, StatusOptions};

/// Info about the upstream tracking branch and the merge-base with HEAD.
#[derive(Debug)]
pub struct UpstreamInfo {
    /// Full name of the upstream ref (e.g. "origin/main").
    pub label: String,
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
    /// One of: 'A' (added), 'M' (modified), 'D' (deleted), 'R' (renamed),
    /// '?' (unknown/untracked).
    pub status: char,
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

    let merge_base_oid = repo.merge_base(head_oid, upstream_oid)?;

    let commits = walk_commits(repo, head_oid, merge_base_oid)?;
    let branches = find_branches_in_range(repo, &commits, merge_base_oid, &branch_name)?;
    let working_changes = get_working_changes(repo)?;

    // Count how many commits upstream is ahead of the merge-base
    let commits_ahead = count_commits(repo, upstream_oid, merge_base_oid)?;

    // Get merge-base commit info
    let base_commit = repo.find_commit(merge_base_oid)?;
    let base_short_id = base_commit
        .as_object()
        .short_id()?
        .as_str()
        .unwrap_or("")
        .to_string();
    let base_message = base_commit.summary().unwrap_or("").to_string();
    let base_time = base_commit.time();
    let base_date = format_epoch(base_time.seconds());

    Ok(RepoInfo {
        upstream: UpstreamInfo {
            label: upstream_name,
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
    Ok(revwalk.count())
}

/// Format a Unix epoch timestamp as YYYY-MM-DD.
fn format_epoch(epoch: i64) -> String {
    const SECS_PER_DAY: i64 = 86400;
    let days = epoch / SECS_PER_DAY;

    // Civil date from day count (algorithm from Howard Hinnant)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
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

#[cfg(test)]
#[path = "git_test.rs"]
mod tests;
