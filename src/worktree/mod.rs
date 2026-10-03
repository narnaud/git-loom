//! `loom worktree`: worktrees that each hold their own integration branch
//! (Spec 022).

pub mod drop;
pub mod list;
pub mod new;
pub mod path;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::core::shortid::{Entity, IdAllocator};
use crate::git;

/// A registered worktree whose directory exists.
#[derive(Debug)]
pub struct Entry {
    pub path: PathBuf,
    /// See [`name_of`].
    pub name: String,
    /// `None` when HEAD is detached.
    pub branch: Option<String>,
    pub main: bool,
    /// The worktree loom runs in.
    pub current: bool,
}

/// Every usable worktree, the main one first; bare and prunable ones are left
/// out.
pub fn entries(workdir: &Path) -> Result<Vec<Entry>> {
    let worktrees = git::list_worktrees(workdir)?;
    let main_dir = worktrees
        .first()
        .map(|w| dir_name(&w.path))
        .unwrap_or_default();
    let current = canonical(workdir);
    Ok(worktrees
        .into_iter()
        .enumerate()
        .filter(|(_, w)| !w.bare && !w.prunable && w.path.exists())
        .map(|(i, w)| Entry {
            name: if i == 0 {
                dir_name(&w.path)
            } else {
                name_of(&dir_name(&w.path), &main_dir)
            },
            current: canonical(&w.path) == current,
            main: i == 0,
            path: w.path,
            branch: w.branch,
        })
        .collect())
}

/// A linked worktree's name: its directory name `dir` minus `<main_dir>-`, or
/// the whole of `dir` when it does not start that way.
pub fn name_of(dir: &str, main_dir: &str) -> String {
    dir.strip_prefix(&format!("{main_dir}-"))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(dir)
        .to_string()
}

pub fn dir_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Short IDs for `entries`, allocated among worktrees only (Spec 022).
pub fn ids(entries: &[Entry]) -> IdAllocator {
    IdAllocator::new(
        entries
            .iter()
            .map(|e| Entity::Worktree(e.name.clone()))
            .collect(),
    )
}

/// The worktree `arg` names: a name, a checked-out branch, a short ID, or a
/// path (relative to the current directory), tried in that order so that, as
/// in Spec 002, a real name wins over an ID spelled the same. Names are not
/// unique (and two same-named worktrees share an ID), so a rule matching
/// several refuses rather than pick one.
pub fn resolve(workdir: &Path, arg: &str) -> Result<Entry> {
    let mut entries = entries(workdir)?;
    let ids = ids(&entries);
    let path = canonical(Path::new(arg));
    let rules: [&dyn Fn(&Entry) -> bool; 4] = [
        &|e| e.name == arg,
        &|e| e.branch.as_deref() == Some(arg),
        &|e| ids.get_worktree(&e.name) == arg,
        &|e| canonical(&e.path) == path,
    ];
    for matches in rules {
        let found: Vec<usize> = (0..entries.len())
            .filter(|&i| matches(&entries[i]))
            .collect();
        match found[..] {
            [] => {}
            [i] => return Ok(entries.swap_remove(i)),
            _ => {
                let paths: Vec<String> = found
                    .iter()
                    .map(|&i| format!("`{}`", entries[i].path.display()))
                    .collect();
                bail!(
                    "Worktree `{arg}` is ambiguous: {}\nName it by its path",
                    paths.join(", ")
                );
            }
        }
    }
    bail!("Worktree `{arg}` not found\nRun `loom worktree list` to see them")
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
