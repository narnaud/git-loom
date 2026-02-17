use git2::{BranchType, Repository};

use crate::git_commands::git_branch;

/// Initialize a new integration branch tracking a remote upstream.
///
/// Creates a branch (default name: "loom") at the upstream tip and switches to it.
/// The remote is auto-detected from the current branch's upstream tracking ref.
/// If no upstream is found, the user is prompted to choose one.
pub fn run(name: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let repo = Repository::discover(cwd)?;

    let name = name.unwrap_or_else(|| "loom".to_string());
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Branch name cannot be empty".into());
    }

    git_branch::validate_name(&name)?;

    if repo.find_branch(&name, BranchType::Local).is_ok() {
        return Err(format!("Branch '{}' already exists", name).into());
    }

    let upstream = detect_upstream(&repo)?;

    let workdir = repo
        .workdir()
        .ok_or("Cannot create branch in bare repository")?;

    git_branch::switch_create_tracking(workdir, &name, &upstream)?;

    println!(
        "Initialized integration branch '{}' tracking {}",
        name, upstream
    );

    Ok(())
}

/// Detect the upstream tracking ref to use for the new integration branch.
///
/// Strategy:
/// 1. If the current branch has an upstream, use it (e.g., "origin/main").
/// 2. Otherwise, check each remote's HEAD symref (e.g., refs/remotes/origin/HEAD).
/// 3. Fall back to scanning for common branch names (main, master, develop).
/// 4. If exactly one candidate, use it. If multiple, prompt the user.
fn detect_upstream(repo: &Repository) -> Result<String, Box<dyn std::error::Error>> {
    // Try the current branch's upstream first
    if let Ok(head) = repo.head()
        && head.is_branch()
        && let Some(branch_name) = head.shorthand()
        && let Ok(local_branch) = repo.find_branch(branch_name, BranchType::Local)
        && let Ok(upstream) = local_branch.upstream()
        && let Ok(Some(upstream_name)) = upstream.name()
    {
        return Ok(upstream_name.to_string());
    }

    // No upstream on current branch — gather remote candidates
    let candidates = gather_remote_candidates(repo)?;

    match candidates.len() {
        0 => Err("No remote tracking branches found.\n\
             Set up a remote with: git remote add origin <url>"
            .into()),
        1 => Ok(candidates[0].clone()),
        _ => {
            // Prompt the user to pick
            let selection = cliclack::select("Which remote branch should this integration track?")
                .items(
                    &candidates
                        .iter()
                        .map(|c| (c.as_str(), c.as_str(), ""))
                        .collect::<Vec<_>>(),
                )
                .interact()?;
            Ok(selection.to_string())
        }
    }
}

/// Gather candidate remote tracking branches.
///
/// For each remote, first checks the remote's HEAD symref (e.g., refs/remotes/origin/HEAD)
/// which points to the remote's default branch. Falls back to scanning for common
/// branch names (main, master, develop) if the HEAD symref is not available.
fn gather_remote_candidates(repo: &Repository) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut candidates = Vec::new();

    let remotes = repo.remotes()?;
    for remote_name in remotes.iter() {
        let Some(remote_name) = remote_name else {
            continue;
        };

        // Try the remote's HEAD symref first (e.g., refs/remotes/origin/HEAD → origin/main)
        let head_ref = format!("refs/remotes/{}/HEAD", remote_name);
        if let Ok(reference) = repo.find_reference(&head_ref)
            && let Ok(resolved) = reference.resolve()
            && let Some(name) = resolved.shorthand()
        {
            candidates.push(name.to_string());
            continue;
        }

        // Fall back to common default branch names
        for branch_name in &["main", "master", "develop"] {
            let ref_name = format!("{}/{}", remote_name, branch_name);
            if repo.find_branch(&ref_name, BranchType::Remote).is_ok() {
                candidates.push(ref_name);
                break; // Use the first match per remote
            }
        }
    }

    Ok(candidates)
}

#[cfg(test)]
#[path = "init_test.rs"]
mod tests;
