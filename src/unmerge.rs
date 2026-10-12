use anyhow::{Result, bail};

use crate::core::msg;
use crate::core::repo;
use crate::core::ui;
use crate::core::weave::{self, Weave};

/// Remove a branch from the integration branch without deleting it.
///
/// The branch's commits are removed from the integration topology,
/// but the branch ref is preserved. Under the TUI, a branch passed in (taken
/// off the tree) or one picked here that takes others at its commit with it
/// is confirmed once every check passed (Spec 020).
pub fn run(branch: Option<String>) -> Result<()> {
    let passed_in = branch.is_some();
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "unmerge")?;
    let info = repo::gather_repo_info(&repo, false, 1)?;
    let mut graph = Weave::from_repo_with_info(&repo, &info)?;

    let branch_name = match branch {
        Some(name) => resolve_woven_branch(&repo, &info, &name)?,
        None => pick_woven_branch(&info, &graph)?,
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
        && !repo::is_on_first_parent_line(&repo, head_oid, merge_base_oid, branch_info.tip_oid)?;

    if !is_woven {
        bail!(
            "Branch '{}' is not woven into the integration branch",
            branch_name
        );
    }

    // A stacked branch has no section of its own: its commits are the outer
    // branch's too, and leave only once nothing stacked on them stays.
    if is_stacked(&graph, &branch_name) {
        match graph.inner_branch_keeper(&branch_name) {
            Some(outer) => bail!(
                "Branch `{}` is stacked under `{}`, which holds its commits too\n\
                 Unmerge `{}` first, then `{}`",
                branch_name,
                outer,
                outer,
                branch_name
            ),
            None => bail!(
                "Branch `{}` is stacked under commits that hold its own too",
                branch_name
            ),
        }
    }
    let mut leaving = vec![branch_name.clone()];
    leaving.extend(
        graph
            .section_branch_names(&branch_name)
            .iter()
            .filter(|n| **n != branch_name)
            .cloned(),
    );
    let names = name_list(&leaving);
    if !graph.drop_branch(&branch_name) {
        bail!(
            "Cannot unmerge: branch '{}' not found in weave graph",
            branch_name
        );
    }

    let detail = if leaving.len() > 1 {
        "They are at the same commit, so they leave together\n\
         Their commits leave the integration branch; the branches keep them, so merging them again brings them back"
    } else {
        "Its commits leave the integration branch; the branch keeps them, so merging it again brings them back"
    };
    let confirm = ui::active() && (passed_in || leaving.len() > 1);
    if confirm && !msg::confirm(&format!("Unmerge {}?\n{}", names, detail), "")? {
        return Err(msg::cancelled());
    }

    let todo = graph.to_todo();
    weave::run_rebase_or_abort(workdir, Some(&graph.base_oid.to_string()), &todo)?;

    // Do NOT delete the branch ref — that's the key difference from `drop`
    msg::success(&format!("Unmerged {} from integration branch", names));

    Ok(())
}

/// Backquoted names joined with commas and a final `and`.
fn name_list(names: &[String]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
    match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {}", rest.join(", "), last),
        None => String::new(),
    }
}

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

fn is_stacked(graph: &Weave, branch_name: &str) -> bool {
    !graph.has_branch_section(branch_name) && graph.is_inner_branch(branch_name)
}

/// Interactive picker: list woven branches, but the stacked ones `run` refuses.
fn pick_woven_branch(info: &repo::RepoInfo, graph: &Weave) -> Result<String> {
    let items: Vec<String> = info
        .branches
        .iter()
        .filter(|b| !is_stacked(graph, &b.name))
        .map(|b| b.name.clone())
        .collect();
    if items.is_empty() {
        bail!("No woven branches to unmerge");
    }
    msg::select(
        "Select branch to unmerge",
        items,
        "re-run with: loom unmerge <branch>",
    )
}

#[cfg(test)]
#[path = "unmerge_test.rs"]
mod tests;
