use anyhow::{Context, Result, bail};

use crate::core::{msg, repo};

/// Print a worktree's path, alone, for a shell function to `cd` into (Spec
/// 022). Without an argument: the main worktree from a linked one, else a
/// picker.
pub fn run(target: Option<String>) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "locate a worktree")?;
    let entry = match target {
        Some(arg) => super::resolve(workdir, &arg)?,
        None => {
            let mut entries = super::entries(workdir)?;
            let in_main = entries.first().is_some_and(|e| e.current);
            if in_main {
                pick(entries)?
            } else {
                entries.swap_remove(0)
            }
        }
    };
    // Stdout even with `--agent`, ahead of its JSON status: the shell
    // functions read it.
    println!("{}", entry.path.display());
    Ok(())
}

fn pick(entries: Vec<super::Entry>) -> Result<super::Entry> {
    let mut others: Vec<_> = entries.into_iter().filter(|e| !e.current).collect();
    if others.is_empty() {
        bail!("No other worktree\nCreate one with `loom worktree new <name>`");
    }
    let items: Vec<String> = others
        .iter()
        .map(|e| format!("{}  {}", e.name, e.path.display()))
        .collect();
    let chosen = msg::select(
        "Select worktree",
        items.clone(),
        "re-run with: loom worktree cd <worktree>",
    )?;
    let index = items
        .iter()
        .position(|i| *i == chosen)
        .context("Selection is not in the list")?;
    Ok(others.swap_remove(index))
}

#[cfg(test)]
#[path = "cd_test.rs"]
mod tests;
