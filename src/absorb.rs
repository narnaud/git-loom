use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Result, bail};
use git2::{Oid, Repository};

use crate::git::{self, CommitInfo};
use crate::git_commands::{self, git_commit};
use crate::msg;
use crate::weave::{self, Weave};

/// Per-hunk analysis result.
enum HunkAnalysis {
    /// Hunk assigned to a specific in-scope commit.
    Assigned { commit_oid: Oid },
    /// Hunk skipped with a reason.
    Skipped { reason: String },
}

/// Result of analyzing a single file.
enum FileAnalysis {
    /// All hunks assigned to the same commit (whole-file absorb).
    Assigned { commit_oid: Oid },
    /// Hunks split across multiple targets or some skipped.
    Split {
        hunks: Vec<(DiffHunk, HunkAnalysis)>,
    },
    /// Entire file skipped.
    Skipped { reason: String },
}

/// Absorb working tree changes into the commits that last touched the affected lines.
pub fn run(dry_run: bool, user_files: Vec<String>) -> Result<()> {
    let repo = git::open_repo()?;
    let workdir = git::require_workdir(&repo, "absorb")?;
    let info = git::gather_repo_info(&repo, false, 1)?;

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

    // Analyze each file — collect assignments at hunk level
    let mut whole_file_assigned: Vec<(String, Oid)> = Vec::new();
    let mut hunk_assigned: Vec<(String, Oid, Vec<DiffHunk>)> = Vec::new();
    let mut skipped_files: Vec<String> = Vec::new();
    let mut any_assigned = false;

    for file in &changed_files {
        match analyze_file(&repo, workdir, file, &in_scope)? {
            FileAnalysis::Assigned { commit_oid } => {
                print_assignment(file, commit_oid, &in_scope, &weave_graph);
                whole_file_assigned.push((file.clone(), commit_oid));
                any_assigned = true;
            }
            FileAnalysis::Split { hunks } => {
                let total = hunks.len();
                let mut per_commit: HashMap<Oid, Vec<DiffHunk>> = HashMap::new();
                let mut has_skipped = false;

                for (i, (hunk, analysis)) in hunks.into_iter().enumerate() {
                    match analysis {
                        HunkAnalysis::Assigned { commit_oid } => {
                            print_hunk_assignment(
                                file,
                                i + 1,
                                total,
                                commit_oid,
                                &in_scope,
                                &weave_graph,
                            );
                            per_commit.entry(commit_oid).or_default().push(hunk);
                        }
                        HunkAnalysis::Skipped { reason } => {
                            println!(
                                "  {} [hunk {}/{}] -- skipped ({})",
                                file,
                                i + 1,
                                total,
                                reason
                            );
                            has_skipped = true;
                        }
                    }
                }

                if has_skipped {
                    skipped_files.push(file.clone());
                }

                for (oid, hunks_for_commit) in per_commit {
                    any_assigned = true;
                    hunk_assigned.push((file.clone(), oid, hunks_for_commit));
                }
            }
            FileAnalysis::Skipped { reason } => {
                println!("  {} -- skipped ({})", file, reason);
                skipped_files.push(file.clone());
            }
        }
    }

    if !any_assigned {
        bail!("No files could be absorbed");
    }

    // Count stats
    let num_hunks: usize = whole_file_assigned.len()
        + hunk_assigned
            .iter()
            .map(|(_, _, hunks)| hunks.len())
            .sum::<usize>();
    let mut unique_targets: HashSet<Oid> = HashSet::new();
    let mut assigned_file_set: HashSet<&str> = HashSet::new();
    for (f, oid) in &whole_file_assigned {
        unique_targets.insert(*oid);
        assigned_file_set.insert(f.as_str());
    }
    for (f, oid, _) in &hunk_assigned {
        unique_targets.insert(*oid);
        assigned_file_set.insert(f.as_str());
    }
    let num_commits = unique_targets.len();
    let num_files = assigned_file_set.len();

    if dry_run {
        println!(
            "\nDry run: would absorb {} hunk(s) from {} file(s) into {} commit(s)",
            num_hunks, num_files, num_commits
        );
        return Ok(());
    }

    // Save state for rollback
    let saved_head = git::head_oid(&repo)?.to_string();
    let saved_refs = git::snapshot_branch_refs(&repo)?;

    // Group whole-file assignments by target commit
    let mut groups: HashMap<Oid, Vec<String>> = HashMap::new();
    for (file, oid) in &whole_file_assigned {
        groups.entry(*oid).or_default().push(file.clone());
    }

    // Group hunk assignments by target commit
    let mut hunk_groups: HashMap<Oid, Vec<(String, Vec<DiffHunk>)>> = HashMap::new();
    for (path, oid, hunks) in hunk_assigned {
        hunk_groups.entry(oid).or_default().push((path, hunks));
    }

    // Build the set of files being absorbed so we can identify pre-existing
    // staged files that are NOT being absorbed and must not leak into any
    // fixup commit.
    let absorbed_files: std::collections::HashSet<&str> = groups
        .values()
        .flatten()
        .map(|s| s.as_str())
        .chain(
            hunk_groups
                .values()
                .flatten()
                .map(|(path, _)| path.as_str()),
        )
        .collect();
    let pre_staged = git::get_staged_files(&repo)?;
    let non_absorbed_staged: Vec<&str> = pre_staged
        .iter()
        .filter(|f| !absorbed_files.contains(f.as_str()))
        .map(|s| s.as_str())
        .collect();
    let saved_staged = if non_absorbed_staged.is_empty() {
        String::new()
    } else {
        let patch = git_commands::diff_cached_files(workdir, &non_absorbed_staged)?;
        git_commands::unstage_files(workdir, &non_absorbed_staged)?;
        patch
    };

    // Snapshot full working-tree diff before any mutations, for rollback
    let saved_worktree_patch = git_commands::run_git_stdout(workdir, &["diff", "HEAD"])?;

    let rollback = |warn_patch_err: bool| {
        let _ = git_commit::reset_hard(workdir, &saved_head);
        let _ = git::restore_branch_refs(workdir, &saved_refs);
        // Best-effort restore of pre-existing staged state
        if !saved_staged.is_empty() {
            let _ = git_commands::apply_cached_patch(workdir, &saved_staged);
        }
        if !saved_worktree_patch.is_empty()
            && let Err(e) = git_commands::apply_patch(workdir, &saved_worktree_patch)
            && warn_patch_err
        {
            eprintln!("Warning: could not restore working tree changes: {}", e);
        }
    };

    let mut fixup_pairs: Vec<(Oid, Oid)> = Vec::new();

    // Create fixup commits for whole-file groups
    for (target_oid, files) in &groups {
        let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        if let Err(e) = git_commit::stage_files(workdir, &file_refs) {
            rollback(false);
            return Err(e);
        }
        let msg = format!("fixup! absorb into {}", target_oid);
        if let Err(e) = git_commit::commit(workdir, &msg) {
            rollback(false);
            return Err(e);
        }
        let repo2 = Repository::discover(workdir)?;
        let fixup_oid = git::head_oid(&repo2)?;
        fixup_pairs.push((fixup_oid, *target_oid));
    }

    // Create fixup commits for hunk groups
    for (target_oid, file_hunks) in &hunk_groups {
        let mut combined_patch = String::new();
        for (path, hunks) in file_hunks {
            combined_patch.push_str(&build_hunk_patch(path, hunks));
        }
        if let Err(e) = git_commands::apply_cached_patch(workdir, &combined_patch) {
            rollback(false);
            return Err(e);
        }
        let msg = format!("fixup! absorb into {}", target_oid);
        if let Err(e) = git_commit::commit(workdir, &msg) {
            rollback(false);
            return Err(e);
        }
        let repo2 = Repository::discover(workdir)?;
        let fixup_oid = git::head_oid(&repo2)?;
        fixup_pairs.push((fixup_oid, *target_oid));
    }

    // Save diffs of skipped content and restore working tree before rebase
    let skipped_file_refs: Vec<&str> = skipped_files.iter().map(|f| f.as_str()).collect();
    let skipped_patch = if !skipped_file_refs.is_empty() {
        let dirty = git_commands::diff_head_name_only(workdir)?;
        if !dirty.trim().is_empty() {
            let mut diff_args: Vec<&str> = vec!["diff", "HEAD", "--"];
            diff_args.extend(skipped_file_refs.iter().copied());
            let full_patch = git_commands::run_git_stdout(workdir, &diff_args)?;
            let _ = git_commands::restore_files_to_head(workdir, &skipped_file_refs);
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
        graph.fixup_commit(*fixup_oid, *target_oid)?;
    }

    let todo = graph.to_todo();
    if let Err(e) = weave::run_rebase_or_abort(workdir, Some(&graph.base_oid.to_string()), &todo) {
        rollback(true);
        return Err(e);
    }

    // Restore pre-existing staged changes that were not part of the absorb.
    if !saved_staged.is_empty()
        && let Err(e) = git_commands::apply_cached_patch(workdir, &saved_staged)
    {
        eprintln!(
            "Warning: could not restore pre-existing staged changes: {}",
            e
        );
    }

    // Re-apply skipped changes to the working tree
    if let Some(ref patch) = skipped_patch
        && let Err(e) = git_commands::apply_patch(workdir, patch)
    {
        eprintln!("Warning: could not re-apply skipped changes: {}", e);
    }

    msg::success(&format!(
        "Absorbed {} hunk(s) from {} file(s) into {} commit(s)",
        num_hunks, num_files, num_commits
    ));

    Ok(())
}

/// Format a commit target label: `abc1234 "message" (branch)`.
fn commit_label(
    commit_oid: Oid,
    in_scope: &HashMap<Oid, &CommitInfo>,
    weave_graph: &Weave,
) -> String {
    let oid_str = commit_oid.to_string();
    let short = git_commands::short_hash(&oid_str);
    let message = in_scope
        .get(&commit_oid)
        .map(|c| c.message.as_str())
        .unwrap_or("");
    let branch_info = find_branch_for_commit(weave_graph, commit_oid)
        .map(|b| format!(" ({})", b))
        .unwrap_or_default();
    format!("{} \"{}\"{}", short, message, branch_info)
}

/// Print a whole-file assignment line.
fn print_assignment(
    file: &str,
    commit_oid: Oid,
    in_scope: &HashMap<Oid, &CommitInfo>,
    weave_graph: &Weave,
) {
    println!(
        "  {} -> {}",
        file,
        commit_label(commit_oid, in_scope, weave_graph)
    );
}

/// Print a hunk-level assignment line.
fn print_hunk_assignment(
    file: &str,
    hunk_num: usize,
    total: usize,
    commit_oid: Oid,
    in_scope: &HashMap<Oid, &CommitInfo>,
    weave_graph: &Weave,
) {
    println!(
        "  {} [hunk {}/{}] -> {}",
        file,
        hunk_num,
        total,
        commit_label(commit_oid, in_scope, weave_graph)
    );
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
            let path = resolve_file_arg(repo, arg)?;
            result.push(path);
        }
        Ok(result)
    }
}

/// Resolve a user argument to a file path using the centralized resolver.
fn resolve_file_arg(repo: &Repository, arg: &str) -> Result<String> {
    match git::resolve_arg(repo, arg, &[git::TargetKind::File])? {
        git::Target::File(path) => Ok(path),
        _ => unreachable!(),
    }
}

/// Analyze a single file to determine which commit(s) its hunks should be absorbed into.
fn analyze_file(
    repo: &Repository,
    workdir: &Path,
    path: &str,
    in_scope: &HashMap<Oid, &CommitInfo>,
) -> Result<FileAnalysis> {
    let diff = git_commands::diff_head_file(workdir, path)?;

    if diff.is_empty() {
        return Ok(FileAnalysis::Skipped {
            reason: "no changes".to_string(),
        });
    }

    if diff.contains("Binary files") {
        return Ok(FileAnalysis::Skipped {
            reason: "binary file".to_string(),
        });
    }

    let hunks = parse_hunks(&diff);

    if hunks.is_empty() {
        return Ok(FileAnalysis::Skipped {
            reason: "no hunks".to_string(),
        });
    }

    // Check if all hunks are pure additions
    if hunks.iter().all(|h| h.modified_lines.is_empty()) {
        return Ok(FileAnalysis::Skipped {
            reason: "pure addition".to_string(),
        });
    }

    let blame = match repo.blame_file(std::path::Path::new(path), None) {
        Ok(b) => b,
        Err(_) => {
            return Ok(FileAnalysis::Skipped {
                reason: "new file".to_string(),
            });
        }
    };

    let analyzed: Vec<(DiffHunk, HunkAnalysis)> = hunks
        .into_iter()
        .map(|h| {
            let analysis = analyze_hunk(&blame, &h, in_scope);
            (h, analysis)
        })
        .collect();

    // Collect unique assigned commits
    let assigned_oids: HashSet<Oid> = analyzed
        .iter()
        .filter_map(|(_, a)| match a {
            HunkAnalysis::Assigned { commit_oid } => Some(*commit_oid),
            _ => None,
        })
        .collect();

    if assigned_oids.is_empty() {
        // All hunks skipped — pick the first skip reason
        let reason = analyzed
            .first()
            .map(|(_, a)| match a {
                HunkAnalysis::Skipped { reason } => reason.clone(),
                _ => "unknown".to_string(),
            })
            .unwrap_or_else(|| "unknown".to_string());
        return Ok(FileAnalysis::Skipped { reason });
    }

    if assigned_oids.len() == 1 {
        let has_skipped = analyzed
            .iter()
            .any(|(_, a)| matches!(a, HunkAnalysis::Skipped { .. }));
        if !has_skipped {
            // All hunks go to the same commit — whole-file assignment
            return Ok(FileAnalysis::Assigned {
                commit_oid: *assigned_oids.iter().next().unwrap(),
            });
        }
    }

    // Multiple targets or mix of assigned/skipped — split
    Ok(FileAnalysis::Split { hunks: analyzed })
}

/// Analyze a single diff hunk to determine which commit it should be absorbed into.
fn analyze_hunk(
    blame: &git2::Blame<'_>,
    hunk: &DiffHunk,
    in_scope: &HashMap<Oid, &CommitInfo>,
) -> HunkAnalysis {
    if hunk.modified_lines.is_empty() {
        return HunkAnalysis::Skipped {
            reason: "pure addition".to_string(),
        };
    }

    let mut source_commits: HashSet<Oid> = HashSet::new();
    for &line_no in &hunk.modified_lines {
        if let Some(blame_hunk) = blame.get_line(line_no) {
            source_commits.insert(blame_hunk.final_commit_id());
        }
    }

    if source_commits.len() > 1 {
        return HunkAnalysis::Skipped {
            reason: "lines from multiple commits".to_string(),
        };
    }

    let commit_oid = match source_commits.into_iter().next() {
        Some(oid) => oid,
        None => {
            return HunkAnalysis::Skipped {
                reason: "no blame data".to_string(),
            };
        }
    };

    if !in_scope.contains_key(&commit_oid) {
        return HunkAnalysis::Skipped {
            reason: "out of scope".to_string(),
        };
    }

    HunkAnalysis::Assigned { commit_oid }
}

/// A single hunk extracted from a unified diff.
struct DiffHunk {
    /// The raw diff text for this hunk (starting with the @@ header, ending before the next hunk
    /// or EOF).
    text: String,
    /// Original (pre-image) line numbers of modified/deleted lines in this hunk.
    modified_lines: Vec<usize>,
}

/// Parse a unified diff into individual hunks.
///
/// Each hunk starts at an `@@ -start,count +start,count @@` header and extends
/// until the next hunk header or end of input. The file headers (`--- a/` / `+++ b/`)
/// are excluded from hunk text.
fn parse_hunks(diff: &str) -> Vec<DiffHunk> {
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut current_text = String::new();
    let mut current_modified: Vec<usize> = Vec::new();
    let mut current_orig_line: usize = 0;
    let mut in_hunk = false;

    for line in diff.lines() {
        if line.starts_with("@@ -") {
            // Save previous hunk if any
            if in_hunk {
                hunks.push(DiffHunk {
                    text: std::mem::take(&mut current_text),
                    modified_lines: std::mem::take(&mut current_modified),
                });
            }
            // Start new hunk
            current_text = format!("{}\n", line);
            current_modified = Vec::new();
            current_orig_line = parse_hunk_start(line).unwrap_or(1);
            in_hunk = true;
        } else if !in_hunk {
            // File header lines (--- a/, +++ b/, diff --git, etc.) — skip
            continue;
        } else if line.starts_with('-') {
            current_text.push_str(line);
            current_text.push('\n');
            current_modified.push(current_orig_line);
            current_orig_line += 1;
        } else if line.starts_with('+') {
            current_text.push_str(line);
            current_text.push('\n');
            // Added line — doesn't consume an original line number
        } else if line.starts_with('\\') {
            current_text.push_str(line);
            current_text.push('\n');
            // "\ No newline at end of file" — no line number change
        } else {
            // Context line
            current_text.push_str(line);
            current_text.push('\n');
            current_orig_line += 1;
        }
    }

    // Save last hunk
    if in_hunk {
        hunks.push(DiffHunk {
            text: current_text,
            modified_lines: current_modified,
        });
    }

    hunks
}

/// Parse a hunk header to extract the starting line number of the original side.
fn parse_hunk_start(line: &str) -> Option<usize> {
    let line = line.strip_prefix("@@ -")?;
    let end = line.find([',', ' '])?;
    line[..end].parse().ok()
}

/// Build a valid unified patch for `git apply` from selected hunks of a single file.
///
/// Produces a patch with one file header (`--- a/` / `+++ b/`) followed by
/// the raw text of each hunk (which includes the `@@` header).
fn build_hunk_patch(path: &str, hunks: &[DiffHunk]) -> String {
    let mut patch = String::new();
    patch.push_str(&format!("--- a/{}\n", path));
    patch.push_str(&format!("+++ b/{}\n", path));
    for hunk in hunks {
        patch.push_str(&hunk.text);
    }
    patch
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
