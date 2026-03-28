use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use git2::{Oid, Repository};
use serde::{Deserialize, Serialize};

use crate::core::diff::{DiffHunk, build_hunk_patch, parse_hunks};
use crate::core::msg;
use crate::core::repo::{self, CommitInfo, RepoInfo};
use crate::core::transaction::{self, LoomState, Rollback};
use crate::core::weave::{self, RebaseOutcome, Weave};
use crate::git;

struct PreRebaseState<'a> {
    saved_head: &'a str,
    saved_refs: &'a HashMap<String, git2::Oid>,
    saved_staged: &'a str,
    saved_worktree: &'a str,
}

#[derive(Serialize, Deserialize)]
struct AbsorbContext {
    skipped_patch: Option<String>,
    num_hunks: usize,
    num_files: usize,
    num_commits: usize,
}

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

/// Planned absorb: which files/hunks go to which commits.
struct AbsorbPlan {
    /// Whole-file assignments: (file_path, target_commit_oid).
    whole_file_assigned: Vec<(String, Oid)>,
    /// Hunk-level assignments: (file_path, target_commit_oid, hunks).
    hunk_assigned: Vec<(String, Oid, Vec<DiffHunk>)>,
    /// Files that could not be absorbed (saved and re-applied after rebase).
    skipped_files: Vec<String>,
    num_hunks: usize,
    num_files: usize,
    num_commits: usize,
}

/// Absorb working tree changes into the commits that last touched the affected lines.
pub fn run(dry_run: bool, user_files: Vec<String>) -> Result<()> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "absorb")?;
    let git_dir = repo.path().to_path_buf();
    let info = repo::gather_repo_info(&repo, false, 1)?;

    if info.commits.is_empty() {
        bail!("No commits in scope — nothing to absorb into");
    }

    let changed_files = get_changed_files(&repo, workdir, &user_files)?;
    if changed_files.is_empty() {
        bail!("Nothing to absorb — make some changes to tracked files first");
    }

    let weave_graph = Weave::from_repo_with_info(&repo, &info)?;
    let plan = build_plan(&repo, workdir, &info, &changed_files, &weave_graph)?;

    if dry_run {
        println!(
            "\nDry run: would absorb {} hunk(s) from {} file(s) into {} commit(s)",
            plan.num_hunks, plan.num_files, plan.num_commits
        );
        return Ok(());
    }

    apply_plan(&repo, workdir, &git_dir, plan)
}

/// Analyze changed files and build an absorb plan.
///
/// Prints each assignment as it's determined. Returns an error if nothing can be absorbed.
fn build_plan(
    repo: &Repository,
    workdir: &Path,
    info: &RepoInfo,
    changed_files: &[String],
    weave_graph: &Weave,
) -> Result<AbsorbPlan> {
    let in_scope: HashMap<Oid, &CommitInfo> = info.commits.iter().map(|c| (c.oid, c)).collect();

    let mut whole_file_assigned: Vec<(String, Oid)> = Vec::new();
    let mut hunk_assigned: Vec<(String, Oid, Vec<DiffHunk>)> = Vec::new();
    let mut skipped_files: Vec<String> = Vec::new();
    let mut any_assigned = false;

    for file in changed_files {
        match analyze_file(repo, workdir, file, &in_scope)? {
            FileAnalysis::Assigned { commit_oid } => {
                print_assignment(file, commit_oid, &in_scope, weave_graph);
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
                                weave_graph,
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

    let num_hunks: usize = whole_file_assigned.len()
        + hunk_assigned
            .iter()
            .map(|(_, _, hunks)| hunks.len())
            .sum::<usize>();
    let num_commits = whole_file_assigned
        .iter()
        .map(|(_, oid)| *oid)
        .chain(hunk_assigned.iter().map(|(_, oid, _)| *oid))
        .collect::<HashSet<Oid>>()
        .len();
    let num_files = whole_file_assigned
        .iter()
        .map(|(f, _)| f.as_str())
        .chain(hunk_assigned.iter().map(|(f, _, _)| f.as_str()))
        .collect::<HashSet<&str>>()
        .len();

    Ok(AbsorbPlan {
        whole_file_assigned,
        hunk_assigned,
        skipped_files,
        num_hunks,
        num_files,
        num_commits,
    })
}

/// Apply an absorb plan: create fixup commits and run the rebase.
fn apply_plan(repo: &Repository, workdir: &Path, git_dir: &Path, plan: AbsorbPlan) -> Result<()> {
    let saved_head = repo::head_oid(repo)?.to_string();
    let saved_refs = repo::snapshot_branch_refs(repo)?;

    // Group assignments by target commit.
    let mut groups: HashMap<Oid, Vec<String>> = HashMap::new();
    for (file, oid) in &plan.whole_file_assigned {
        groups.entry(*oid).or_default().push(file.clone());
    }
    let mut hunk_groups: HashMap<Oid, Vec<(String, Vec<DiffHunk>)>> = HashMap::new();
    for (path, oid, hunks) in plan.hunk_assigned {
        hunk_groups.entry(oid).or_default().push((path, hunks));
    }

    // Save staged files that are NOT being absorbed so they don't leak into fixup commits.
    let absorbed_files: HashSet<&str> = groups
        .values()
        .flatten()
        .map(|s| s.as_str())
        .chain(hunk_groups.values().flatten().map(|(p, _)| p.as_str()))
        .collect();
    let pre_staged = repo::get_staged_files(repo)?;
    let non_absorbed_staged: Vec<&str> = pre_staged
        .iter()
        .filter(|f| !absorbed_files.contains(f.as_str()))
        .map(|s| s.as_str())
        .collect();
    let saved_staged = if non_absorbed_staged.is_empty() {
        String::new()
    } else {
        let patch = git::diff_cached_files(workdir, &non_absorbed_staged)?;
        git::unstage_files(workdir, &non_absorbed_staged)?;
        patch
    };

    // Snapshot full working-tree diff before any mutations (needed for pre-rebase rollback).
    let saved_worktree = git::diff_head(workdir)?;

    // Create fixup commits; rolls back to pre-mutation state on any failure.
    let fixup_pairs = create_fixup_commits(
        repo,
        workdir,
        &groups,
        &hunk_groups,
        &PreRebaseState {
            saved_head: &saved_head,
            saved_refs: &saved_refs,
            saved_staged: &saved_staged,
            saved_worktree: &saved_worktree,
        },
    )?;

    // Save diffs for skipped files and restore their working-tree state before the rebase.
    let skipped_patch = save_skipped_patch(workdir, &plan.skipped_files)?;

    // Build Weave with the fixup commits.
    let repo2 = Repository::discover(workdir)?;
    let mut graph = Weave::from_repo(&repo2)?;
    for (fixup_oid, target_oid) in &fixup_pairs {
        graph.fixup_commit(*fixup_oid, *target_oid)?;
    }

    let ctx = AbsorbContext {
        skipped_patch: skipped_patch.clone(),
        num_hunks: plan.num_hunks,
        num_files: plan.num_files,
        num_commits: plan.num_commits,
    };
    let state = LoomState {
        command: "absorb".to_string(),
        rollback: Rollback {
            // reset_hard_to undoes the fixup commits created before the rebase.
            // git rebase --abort restores HEAD to after the fixup commits, not to
            // the original pre-absorb HEAD, so we must hard-reset to fully undo them.
            reset_hard_to: saved_head.to_string(),
            saved_staged_patch: saved_staged.clone(),
            saved_worktree_patch: saved_worktree.clone(),
            ..Default::default()
        },
        context: serde_json::to_value(&ctx)?,
    };
    transaction::save(git_dir, &state)?;

    let todo = graph.to_todo();
    match weave::run_rebase(workdir, Some(&graph.base_oid.to_string()), &todo)? {
        RebaseOutcome::Completed => {
            transaction::delete(git_dir)?;
            post_absorb(
                workdir,
                &saved_staged,
                skipped_patch.as_deref(),
                plan.num_hunks,
                plan.num_files,
                plan.num_commits,
            )?;
        }
        RebaseOutcome::Conflicted => {
            transaction::warn_conflict_paused("absorb");
        }
    }

    Ok(())
}

/// Create fixup commits for each whole-file and hunk-level assignment.
///
/// On any failure, rolls back to the pre-mutation state (reset hard, restore refs and patches)
/// before returning the error.
fn create_fixup_commits(
    repo: &Repository,
    workdir: &Path,
    groups: &HashMap<Oid, Vec<String>>,
    hunk_groups: &HashMap<Oid, Vec<(String, Vec<DiffHunk>)>>,
    pre_rebase: &PreRebaseState<'_>,
) -> Result<Vec<(Oid, Oid)>> {
    let mut fixup_pairs: Vec<(Oid, Oid)> = Vec::new();

    for (target_oid, files) in groups {
        let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        if let Err(e) = git::stage_files(workdir, &file_refs) {
            rollback_pre_rebase(workdir, pre_rebase);
            return Err(e);
        }
        let subject = repo
            .find_commit(*target_oid)
            .ok()
            .map(|c| repo::commit_subject(&c))
            .unwrap_or_else(|| target_oid.to_string());
        let msg = format!("fixup! {}", subject);
        if let Err(e) = git::commit(workdir, &msg) {
            rollback_pre_rebase(workdir, pre_rebase);
            return Err(e);
        }
        let fixup_hash = git::rev_parse(workdir, "HEAD")?;
        let fixup_oid = git2::Oid::from_str(&fixup_hash)?;
        fixup_pairs.push((fixup_oid, *target_oid));
    }

    for (target_oid, file_hunks) in hunk_groups {
        let mut combined_patch = String::new();
        for (path, hunks) in file_hunks {
            combined_patch.push_str(&build_hunk_patch(path, hunks));
        }
        if let Err(e) = git::apply_cached_patch(workdir, &combined_patch) {
            rollback_pre_rebase(workdir, pre_rebase);
            return Err(e);
        }
        let subject = repo
            .find_commit(*target_oid)
            .ok()
            .map(|c| repo::commit_subject(&c))
            .unwrap_or_else(|| target_oid.to_string());
        let msg = format!("fixup! {}", subject);
        if let Err(e) = git::commit(workdir, &msg) {
            rollback_pre_rebase(workdir, pre_rebase);
            return Err(e);
        }
        let fixup_hash = git::rev_parse(workdir, "HEAD")?;
        let fixup_oid = git2::Oid::from_str(&fixup_hash)?;
        fixup_pairs.push((fixup_oid, *target_oid));
    }

    Ok(fixup_pairs)
}

/// Save diffs for skipped files and restore their working-tree state before the rebase.
fn save_skipped_patch(workdir: &Path, skipped_files: &[String]) -> Result<Option<String>> {
    if skipped_files.is_empty() {
        return Ok(None);
    }
    let refs: Vec<&str> = skipped_files.iter().map(|f| f.as_str()).collect();
    let dirty = git::diff_head_name_only(workdir)?;
    if dirty.trim().is_empty() {
        return Ok(None);
    }
    let patch = git::diff_head_files(workdir, &refs)?;
    let _ = git::restore_files_to_head(workdir, &refs);
    Ok(Some(patch))
}

/// Roll back pre-rebase mutations: reset hard, restore refs and saved patches.
///
/// Used when a failure occurs during fixup commit creation, before any rebase has started.
/// This is distinct from `Rollback::apply_abort()`, which handles post-conflict abort and
/// trusts `git rebase --abort --update-refs` to restore branch refs.
fn rollback_pre_rebase(workdir: &Path, state: &PreRebaseState<'_>) {
    let _ = git::reset_hard(workdir, state.saved_head);
    let _ = repo::restore_branch_refs(workdir, state.saved_refs);
    if !state.saved_staged.is_empty() {
        let _ = git::apply_cached_patch(workdir, state.saved_staged);
    }
    if !state.saved_worktree.is_empty()
        && let Err(e) = git::apply_patch(workdir, state.saved_worktree)
    {
        eprintln!("Warning: could not restore working tree changes: {}", e);
    }
}

/// Resume an `absorb` operation after a conflict has been resolved.
pub fn after_continue(
    workdir: &Path,
    rollback: &Rollback,
    context: &serde_json::Value,
) -> Result<()> {
    let ctx: AbsorbContext =
        serde_json::from_value(context.clone()).context("Failed to parse absorb resume context")?;
    post_absorb(
        workdir,
        &rollback.saved_staged_patch,
        ctx.skipped_patch.as_deref(),
        ctx.num_hunks,
        ctx.num_files,
        ctx.num_commits,
    )
}

/// Post-rebase work: restore staged/skipped patches and print success message.
fn post_absorb(
    workdir: &Path,
    saved_staged: &str,
    skipped_patch: Option<&str>,
    num_hunks: usize,
    num_files: usize,
    num_commits: usize,
) -> Result<()> {
    git::restore_staged_patch(workdir, saved_staged)?;

    if let Some(patch) = skipped_patch
        && let Err(e) = git::apply_patch(workdir, patch)
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
    let short = git::short_hash(&oid_str);
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
        let output = git::diff_head_name_only(workdir)?;
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
    match repo::resolve_arg(repo, arg, &[repo::TargetKind::File])? {
        repo::Target::File(path) => Ok(path),
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
    if git::diff_head_file_is_binary(workdir, path)? {
        return Ok(FileAnalysis::Skipped {
            reason: "binary file".to_string(),
        });
    }

    let diff = git::diff_head_file(workdir, path)?;

    if diff.is_empty() {
        return Ok(FileAnalysis::Skipped {
            reason: "no changes".to_string(),
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
