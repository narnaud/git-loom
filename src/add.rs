use anyhow::Result;

use crate::core::repo::{self, Target, TargetKind};
use crate::git;

/// Stage files into the index using short IDs, filenames, or `zz` for all.
pub fn run(files: Vec<String>) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo
        .workdir()
        .expect("bare repositories are not supported")
        .to_path_buf();

    // `zz` stages everything, regardless of other args.
    if files.iter().any(|f| f == "zz") {
        git::stage_all(&workdir)?;
        println!("Staged all changes");
        return Ok(());
    }

    // Resolve each argument to a file path.
    let mut paths = Vec::new();
    for arg in &files {
        match repo::resolve_arg(&repo, arg, &[TargetKind::File])? {
            Target::File(path) => paths.push(path),
            _ => unreachable!(),
        }
    }

    let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    git::stage_files(&workdir, &path_refs)?;

    println!("Staged {} file(s)", paths.len());
    Ok(())
}

#[cfg(test)]
#[path = "add_test.rs"]
mod tests;
