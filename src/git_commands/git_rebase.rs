use std::io::Write;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use shell_escape::unix::escape as unix_escape;

use super::loom_exe_path;

/// An action to apply during an interactive rebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RebaseAction {
    /// Mark a commit for editing.
    Edit { short_hash: String },
    /// Move source commit right after target and mark it as fixup.
    Fixup {
        source_hash: String,
        target_hash: String,
    },
    /// Move a commit to just before a label (i.e., to the tip of a branch section).
    Move {
        commit_hash: String,
        before_label: String,
    },
    /// Remove a commit from history entirely.
    Drop { short_hash: String },
    /// Remove an entire woven branch section and its merge line from history.
    DropBranch { branch_name: String },
    /// Reassign a woven branch section to a co-located branch.
    ///
    /// When dropping a woven branch that shares its tip with another branch,
    /// the section's label and merge line are renamed to the co-located branch
    /// instead of being removed. This keeps the commits for the surviving branch.
    ReassignBranch {
        drop_branch: String,
        keep_branch: String,
    },
}

/// The target of a rebase operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RebaseTarget {
    /// Rebase onto a specific commit (uses `<hash>^` as the base).
    Commit(String),
    /// Rebase the entire history (uses `--root`).
    Root,
}

/// Builder for running an interactive rebase with custom actions.
pub struct Rebase<'a> {
    workdir: &'a Path,
    target: RebaseTarget,
    actions: Vec<RebaseAction>,
}

impl<'a> Rebase<'a> {
    pub fn new(workdir: &'a Path, target: RebaseTarget) -> Self {
        Self {
            workdir,
            target,
            actions: Vec::new(),
        }
    }

    pub fn action(mut self, action: RebaseAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Start the interactive rebase, using git-loom as the sequence editor.
    /// If the rebase fails, automatically aborts to clean up any partial state.
    pub fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let self_exe = loom_exe_path()?;

        // Validate all hashes before building the command
        for action in &self.actions {
            match action {
                RebaseAction::Edit { short_hash } => {
                    validate_hex(short_hash)?;
                }
                RebaseAction::Fixup {
                    source_hash,
                    target_hash,
                } => {
                    validate_hex(source_hash)?;
                    validate_hex(target_hash)?;
                }
                RebaseAction::Move { commit_hash, .. } => {
                    validate_hex(commit_hash)?;
                }
                RebaseAction::Drop { short_hash } => {
                    validate_hex(short_hash)?;
                }
                RebaseAction::DropBranch { .. } | RebaseAction::ReassignBranch { .. } => {
                    // Branch names don't need hex validation
                }
            }
        }

        // Serialize actions to JSON
        let actions_json = serde_json::to_string(&self.actions)?;

        // Build the sequence editor command with JSON-encoded actions
        // Convert backslashes to forward slashes for Git compatibility on Windows
        // Note: Git will automatically append the todo file path as the last argument
        let exe_str = self_exe.display().to_string().replace('\\', "/");

        // Use platform-appropriate shell escaping.
        // On Windows, Git for Windows uses MSYS2/bash to execute GIT_SEQUENCE_EDITOR,
        // so we still use Unix-style escaping there. The windows_escape variant is
        // reserved for contexts where cmd.exe or PowerShell are the shell.
        let sequence_editor = format!(
            "{} internal-sequence-edit --actions-json {}",
            unix_escape(exe_str.into()),
            unix_escape(actions_json.into())
        );

        let mut cmd = Command::new("git");
        cmd.current_dir(self.workdir)
            .args([
                "rebase",
                "--interactive",
                "--autostash",
                "--keep-empty",
                "--no-autosquash",
                "--rebase-merges",
                "--update-refs",
            ])
            .env("GIT_SEQUENCE_EDITOR", sequence_editor);

        match &self.target {
            RebaseTarget::Root => {
                cmd.arg("--root");
            }
            RebaseTarget::Commit(hash) => {
                cmd.arg(format!("{}^", hash));
            }
        }

        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = abort(self.workdir);
            return Err(format!("Git rebase failed to start:\n{}", stderr).into());
        }

        Ok(())
    }
}

/// Continue an in-progress rebase.
/// If continuation fails, automatically aborts the rebase.
pub fn continue_rebase(workdir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = super::run_git(workdir, &["rebase", "--continue"]) {
        let _ = abort(workdir);
        return Err(format!("Git rebase --continue failed. Rebase aborted:\n{}", e).into());
    }
    Ok(())
}

/// Rebase commits between `upstream` and HEAD onto `newbase`.
///
/// Runs `git rebase --onto <newbase> <upstream> --update-refs`.
/// The `--update-refs` flag keeps any branch refs in the rebased range up to date.
pub fn rebase_onto(
    workdir: &Path,
    newbase: &str,
    upstream: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    super::run_git(
        workdir,
        &[
            "rebase",
            "--onto",
            newbase,
            upstream,
            "--autostash",
            "--update-refs",
        ],
    )
}

/// Abort an in-progress rebase.
pub fn abort(workdir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    super::run_git(workdir, &["rebase", "--abort"])
}

/// Apply rebase actions to a todo file (used as GIT_SEQUENCE_EDITOR).
///
/// Supports multiple action types:
/// - `Edit`: replaces `pick <hash>` with `edit <hash>`
/// - `Fixup`: moves source commit after target and marks it as `fixup`
/// - `Move`: moves a commit line to just before a `label` directive
pub fn apply_actions_to_todo(
    actions: &[RebaseAction],
    todo_file: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(todo_file)?;
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    for action in actions {
        match action {
            RebaseAction::Edit { short_hash } => {
                apply_edit(&mut lines, short_hash)?;
            }
            RebaseAction::Fixup {
                source_hash,
                target_hash,
            } => {
                apply_fixup(&mut lines, source_hash, target_hash)?;
            }
            RebaseAction::Move {
                commit_hash,
                before_label,
            } => {
                apply_move(&mut lines, commit_hash, before_label)?;
            }
            RebaseAction::Drop { short_hash } => {
                apply_drop(&mut lines, short_hash)?;
            }
            RebaseAction::DropBranch { branch_name } => {
                apply_drop_branch(&mut lines, branch_name)?;
            }
            RebaseAction::ReassignBranch {
                drop_branch,
                keep_branch,
            } => {
                apply_reassign_branch(&mut lines, drop_branch, keep_branch)?;
            }
        }
    }

    let mut output = lines.join("\n");
    output.push('\n');
    std::fs::write(todo_file, output)?;
    Ok(())
}

/// Replace `pick <hash>` with `edit <hash>`.
fn apply_edit(lines: &mut [String], short_hash: &str) -> Result<(), Box<dyn std::error::Error>> {
    for line in lines.iter_mut() {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() >= 2 && parts[0] == "pick" && parts[1].starts_with(short_hash) {
            *line = format!("edit{}", &line["pick".len()..]);
            return Ok(());
        }
    }
    Err(format!("Commit {} not found in rebase todo", short_hash).into())
}

/// Move the source commit line right after the target commit line and mark it as `fixup`.
fn apply_fixup(
    lines: &mut Vec<String>,
    source_hash: &str,
    target_hash: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Find and remove the source line
    let source_idx = lines.iter().position(|line| {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        parts.len() >= 2 && parts[0] == "pick" && parts[1].starts_with(source_hash)
    });
    let source_idx = source_idx
        .ok_or_else(|| format!("Source commit {} not found in rebase todo", source_hash))?;
    let source_line = lines.remove(source_idx);

    // Change pick to fixup
    let fixup_line = format!("fixup{}", &source_line["pick".len()..]);

    // Find the target line and insert after it
    let target_idx = lines.iter().position(|line| {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        parts.len() >= 2 && parts[0] == "pick" && parts[1].starts_with(target_hash)
    });
    let target_idx = target_idx
        .ok_or_else(|| format!("Target commit {} not found in rebase todo", target_hash))?;

    lines.insert(target_idx + 1, fixup_line);
    Ok(())
}

/// Move a commit line to the tip of a branch's section in the rebase todo.
///
/// With `--rebase-merges --update-refs`, each branch section ends with a block of
/// `update-ref` and `label` directives (possibly separated by blank lines):
///
/// ```text
/// pick abc feat2
/// update-ref refs/heads/test3
///
/// update-ref refs/heads/test2
///
/// label test3
/// ```
///
/// The `label` defines the merge parent (used by `merge -C ... test3`).
/// The `update-ref` lines update branch pointers.
///
/// When inserting a commit at the tip of a branch, we must ensure:
/// - The `label` and target's `update-ref` come AFTER the inserted commit
///   (so the merge includes the commit and the branch pointer advances).
/// - Other branches' `update-ref` lines stay BEFORE the inserted commit
///   (so co-located branches don't accidentally advance).
fn apply_move(
    lines: &mut Vec<String>,
    commit_hash: &str,
    before_label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Find and remove the commit line
    let commit_idx = lines.iter().position(|line| {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        parts.len() >= 2 && parts[0] == "pick" && parts[1].starts_with(commit_hash)
    });
    let commit_idx =
        commit_idx.ok_or_else(|| format!("Commit {} not found in rebase todo", commit_hash))?;
    let commit_line = lines.remove(commit_idx);

    let label_target = format!("label {}", before_label);
    let update_ref_target = format!("update-ref refs/heads/{}", before_label);

    let label_pos = lines.iter().position(|l| l.trim() == label_target);
    let update_ref_pos = lines.iter().position(|l| l.trim() == update_ref_target);

    if label_pos.is_none() && update_ref_pos.is_none() {
        return Err(format!(
            "Branch '{}' not found in rebase todo. \
             The target branch may not be woven into the integration branch.",
            before_label
        )
        .into());
    }

    // Determine the furthest anchor position (label or update-ref).
    let anchor_end = match (label_pos, update_ref_pos) {
        (Some(lp), Some(up)) => lp.max(up),
        (Some(lp), None) => lp,
        (None, Some(up)) => up,
        _ => unreachable!(),
    };

    // Walk backwards from anchor_end to find the start of the block.
    // The block includes update-ref lines, the target's label, and blank lines
    // that separate them. Stop at any other line type (pick, reset, merge, etc.).
    let block_start = {
        let mut start = anchor_end;
        while start > 0 {
            let trimmed = lines[start - 1].trim();
            if trimmed.is_empty() || trimmed.starts_with("update-ref ") || trimmed == label_target {
                start -= 1;
            } else {
                break;
            }
        }
        start
    };

    // Walk forwards from anchor_end to capture trailing blank lines.
    let block_end = {
        let mut end = anchor_end;
        while end + 1 < lines.len() && lines[end + 1].trim().is_empty() {
            end += 1;
        }
        end
    };

    // Extract the entire block
    let block: Vec<String> = lines.drain(block_start..=block_end).collect();

    // Categorize lines (skip blanks)
    let mut other_refs = Vec::new();
    let mut label_line = None;
    let mut target_ref = None;
    for line in block {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        } else if trimmed == label_target {
            label_line = Some(line);
        } else if trimmed == update_ref_target {
            target_ref = Some(line);
        } else {
            other_refs.push(line);
        }
    }

    // Re-insert: [other refs] [commit] [label] [target ref]
    let mut pos = block_start;
    for ref_line in other_refs {
        lines.insert(pos, ref_line);
        pos += 1;
    }
    lines.insert(pos, commit_line);
    pos += 1;
    if let Some(ll) = label_line {
        lines.insert(pos, ll);
        pos += 1;
    }
    if let Some(tr) = target_ref {
        lines.insert(pos, tr);
    }

    Ok(())
}

/// Remove a `pick <hash>` line entirely from the todo.
fn apply_drop(lines: &mut Vec<String>, short_hash: &str) -> Result<(), Box<dyn std::error::Error>> {
    let idx = lines.iter().position(|line| {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        parts.len() >= 2 && parts[0] == "pick" && parts[1].starts_with(short_hash)
    });
    match idx {
        Some(i) => {
            lines.remove(i);
            Ok(())
        }
        None => Err(format!("Commit {} not found in rebase todo", short_hash).into()),
    }
}

/// Remove an entire woven branch section and its merge line from the rebase todo.
///
/// With `--rebase-merges --update-refs`, a woven branch section looks like:
/// ```text
/// reset onto
/// pick <hash> commit message
/// pick <hash> another commit
/// label <branch>
/// update-ref refs/heads/<branch>
/// ...
/// merge -C <hash> <branch>
/// ```
///
/// This function removes:
/// 1. The `reset` line that opens the branch section
/// 2. All `pick` lines in the section
/// 3. The `label <branch>` line
/// 4. The `update-ref refs/heads/<branch>` line
/// 5. The `merge ... <branch>` line
fn apply_drop_branch(
    lines: &mut Vec<String>,
    branch_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let label_target = format!("label {}", branch_name);

    // Find the label line for this branch
    let label_idx = lines.iter().position(|l| l.trim() == label_target);
    let label_idx = match label_idx {
        Some(i) => i,
        None => {
            writeln!(
                std::io::stderr(),
                "warning: branch '{}' label not found in rebase todo; \
                 branch may not be woven",
                branch_name
            )?;
            return Ok(());
        }
    };

    // Walk backwards from label_idx to find the `reset` line that opens this section.
    // Stop at `reset` lines only. If we encounter another structural marker
    // (label, update-ref, merge) before finding a `reset`, stop there to avoid
    // including lines from a preceding section.
    let section_start = {
        let mut i = label_idx;
        let mut found_reset = None;
        while i > 0 {
            i -= 1;
            let trimmed = lines[i].trim();
            if trimmed.starts_with("reset ") {
                found_reset = Some(i);
                break;
            }
            // Stop at any structural marker that belongs to a different section
            if trimmed.starts_with("label ")
                || trimmed.starts_with("update-ref ")
                || trimmed.starts_with("merge ")
            {
                break;
            }
        }
        // If no reset found, start after the last structural marker we stopped at
        found_reset.unwrap_or(i + 1)
    };

    // Check if the line after the label is an update-ref for this branch
    let update_ref_target = format!("update-ref refs/heads/{}", branch_name);
    let section_end = if lines
        .get(label_idx + 1)
        .is_some_and(|l| l.trim() == update_ref_target)
    {
        label_idx + 1 // inclusive: remove up to and including the update-ref
    } else {
        label_idx // inclusive: remove up to and including the label
    };

    // Collect indices to remove: branch section + merge line
    let mut to_remove: Vec<usize> = (section_start..=section_end).collect();

    // Find the `merge ... <branch_name>` line.
    // Format: `merge [-C|-c] <hash> <label> [# comment]`
    // The label is the 4th whitespace-delimited token.
    let merge_idx = lines.iter().position(|l| {
        let parts: Vec<&str> = l.split_whitespace().collect();
        parts.len() >= 4 && parts[0] == "merge" && parts[3] == branch_name
    });
    match merge_idx {
        Some(i) => to_remove.push(i),
        None => {
            writeln!(
                std::io::stderr(),
                "warning: merge line for branch '{}' not found in rebase todo",
                branch_name
            )?;
        }
    }

    // Remove in descending order so indices stay valid
    to_remove.sort_unstable();
    to_remove.dedup();
    for i in to_remove.into_iter().rev() {
        lines.remove(i);
    }

    Ok(())
}

/// Reassign a woven branch section to a co-located branch.
///
/// When two branches share the same tip and one is dropped, the section's
/// `label` and `merge` lines are renamed to the surviving branch. The
/// dropped branch's `update-ref` is removed while the surviving branch's
/// `update-ref` is preserved.
///
/// Transforms:
/// ```text
/// reset onto
/// pick abc1234 A1
/// label drop-branch
/// update-ref refs/heads/drop-branch
/// update-ref refs/heads/keep-branch
/// ...
/// merge -C xxx drop-branch
/// ```
/// Into:
/// ```text
/// reset onto
/// pick abc1234 A1
/// label keep-branch
/// update-ref refs/heads/keep-branch
/// ...
/// merge -C xxx keep-branch
/// ```
fn apply_reassign_branch(
    lines: &mut Vec<String>,
    drop_branch: &str,
    keep_branch: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let label_target = format!("label {}", drop_branch);
    let new_label = format!("label {}", keep_branch);
    let drop_update_ref = format!("update-ref refs/heads/{}", drop_branch);

    // Rename the label
    if let Some(line) = lines.iter_mut().find(|l| l.trim() == label_target) {
        *line = new_label;
    }

    // Remove the dropped branch's update-ref
    lines.retain(|l| l.trim() != drop_update_ref);

    // Rename the merge line: `merge -C <hash> drop-branch` → `merge -C <hash> keep-branch`
    // Format: `merge [-C|-c] <hash> <label> [# comment]`
    for line in lines.iter_mut() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 && parts[0] == "merge" && parts[3] == drop_branch {
            // Reconstruct: keep first 3 tokens, replace the 4th, append the rest
            let rest: String = if parts.len() > 4 {
                format!(" {}", parts[4..].join(" "))
            } else {
                String::new()
            };
            *line = format!(
                "{} {} {} {}{}",
                parts[0], parts[1], parts[2], keep_branch, rest
            );
            break;
        }
    }

    Ok(())
}

/// Validate that a string contains only hexadecimal characters.
fn validate_hex(s: &str) -> Result<(), Box<dyn std::error::Error>> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "Invalid commit hash: '{}' (expected hex characters only)",
            s
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_drop_removes_pick_line() {
        let mut lines = vec![
            "pick abc1234 First".to_string(),
            "pick def5678 Second".to_string(),
            "pick 9876543 Third".to_string(),
        ];
        apply_drop(&mut lines, "def5678").unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("First"));
        assert!(lines[1].contains("Third"));
    }

    #[test]
    fn apply_drop_branch_removes_section_and_merge() {
        let mut lines = vec![
            "label onto".to_string(),
            "".to_string(),
            "reset onto".to_string(),
            "pick abc1234 A1".to_string(),
            "pick def5678 A2".to_string(),
            "label feature-a".to_string(),
            "update-ref refs/heads/feature-a".to_string(),
            "".to_string(),
            "reset onto".to_string(),
            "pick 1111111 Int".to_string(),
            "merge -C 9999999 feature-a # Merge branch 'feature-a'".to_string(),
            "update-ref refs/heads/integration".to_string(),
        ];

        apply_drop_branch(&mut lines, "feature-a").unwrap();

        let remaining: Vec<&str> = lines.iter().map(|l| l.as_str()).collect();
        // Branch section (reset, picks, label, update-ref) and merge line should be gone
        assert!(
            !remaining.iter().any(|l| l.contains("A1")),
            "A1 should be removed"
        );
        assert!(
            !remaining.iter().any(|l| l.contains("A2")),
            "A2 should be removed"
        );
        assert!(
            !remaining.iter().any(|l| l.contains("label feature-a")),
            "label should be removed"
        );
        assert!(
            !remaining
                .iter()
                .any(|l| l.contains("update-ref refs/heads/feature-a")),
            "update-ref should be removed"
        );
        assert!(
            !remaining
                .iter()
                .any(|l| l.contains("merge") && l.contains("feature-a")),
            "merge line should be removed"
        );
        // These should remain
        assert!(
            remaining.contains(&"label onto"),
            "label onto should remain"
        );
        assert!(
            remaining.iter().any(|l| l.contains("Int")),
            "Int should remain"
        );
        assert!(
            remaining
                .iter()
                .any(|l| l.contains("update-ref refs/heads/integration")),
            "integration update-ref should remain"
        );
    }

    #[test]
    fn apply_move_inserts_before_label_and_update_ref() {
        let mut lines = vec![
            "pick abc1234 Feat1".to_string(),
            "update-ref refs/heads/test".to_string(),
            "label test".to_string(),
            "pick def5678 Feat3".to_string(),
        ];
        apply_move(&mut lines, "def5678", "test").unwrap();
        assert_eq!(
            lines,
            vec![
                "pick abc1234 Feat1",
                "pick def5678 Feat3",
                "label test",
                "update-ref refs/heads/test",
            ]
        );
    }

    #[test]
    fn apply_move_colocated_target_after_other() {
        // test2's update-ref comes before test3's — already correct order
        let mut lines = vec![
            "pick abc1234 feat2".to_string(),
            "update-ref refs/heads/test2".to_string(),
            "update-ref refs/heads/test3".to_string(),
            "label test3".to_string(),
            "pick def5678 Feat3".to_string(),
        ];
        apply_move(&mut lines, "def5678", "test3").unwrap();
        assert_eq!(
            lines,
            vec![
                "pick abc1234 feat2",
                "update-ref refs/heads/test2",
                "pick def5678 Feat3",
                "label test3",
                "update-ref refs/heads/test3",
            ]
        );
    }

    #[test]
    fn apply_move_colocated_target_before_other() {
        // test3's update-ref comes before test2's — needs reordering
        let mut lines = vec![
            "pick abc1234 feat2".to_string(),
            "update-ref refs/heads/test3".to_string(),
            "update-ref refs/heads/test2".to_string(),
            "label test3".to_string(),
            "pick def5678 Feat3".to_string(),
        ];
        apply_move(&mut lines, "def5678", "test3").unwrap();
        // test2 must stay before the commit, test3 after it
        assert_eq!(
            lines,
            vec![
                "pick abc1234 feat2",
                "update-ref refs/heads/test2",
                "pick def5678 Feat3",
                "label test3",
                "update-ref refs/heads/test3",
            ]
        );
    }

    #[test]
    fn apply_move_three_colocated_branches() {
        // Three co-located branches, target in the middle
        let mut lines = vec![
            "pick abc1234 feat2".to_string(),
            "update-ref refs/heads/test".to_string(),
            "update-ref refs/heads/test3".to_string(),
            "update-ref refs/heads/test2".to_string(),
            "label test3".to_string(),
            "pick def5678 Feat3".to_string(),
        ];
        apply_move(&mut lines, "def5678", "test3").unwrap();
        // test and test2 must stay before the commit, test3 after it
        assert_eq!(
            lines,
            vec![
                "pick abc1234 feat2",
                "update-ref refs/heads/test",
                "update-ref refs/heads/test2",
                "pick def5678 Feat3",
                "label test3",
                "update-ref refs/heads/test3",
            ]
        );
    }

    #[test]
    fn apply_move_colocated_with_blank_lines() {
        // Real git format: blank lines separate update-ref directives
        let mut lines = vec![
            "pick abc1234 feat2".to_string(),
            "update-ref refs/heads/test3".to_string(),
            "".to_string(),
            "update-ref refs/heads/test2".to_string(),
            "".to_string(),
            "label test3".to_string(),
            "".to_string(),
            "reset branch-point".to_string(),
            "pick def5678 Feat3".to_string(),
        ];
        apply_move(&mut lines, "def5678", "test3").unwrap();
        // test2 must stay before the commit, blank lines are stripped
        assert_eq!(
            lines,
            vec![
                "pick abc1234 feat2",
                "update-ref refs/heads/test2",
                "pick def5678 Feat3",
                "label test3",
                "update-ref refs/heads/test3",
                "reset branch-point",
            ]
        );
    }

    #[test]
    fn apply_reassign_branch_renames_section() {
        let mut lines = vec![
            "label onto".to_string(),
            "".to_string(),
            "reset onto".to_string(),
            "pick abc1234 A1".to_string(),
            "label feature-a".to_string(),
            "update-ref refs/heads/feature-a".to_string(),
            "update-ref refs/heads/feature-b".to_string(),
            "".to_string(),
            "reset onto".to_string(),
            "pick 1111111 Int".to_string(),
            "merge -C 9999999 feature-a # Merge branch 'feature-a'".to_string(),
            "update-ref refs/heads/integration".to_string(),
        ];

        apply_reassign_branch(&mut lines, "feature-a", "feature-b").unwrap();

        let remaining: Vec<&str> = lines.iter().map(|l| l.as_str()).collect();
        // Label should be renamed to feature-b
        assert!(
            remaining.contains(&"label feature-b"),
            "label should be renamed to feature-b"
        );
        // feature-a's update-ref should be removed
        assert!(
            !remaining
                .iter()
                .any(|l| l.contains("update-ref refs/heads/feature-a")),
            "feature-a update-ref should be removed"
        );
        // feature-b's update-ref should remain
        assert!(
            remaining
                .iter()
                .any(|l| l.contains("update-ref refs/heads/feature-b")),
            "feature-b update-ref should remain"
        );
        // Merge should reference feature-b now
        assert!(
            remaining
                .iter()
                .any(|l| l.contains("merge") && l.contains("feature-b")),
            "merge should reference feature-b"
        );
        // Commits should remain
        assert!(
            remaining.iter().any(|l| l.contains("A1")),
            "A1 should remain"
        );
        assert!(
            remaining.iter().any(|l| l.contains("Int")),
            "Int should remain"
        );
    }

    #[test]
    fn apply_drop_branch_with_no_blank_lines() {
        // Test without blank lines between sections
        let mut lines = vec![
            "label onto".to_string(),
            "reset onto".to_string(),
            "pick abc A1".to_string(),
            "label feature-a".to_string(),
            "update-ref refs/heads/feature-a".to_string(),
            "reset onto".to_string(),
            "pick def Int".to_string(),
            "merge -C xxx feature-a".to_string(),
            "update-ref refs/heads/integration".to_string(),
        ];

        apply_drop_branch(&mut lines, "feature-a").unwrap();

        let remaining: Vec<&str> = lines.iter().map(|l| l.as_str()).collect();
        assert!(!remaining.iter().any(|l| l.contains("A1")));
        assert!(remaining.iter().any(|l| l.contains("Int")));
        assert_eq!(remaining.len(), 4); // label onto, reset onto, pick Int, update-ref integration
    }
}
