use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Result, bail};
use git2::{Oid, Repository};

use crate::git::{self, CommitInfo};
use crate::git_commands::{self, git_commit};
use crate::msg;
use crate::weave::{self, Weave};

/// Result of analyzing a single file.
enum FileAnalysis {
    /// File assigned to a specific in-scope commit.
    Assigned { commit_oid: Oid },
    /// File skipped with a reason.
    Skipped { reason: String },
}

/// Absorb working tree changes into the commits that last touched the affected lines.
pub fn run(dry_run: bool, user_files: Vec<String>) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "absorb")?;
    let info = git::gather_repo_info(&repo, false)?;

    // Build in-scope commit set (non-merge commits between merge-base and HEAD)
    let in_scope: HashMap<Oid, &CommitInfo> = info.commits.iter().map(|c| (c.oid, c)).collect();

    if in_scope.is_empty() {
        bail!("No commits in scope — nothing to absorb into");
    }

    // Get changed files
    let changed_files = get_changed_files(&repo, workdir, &user_files)?;
    if changed_files.is_empty() {
        bail!("Nothing to absorb — make some changes to tracked files first");
    }

    // Build Weave for branch name lookup in output
    let weave_graph = Weave::from_repo_with_info(&repo, &info)?;

    // Analyze each file
    let mut assigned: Vec<(String, Oid)> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();

    for file in &changed_files {
        match analyze_file(&repo, workdir, file, &in_scope)? {
            FileAnalysis::Assigned { commit_oid } => {
                assigned.push((file.clone(), commit_oid));
            }
            FileAnalysis::Skipped { reason } => {
                skipped.push((file.clone(), reason));
            }
        }
    }

    // Print per-file output
    for (file, oid) in &assigned {
        let oid_str = oid.to_string();
        let short = git_commands::short_hash(&oid_str);
        let message = in_scope.get(oid).map(|c| c.message.as_str()).unwrap_or("");
        let branch_info = find_branch_for_commit(&weave_graph, *oid)
            .map(|b| format!(" ({})", b))
            .unwrap_or_default();
        println!("  {} -> {} \"{}\"{}", file, short, message, branch_info);
    }
    for (file, reason) in &skipped {
        println!("  {} -- skipped ({})", file, reason);
    }

    if assigned.is_empty() {
        bail!("No files could be absorbed");
    }

    let num_files = assigned.len();
    let unique_targets: HashSet<Oid> = assigned.iter().map(|(_, oid)| *oid).collect();
    let num_commits = unique_targets.len();

    if dry_run {
        println!(
            "\nDry run: would absorb {} file(s) into {} commit(s)",
            num_files, num_commits
        );
        return Ok(());
    }

    // Save state for rollback
    let saved_head = git::head_oid(&repo)?.to_string();
    let saved_refs = git::snapshot_branch_refs(&repo)?;

    // Group assigned files by target commit
    let mut groups: HashMap<Oid, Vec<String>> = HashMap::new();
    for (file, oid) in &assigned {
        groups.entry(*oid).or_default().push(file.clone());
    }

    // Create fixup commits for each group
    let mut fixup_pairs: Vec<(Oid, Oid)> = Vec::new(); // (fixup_oid, target_oid)

    for (target_oid, files) in &groups {
        let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        git_commit::stage_files(workdir, &file_refs)?;
        let msg = format!("fixup! absorb into {}", target_oid);
        if let Err(e) = git_commit::commit(workdir, &msg) {
            // Rollback: unstage and bail
            let _ = git_commit::reset_hard(workdir, &saved_head);
            let _ = git::restore_branch_refs(workdir, &saved_refs);
            return Err(e);
        }
        let repo2 = Repository::discover(workdir)?;
        let fixup_oid = git::head_oid(&repo2)?;
        fixup_pairs.push((fixup_oid, *target_oid));
    }

    // Save diffs of skipped files and restore them to HEAD before rebase.
    // This prevents autostash conflicts when the rebase rewrites history.
    let skipped_files: Vec<&str> = skipped.iter().map(|(f, _)| f.as_str()).collect();
    let skipped_patch = if !skipped_files.is_empty() {
        let patch = git_commands::diff_head_name_only(workdir)?;
        if !patch.trim().is_empty() {
            // There are still dirty files (the skipped ones) — save their full diff
            let full_patch = git_commands::run_git_stdout(workdir, &["diff", "HEAD"])?;
            let _ = git_commands::restore_files_to_head(workdir, &skipped_files);
            Some(full_patch)
        } else {
            None
        }
    } else {
        None
    };

    // Build Weave with the fixup commits, apply fixup_commit for each pair
    let repo2 = Repository::discover(workdir)?;
    let mut graph = Weave::from_repo(&repo2)?;
    for (fixup_oid, target_oid) in &fixup_pairs {
        graph.fixup_commit(*fixup_oid, *target_oid);
    }

    let todo = graph.to_todo();
    if let Err(e) = weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo) {
        let _ = git_commit::reset_hard(workdir, &saved_head);
        let _ = git::restore_branch_refs(workdir, &saved_refs);
        // Try to restore skipped file changes
        if let Some(ref patch) = skipped_patch {
            let _ = git_commands::apply_patch(workdir, patch);
        }
        return Err(e);
    }

    // Re-apply skipped file changes to the working tree
    if let Some(ref patch) = skipped_patch {
        let _ = git_commands::apply_patch(workdir, patch);
    }

    msg::success(&format!(
        "Absorbed {} file(s) into {} commit(s)",
        num_files, num_commits
    ));

    Ok(())
}

/// Determine the list of changed files to analyze.
///
/// When `user_files` is non-empty, each entry may be a file path or a short ID
/// (as shown by `git-loom status`). Short IDs are resolved via `resolve_target`.
fn get_changed_files(
    repo: &Repository,
    workdir: &Path,
    user_files: &[String],
) -> Result<Vec<String>> {
    if user_files.is_empty() {
        // All tracked files with uncommitted changes
        let output = git_commands::diff_head_name_only(workdir)?;
        Ok(output
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    } else {
        let mut result = Vec::new();
        for arg in user_files {
            let path = resolve_file_arg(repo, workdir, arg)?;
            result.push(path);
        }
        Ok(result)
    }
}

/// Resolve a user argument to a file path.
///
/// Tries, in order:
/// 1. Literal file path (exists on disk or has a diff against HEAD)
/// 2. Short ID via `resolve_target` → `Target::File(path)`
fn resolve_file_arg(repo: &Repository, workdir: &Path, arg: &str) -> Result<String> {
    // Try as literal path first
    let full_path = workdir.join(arg);
    if full_path.exists() {
        return Ok(arg.to_string());
    }
    // Check if it's a deletion (file existed in HEAD but was deleted)
    let diff = git_commands::diff_head_file(workdir, arg)?;
    if !diff.is_empty() {
        return Ok(arg.to_string());
    }

    // Try as short ID
    match git::resolve_target(repo, arg)? {
        git::Target::File(path) => Ok(path),
        _ => bail!(
            "'{}' is not a file path or file short ID\n\
             Run `git-loom status` to see available IDs",
            arg
        ),
    }
}

/// Analyze a single file to determine which commit it should be absorbed into.
fn analyze_file(
    repo: &Repository,
    workdir: &Path,
    path: &str,
    in_scope: &HashMap<Oid, &CommitInfo>,
) -> Result<FileAnalysis> {
    // Get the diff for this file
    let diff = git_commands::diff_head_file(workdir, path)?;

    if diff.is_empty() {
        return Ok(FileAnalysis::Skipped {
            reason: "no changes".to_string(),
        });
    }

    // Check for binary files
    if diff.contains("Binary files") {
        return Ok(FileAnalysis::Skipped {
            reason: "binary file".to_string(),
        });
    }

    // Parse modified original line numbers from the diff
    let modified_lines = parse_modified_lines(&diff);

    if modified_lines.is_empty() {
        return Ok(FileAnalysis::Skipped {
            reason: "pure addition".to_string(),
        });
    }

    // Blame the file at HEAD to find which commit owns each modified line
    let blame = match repo.blame_file(std::path::Path::new(path), None) {
        Ok(b) => b,
        Err(_) => {
            return Ok(FileAnalysis::Skipped {
                reason: "new file".to_string(),
            });
        }
    };

    let mut source_commits: HashSet<Oid> = HashSet::new();

    for &line_no in &modified_lines {
        if let Some(hunk) = blame.get_line(line_no) {
            source_commits.insert(hunk.final_commit_id());
        }
    }

    if source_commits.len() > 1 {
        return Ok(FileAnalysis::Skipped {
            reason: "lines from multiple commits".to_string(),
        });
    }

    let commit_oid = match source_commits.into_iter().next() {
        Some(oid) => oid,
        None => {
            return Ok(FileAnalysis::Skipped {
                reason: "no blame data".to_string(),
            });
        }
    };

    // Check if the commit is in scope
    if !in_scope.contains_key(&commit_oid) {
        return Ok(FileAnalysis::Skipped {
            reason: "out of scope".to_string(),
        });
    }

    Ok(FileAnalysis::Assigned { commit_oid })
}

/// Parse a unified diff to extract the original line numbers of modified/deleted lines.
///
/// Looks for `-` lines (lines removed from the original) and maps them back to
/// their original line numbers using hunk headers (`@@ -start,count ... @@`).
fn parse_modified_lines(diff: &str) -> Vec<usize> {
    let mut result = Vec::new();
    let mut current_orig_line: usize = 0;

    for line in diff.lines() {
        if let Some(start) = parse_hunk_header(line) {
            current_orig_line = start;
        } else if let Some(rest) = line.strip_prefix('-') {
            // A removed line (but not a file header like "--- a/file")
            if !rest.starts_with("-- ") {
                result.push(current_orig_line);
            }
            current_orig_line += 1;
        } else if line.starts_with('+') {
            // Added line — doesn't consume an original line number
        } else if !line.starts_with('\\') {
            // Context line — advances original line counter
            current_orig_line += 1;
        }
    }

    result
}

/// Parse a hunk header line to extract the starting line number of the original side.
///
/// Format: `@@ -start,count +new_start,new_count @@`
/// Returns the `start` value.
fn parse_hunk_header(line: &str) -> Option<usize> {
    let line = line.strip_prefix("@@ -")?;
    let end = line.find([',', ' '])?;
    line[..end].parse().ok()
}

/// Find the branch name that contains a given commit OID in the Weave graph.
fn find_branch_for_commit(weave: &Weave, oid: Oid) -> Option<String> {
    for section in &weave.branch_sections {
        for commit in &section.commits {
            if commit.oid == oid {
                return Some(section.label.clone());
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "absorb_test.rs"]
mod tests;
