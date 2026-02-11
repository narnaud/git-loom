use std::io::Write;
use std::path::Path;
use std::process::Command;

use shell_escape::escape;

use super::loom_exe_path;

/// An action to apply during an interactive rebase.
pub enum RebaseAction {
    /// Mark a commit for editing.
    Edit { short_hash: String },
}

/// The target of a rebase operation.
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

        // Build the sequence editor command with action flags
        // Convert backslashes to forward slashes for Git compatibility on Windows
        let exe_str = self_exe.display().to_string().replace('\\', "/");
        let mut editor_parts = vec![escape(exe_str.into()).into_owned()];
        editor_parts.push("internal-sequence-edit".to_string());
        for action in &self.actions {
            match action {
                RebaseAction::Edit { short_hash } => {
                    editor_parts.push("--edit".to_string());
                    editor_parts.push(short_hash.clone());
                }
            }
        }

        let sequence_editor = editor_parts.join(" ");

        let mut cmd = Command::new("git");
        cmd.current_dir(self.workdir)
            .args([
                "rebase",
                "--interactive",
                "--autostash",
                "--keep-empty",
                "--no-autosquash",
                "--rebase-merges",
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

/// Abort an in-progress rebase.
pub fn abort(workdir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    super::run_git(workdir, &["rebase", "--abort"])
}

/// Apply rebase actions to a todo file (used as GIT_SEQUENCE_EDITOR).
///
/// For each `Edit` action, replaces the corresponding `pick <hash>` line
/// with `edit <hash>`.
pub fn apply_actions_to_todo(
    actions: &[RebaseAction],
    todo_file: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(todo_file)?;
    let mut output = String::with_capacity(content.len());

    // Collect hashes that need editing
    let mut edit_hashes: Vec<&str> = Vec::new();
    for action in actions {
        match action {
            RebaseAction::Edit { short_hash } => edit_hashes.push(short_hash),
        }
    }

    let mut found_count = 0;

    for line in content.lines() {
        let mut matched = false;
        for hash in &edit_hashes {
            if line.starts_with(&format!("pick {}", hash)) {
                output.push_str(&format!("edit {}", &line["pick".len()..]));
                matched = true;
                found_count += 1;
                break;
            }
        }
        if !matched {
            output.push_str(line);
        }
        output.push('\n');
    }

    if found_count < edit_hashes.len() {
        let missing: Vec<_> = edit_hashes
            .iter()
            .filter(|h| {
                !content
                    .lines()
                    .any(|l| l.starts_with(&format!("edit {}", h)))
            })
            .collect();
        if !missing.is_empty() {
            writeln!(
                std::io::stderr(),
                "warning: commit(s) {} not found in rebase todo",
                missing
                    .iter()
                    .map(|h| h.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
    }

    std::fs::write(todo_file, output)?;
    Ok(())
}
