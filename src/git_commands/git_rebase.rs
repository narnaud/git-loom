use std::io::Write;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use shell_escape::escape;

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
            }
        }

        // Serialize actions to JSON
        let actions_json = serde_json::to_string(&self.actions)?;

        // Build the sequence editor command with JSON-encoded actions
        // Convert backslashes to forward slashes for Git compatibility on Windows
        // Note: Git will automatically append the todo file path as the last argument
        let exe_str = self_exe.display().to_string().replace('\\', "/");
        let sequence_editor = format!(
            "{} internal-sequence-edit --actions-json {}",
            escape(exe_str.into()),
            escape(actions_json.into())
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
        &["rebase", "--onto", newbase, upstream, "--update-refs"],
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
        }
    }

    let mut output = lines.join("\n");
    output.push('\n');
    std::fs::write(todo_file, output)?;
    Ok(())
}

/// Replace `pick <hash>` with `edit <hash>`.
fn apply_edit(lines: &mut [String], short_hash: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut found = false;
    for line in lines.iter_mut() {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() >= 2 && parts[0] == "pick" && parts[1].starts_with(short_hash) {
            *line = format!("edit{}", &line["pick".len()..]);
            found = true;
            break;
        }
    }
    if !found {
        writeln!(
            std::io::stderr(),
            "warning: commit {} not found in rebase todo",
            short_hash
        )?;
    }
    Ok(())
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
/// With `--rebase-merges --update-refs`, the todo has `update-ref refs/heads/<branch>`
/// directives that control where branch refs end up after rebase. We insert the commit
/// just before the `update-ref` line so the branch ref will point to the moved commit.
/// Falls back to inserting before `label <branch>` if no `update-ref` is found.
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

    // Try to find `update-ref refs/heads/<branch>` first (preferred anchor)
    let update_ref_target = format!("update-ref refs/heads/{}", before_label);
    let insert_idx = lines
        .iter()
        .position(|line| line.trim() == update_ref_target)
        .or_else(|| {
            // Fall back to `label <branch>`
            let label_target = format!("label {}", before_label);
            lines.iter().position(|line| line.trim() == label_target)
        });

    let insert_idx = insert_idx.ok_or_else(|| {
        format!(
            "Branch '{}' not found in rebase todo. \
             The target branch may not be woven into the integration branch.",
            before_label
        )
    })?;

    lines.insert(insert_idx, commit_line);
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
