use anyhow::Result;
use colored::Colorize;

use crate::core::{graph, repo};
use crate::git;

/// Print one line per worktree, the main one first (Spec 022).
pub fn run(theme: &graph::Theme) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "list worktrees")?;
    let entries = super::entries(workdir)?;
    let ids = super::ids(&repo, &entries);
    let width = entries.iter().map(|e| e.name.len()).max().unwrap_or(0);

    // stdout even with `--agent`: the listing is the command's answer (Spec 019).
    for e in &entries {
        let id = ids.get_worktree(&e.name);
        let branch = match &e.branch {
            Some(b) => format!("[{b}]").color(theme.branch),
            None => "detached".color(theme.dim),
        };
        let mut line = format!(
            "{}{} {:<width$} {} {}",
            id.color(theme.shortid).underline(),
            " ".repeat(4usize.saturating_sub(id.len())),
            e.name,
            branch,
            e.path.display().to_string().color(theme.dim),
        );
        if e.current {
            line.push_str(" *");
        }
        if is_dirty(&e.path) {
            line.push_str(&format!(" {}", "dirty".color(theme.unstaged)));
        }
        println!("{line}");
    }
    Ok(())
}

/// Whether `git status` reports any change, untracked files included.
pub fn is_dirty(path: &std::path::Path) -> bool {
    check_dirty(path).unwrap_or(false)
}

/// [`is_dirty`], failing on a failed status rather than reporting clean.
/// `--no-optional-locks`: a status in another worktree must not take its index
/// lock and fail a command running there. The explicit modes keep
/// `status.showUntrackedFiles` and submodule `ignore` settings from hiding what
/// `git worktree remove` would delete.
pub fn check_dirty(path: &std::path::Path) -> Result<bool> {
    let out = git::run_git_stdout(
        path,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=normal",
            "--ignore-submodules=none",
        ],
    )?;
    Ok(!out.trim().is_empty())
}
