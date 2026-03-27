use anyhow::{Result, bail};

use crate::branch::is_on_first_parent_line;
use crate::core::msg;
use crate::core::repo;
use crate::core::weave::{self, Weave};

/// Remove a branch from the integration branch without deleting it.
///
/// The branch's commits are removed from the integration topology,
/// but the branch ref is preserved.
pub fn run(branch: Option<String>) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "unmerge")?;
    let info = repo::gather_repo_info(&repo, false, 1)?;

    let branch_name = match branch {
        Some(name) => resolve_woven_branch(&repo, &info, &name)?,
        None => pick_woven_branch(&info)?,
    };

    // Verify the branch is woven (not on first-parent line)
    let branch_info = info
        .branches
        .iter()
        .find(|b| b.name == branch_name)
        .expect("branch guaranteed to exist after resolve_woven_branch");

    let head_oid = repo::head_oid(&repo)?;
    let merge_base_oid = info.upstream.merge_base_oid;

    let is_woven = branch_info.tip_oid != head_oid
        && !is_on_first_parent_line(&repo, head_oid, merge_base_oid, branch_info.tip_oid)?;

    if !is_woven {
        bail!(
            "Branch '{}' is not woven into the integration branch",
            branch_name
        );
    }

    // Build weave and remove the branch section
    let mut graph = Weave::from_repo_with_info(&repo, &info)?;
    graph.drop_branch(&branch_name);

    let todo = graph.to_todo();
    weave::run_rebase_or_abort(workdir, Some(&graph.base_oid.to_string()), &todo)?;

    // Do NOT delete the branch ref — that's the key difference from `drop`
    msg::success(&format!(
        "Unwoven `{}` from integration branch",
        branch_name
    ));

    Ok(())
}

/// Resolve a branch argument to a woven branch name.
fn resolve_woven_branch(
    repo: &git2::Repository,
    info: &repo::RepoInfo,
    branch_arg: &str,
) -> Result<String> {
    let name = repo::resolve_arg(repo, branch_arg, &[repo::TargetKind::Branch])?.expect_branch()?;
    if info.branches.iter().any(|b| b.name == name) {
        Ok(name)
    } else {
        bail!("Branch '{}' is not woven into the integration branch", name)
    }
}

/// Interactive picker: list woven branches.
fn pick_woven_branch(info: &repo::RepoInfo) -> Result<String> {
    let items: Vec<String> = info.branches.iter().map(|b| b.name.clone()).collect();
    if items.is_empty() {
        bail!("No woven branches to unmerge");
    }
    msg::select("Select branch to unmerge", items)
}
