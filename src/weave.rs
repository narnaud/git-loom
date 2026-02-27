use std::path::Path;

use anyhow::{Context, Result, bail};
use git2::{Oid, Repository};

use crate::git;
use crate::git_commands;

/// Command for a commit in the todo file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Pick,
    Edit,
    Fixup,
}

impl Command {
    fn as_str(&self) -> &str {
        match self {
            Command::Pick => "pick",
            Command::Edit => "edit",
            Command::Fixup => "fixup",
        }
    }
}

/// A commit in the todo file.
#[derive(Debug, Clone)]
pub struct CommitEntry {
    pub oid: Oid,
    pub short_hash: String,
    pub message: String,
    pub command: Command,
    /// Non-woven branch names at this commit, serialized as `update-ref` lines.
    pub update_refs: Vec<String>,
}

/// A woven branch section in the todo file.
#[derive(Debug, Clone)]
pub struct BranchSection {
    /// The reset target ("onto" or another branch label).
    pub reset_target: String,
    /// Commits in the section, oldest first.
    pub commits: Vec<CommitEntry>,
    /// Canonical label for the section (used in `label` and `merge` directives).
    pub label: String,
    /// All branch refs at this section's tip (co-located branches).
    pub branch_names: Vec<String>,
}

/// An entry on the integration (first-parent) line.
#[derive(Debug, Clone)]
pub enum IntegrationEntry {
    /// A regular commit on the integration line.
    Pick(CommitEntry),
    /// A merge point (weave) referencing a branch section.
    Merge {
        /// The original merge commit OID (None for newly created merges).
        original_oid: Option<Oid>,
        /// The label of the branch section being merged.
        label: String,
    },
}

/// The weave: a structured representation of the integration branch topology.
#[derive(Debug, Clone)]
pub struct Weave {
    /// The merge-base OID (the "onto" target).
    pub base_oid: Oid,
    /// Woven branch sections in dependency order.
    pub branch_sections: Vec<BranchSection>,
    /// The integration (first-parent) line entries.
    pub integration_line: Vec<IntegrationEntry>,
}

impl Weave {
    /// Serialize the weave to a git rebase todo file string.
    pub fn to_todo(&self) -> String {
        let mut out = String::new();

        // Start with label onto
        out.push_str("label onto\n");

        // Branch sections
        for section in &self.branch_sections {
            out.push('\n');
            out.push_str(&format!("reset {}\n", section.reset_target));
            // Defer update-ref lines past any trailing fixups so that the
            // ref points to the final (combined) result, not the pre-fixup hash.
            let mut pending_refs: Vec<String> = Vec::new();
            for commit in &section.commits {
                // A non-fixup command means any pending refs from the previous
                // pick are now finalised — emit them before the new pick.
                if commit.command != Command::Fixup && !pending_refs.is_empty() {
                    for r in pending_refs.drain(..) {
                        out.push_str(&format!("update-ref refs/heads/{}\n", r));
                    }
                }
                out.push_str(&format!(
                    "{} {} {}\n",
                    commit.command.as_str(),
                    commit.short_hash,
                    commit.message
                ));
                pending_refs.extend(commit.update_refs.iter().cloned());
            }
            // Emit any remaining refs (from the last pick/fixup sequence)
            for r in &pending_refs {
                out.push_str(&format!("update-ref refs/heads/{}\n", r));
            }
            out.push_str(&format!("label {}\n", section.label));
            for branch_name in &section.branch_names {
                out.push_str(&format!("update-ref refs/heads/{}\n", branch_name));
            }
        }

        // Integration line
        out.push('\n');
        out.push_str("reset onto\n");
        let mut pending_refs: Vec<String> = Vec::new();
        for entry in &self.integration_line {
            match entry {
                IntegrationEntry::Pick(commit) => {
                    if commit.command != Command::Fixup && !pending_refs.is_empty() {
                        for r in pending_refs.drain(..) {
                            out.push_str(&format!("update-ref refs/heads/{}\n", r));
                        }
                    }
                    out.push_str(&format!(
                        "{} {} {}\n",
                        commit.command.as_str(),
                        commit.short_hash,
                        commit.message
                    ));
                    pending_refs.extend(commit.update_refs.iter().cloned());
                }
                IntegrationEntry::Merge {
                    original_oid,
                    label,
                } => {
                    // Emit any pending refs before the merge
                    for r in pending_refs.drain(..) {
                        out.push_str(&format!("update-ref refs/heads/{}\n", r));
                    }
                    if let Some(oid) = original_oid {
                        out.push_str(&format!(
                            "merge -C {} {} # Merge branch '{}'\n",
                            git_commands::short_hash(&oid.to_string()),
                            label,
                            label
                        ));
                    } else {
                        out.push_str(&format!("merge {} # Merge branch '{}'\n", label, label));
                    }
                }
            }
        }
        // Emit any remaining refs from the integration line
        for r in &pending_refs {
            out.push_str(&format!("update-ref refs/heads/{}\n", r));
        }

        out
    }

    /// Build the weave from the current repository state.
    ///
    /// Walks the first-parent line from HEAD to the merge-base, collecting
    /// branch sections (from merge commits) and integration-line entries.
    pub fn from_repo(repo: &Repository) -> Result<Self> {
        let info = git::gather_repo_info(repo, false)?;
        let head_oid = git::head_oid(repo)?;
        let merge_base_oid = info.upstream.merge_base_oid;

        // Walk the first-parent line from HEAD to merge-base
        let first_parent_entries = walk_first_parent_line(repo, head_oid, merge_base_oid)?;

        // Build a set of all branch tips for matching
        let branch_tips: std::collections::HashMap<Oid, Vec<String>> = {
            let mut map: std::collections::HashMap<Oid, Vec<String>> =
                std::collections::HashMap::new();
            for branch in &info.branches {
                map.entry(branch.tip_oid)
                    .or_default()
                    .push(branch.name.clone());
            }
            map
        };

        let mut branch_sections = Vec::new();
        let mut integration_line = Vec::new();

        // Track which branch names have been assigned to sections
        let mut assigned_branches: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for entry in &first_parent_entries {
            if entry.is_merge {
                // This is a merge commit on the integration line.
                // The second parent leads to a branch.
                if let Some(merge_parent_oid) = entry.merge_parent {
                    // Find which branch this merge represents
                    let branch_names_at_tip = branch_tips
                        .get(&merge_parent_oid)
                        .cloned()
                        .unwrap_or_default();

                    // Walk the second parent backward to collect branch commits
                    let branch_commits =
                        walk_branch_commits(repo, merge_parent_oid, merge_base_oid)?;

                    if !branch_commits.is_empty() || !branch_names_at_tip.is_empty() {
                        // Determine the section label (use the first branch name, or generate one)
                        let label = if !branch_names_at_tip.is_empty() {
                            branch_names_at_tip[0].clone()
                        } else {
                            // No branch ref at the merge parent — use a generated label
                            format!(
                                "section-{}",
                                git_commands::short_hash(&merge_parent_oid.to_string())
                            )
                        };

                        // Convert to CommitEntry (oldest first)
                        let todo_commits: Vec<CommitEntry> = branch_commits
                            .into_iter()
                            .rev()
                            .map(|c| {
                                let mut update_refs = Vec::new();
                                // Check if any non-woven branches point at this commit
                                if let Some(names) = branch_tips.get(&c.oid) {
                                    for name in names {
                                        if !branch_names_at_tip.contains(name)
                                            && !assigned_branches.contains(name)
                                        {
                                            update_refs.push(name.clone());
                                        }
                                    }
                                }
                                CommitEntry {
                                    oid: c.oid,
                                    short_hash: c.short_hash,
                                    message: c.message,
                                    command: Command::Pick,
                                    update_refs,
                                }
                            })
                            .collect();

                        let section = BranchSection {
                            reset_target: "onto".to_string(),
                            commits: todo_commits,
                            label: label.clone(),
                            branch_names: branch_names_at_tip.clone(),
                        };

                        for name in &branch_names_at_tip {
                            assigned_branches.insert(name.clone());
                        }

                        branch_sections.push(section);

                        // Add merge entry on integration line
                        integration_line.push(IntegrationEntry::Merge {
                            original_oid: Some(entry.oid),
                            label,
                        });
                    }
                }
            } else {
                // Regular commit on the integration line
                let mut update_refs = Vec::new();
                // Check if any non-woven branches point at this commit
                if let Some(names) = branch_tips.get(&entry.oid) {
                    for name in names {
                        if !assigned_branches.contains(name) {
                            update_refs.push(name.clone());
                            assigned_branches.insert(name.clone());
                        }
                    }
                }

                integration_line.push(IntegrationEntry::Pick(CommitEntry {
                    oid: entry.oid,
                    short_hash: entry.short_hash.clone(),
                    message: entry.message.clone(),
                    command: Command::Pick,
                    update_refs,
                }));
            }
        }

        Ok(Weave {
            base_oid: merge_base_oid,
            branch_sections,
            integration_line,
        })
    }

    // ── Mutation methods ─────────────────────────────────────────────────

    /// Remove a commit from the graph.
    ///
    /// If the commit is in a branch section and is the last commit, the section
    /// and its merge entry are also removed.
    pub fn drop_commit(&mut self, oid: Oid) {
        // Check branch sections first
        for i in 0..self.branch_sections.len() {
            if let Some(pos) = self.branch_sections[i]
                .commits
                .iter()
                .position(|c| c.oid == oid)
            {
                self.branch_sections[i].commits.remove(pos);

                // If section is now empty, remove it and its merge
                if self.branch_sections[i].commits.is_empty() {
                    let label = self.branch_sections[i].label.clone();
                    self.branch_sections.remove(i);
                    self.integration_line.retain(
                        |e| !matches!(e, IntegrationEntry::Merge { label: l, .. } if *l == label),
                    );
                }
                return;
            }
        }

        // Check integration line
        self.integration_line
            .retain(|e| !matches!(e, IntegrationEntry::Pick(c) if c.oid == oid));
    }

    /// Remove an entire branch section and its merge entry.
    pub fn drop_branch(&mut self, branch_name: &str) {
        // Find and remove the section that has this branch name
        if let Some(idx) = self.branch_sections.iter().position(|s| {
            s.branch_names.contains(&branch_name.to_string()) || s.label == branch_name
        }) {
            let label = self.branch_sections[idx].label.clone();
            self.branch_sections.remove(idx);
            self.integration_line
                .retain(|e| !matches!(e, IntegrationEntry::Merge { label: l, .. } if *l == label));
        }
    }

    /// Move a commit to the tip of a branch section.
    ///
    /// If the target branch is co-located with other branches (multiple branch
    /// names in the same section), the section is split: original commits stay
    /// with the remaining branches, and a new stacked section is created for the
    /// target branch containing the moved commit.
    pub fn move_commit(&mut self, oid: Oid, to_branch: &str) {
        // Find and remove the commit from its current location
        let commit = self.remove_commit(oid);
        let Some(mut commit) = commit else { return };

        // Ensure command is Pick (not Fixup etc.)
        commit.command = Command::Pick;

        // Find the target branch section
        let section_idx = self
            .branch_sections
            .iter()
            .position(|s| s.label == to_branch || s.branch_names.contains(&to_branch.to_string()));
        let Some(section_idx) = section_idx else {
            return;
        };

        // If the target branch is co-located with others, split the section
        if self.branch_sections[section_idx].branch_names.len() > 1
            && self.branch_sections[section_idx]
                .branch_names
                .contains(&to_branch.to_string())
        {
            let old_label = self.branch_sections[section_idx].label.clone();

            // Remove the target branch from the original section
            self.branch_sections[section_idx]
                .branch_names
                .retain(|n| n != to_branch);

            // If the old label was the target, rename to a remaining branch name
            if old_label == to_branch
                && let Some(first_remaining) =
                    self.branch_sections[section_idx].branch_names.first()
            {
                self.branch_sections[section_idx].label = first_remaining.clone();
            }

            let base_label = self.branch_sections[section_idx].label.clone();

            // Create a stacked section for the target branch
            let new_section = BranchSection {
                reset_target: base_label,
                commits: vec![commit],
                label: to_branch.to_string(),
                branch_names: vec![to_branch.to_string()],
            };
            self.branch_sections.insert(section_idx + 1, new_section);

            // Update the merge entry to reference the outermost (stacked) section
            for entry in &mut self.integration_line {
                if let IntegrationEntry::Merge { label, .. } = entry
                    && *label == old_label
                {
                    *label = to_branch.to_string();
                }
            }
        } else {
            // Simple case: only one branch in the section, just append
            self.branch_sections[section_idx].commits.push(commit);
        }
    }

    /// Change the source commit to Fixup and move it right after the target.
    pub fn fixup_commit(&mut self, source_oid: Oid, target_oid: Oid) {
        let commit = self.remove_commit(source_oid);
        let Some(mut commit) = commit else { return };
        commit.command = Command::Fixup;

        // Find the target commit and insert the fixup after it
        for section in &mut self.branch_sections {
            if let Some(pos) = section.commits.iter().position(|c| c.oid == target_oid) {
                section.commits.insert(pos + 1, commit);
                return;
            }
        }

        // Check integration line
        for (i, entry) in self.integration_line.iter().enumerate() {
            if let IntegrationEntry::Pick(c) = entry
                && c.oid == target_oid
            {
                self.integration_line
                    .insert(i + 1, IntegrationEntry::Pick(commit));
                return;
            }
        }
    }

    /// Change a commit's command to Edit.
    pub fn edit_commit(&mut self, oid: Oid) {
        self.set_command(oid, Command::Edit);
    }

    /// Add a new branch section to the graph.
    pub fn add_branch_section(
        &mut self,
        label: String,
        branch_names: Vec<String>,
        commits: Vec<CommitEntry>,
        reset_target: String,
    ) {
        self.branch_sections.push(BranchSection {
            reset_target,
            commits,
            label,
            branch_names,
        });
    }

    /// Add a merge entry on the integration line.
    ///
    /// If `position` is `None`, appends at the end.
    /// If `position` is `Some(idx)`, inserts at that index.
    pub fn add_merge(&mut self, label: String, original_oid: Option<Oid>, position: Option<usize>) {
        let entry = IntegrationEntry::Merge {
            original_oid,
            label,
        };
        match position {
            Some(idx) => self.integration_line.insert(idx, entry),
            None => self.integration_line.push(entry),
        }
    }

    /// Weave a non-woven branch into the integration line.
    ///
    /// Moves all integration line picks from the start up to (and including) the
    /// branch tip into a new branch section, and adds a merge entry at the end.
    pub fn weave_branch(&mut self, branch_name: &str) {
        // Find which integration line Pick has this branch in update_refs
        let branch_idx = self.integration_line.iter().position(|e| {
            matches!(e, IntegrationEntry::Pick(c) if c.update_refs.contains(&branch_name.to_string()))
        });

        let Some(branch_idx) = branch_idx else {
            return;
        };

        // Collect all Pick entries from 0..=branch_idx into the branch section
        let mut section_commits = Vec::new();
        let mut indices_to_remove = Vec::new();

        for i in 0..=branch_idx {
            if let IntegrationEntry::Pick(commit) = &self.integration_line[i] {
                let mut commit = commit.clone();
                // Remove this branch from the commit's update_refs
                commit.update_refs.retain(|r| r != branch_name);
                section_commits.push(commit);
                indices_to_remove.push(i);
            }
        }

        // Remove from integration line in reverse order to preserve indices
        for &i in indices_to_remove.iter().rev() {
            self.integration_line.remove(i);
        }

        // Create the branch section
        self.branch_sections.push(BranchSection {
            reset_target: "onto".to_string(),
            commits: section_commits,
            label: branch_name.to_string(),
            branch_names: vec![branch_name.to_string()],
        });

        // Add merge at the end of the integration line
        self.integration_line.push(IntegrationEntry::Merge {
            original_oid: None,
            label: branch_name.to_string(),
        });
    }

    /// Reassign a branch section from one branch name to another.
    ///
    /// Renames the section's label and merge line, removes the dropped branch
    /// from branch_names, ensures the keep branch is present.
    pub fn reassign_branch(&mut self, drop_branch: &str, keep_branch: &str) {
        if let Some(section) = self
            .branch_sections
            .iter_mut()
            .find(|s| s.label == drop_branch || s.branch_names.contains(&drop_branch.to_string()))
        {
            let old_label = section.label.clone();

            // If the section was labeled with the drop branch, rename the label
            if section.label == drop_branch {
                section.label = keep_branch.to_string();
            }

            // Remove the drop branch from branch_names
            section.branch_names.retain(|n| n != drop_branch);

            // Ensure the keep branch is in branch_names
            if !section.branch_names.contains(&keep_branch.to_string()) {
                section.branch_names.push(keep_branch.to_string());
            }

            // Update the merge entry label
            let new_label = section.label.clone();
            for entry in &mut self.integration_line {
                if let IntegrationEntry::Merge { label, .. } = entry
                    && *label == old_label
                {
                    *label = new_label.clone();
                }
            }
        }
    }

    // ── Private helpers ──────────────────────────────────────────────────

    /// Remove a commit from wherever it is in the graph, returning it.
    fn remove_commit(&mut self, oid: Oid) -> Option<CommitEntry> {
        // Check branch sections
        for section in &mut self.branch_sections {
            if let Some(pos) = section.commits.iter().position(|c| c.oid == oid) {
                return Some(section.commits.remove(pos));
            }
        }

        // Check integration line
        let idx = self
            .integration_line
            .iter()
            .position(|e| matches!(e, IntegrationEntry::Pick(c) if c.oid == oid));
        if let Some(i) = idx
            && let IntegrationEntry::Pick(commit) = self.integration_line.remove(i)
        {
            return Some(commit);
        }

        None
    }

    /// Set the command for a commit in the graph.
    fn set_command(&mut self, oid: Oid, command: Command) {
        for section in &mut self.branch_sections {
            for commit in &mut section.commits {
                if commit.oid == oid {
                    commit.command = command;
                    return;
                }
            }
        }

        for entry in &mut self.integration_line {
            if let IntegrationEntry::Pick(commit) = entry
                && commit.oid == oid
            {
                commit.command = command;
                return;
            }
        }
    }
}

/// An entry from the first-parent walk of the integration branch.
#[derive(Debug)]
struct FirstParentEntry {
    oid: Oid,
    short_hash: String,
    message: String,
    is_merge: bool,
    /// For merge commits: the second parent (branch being merged).
    merge_parent: Option<Oid>,
}

/// Walk the first-parent line from `head` to `stop` (exclusive).
///
/// Returns entries in oldest-first order (reversed from the walk direction).
/// Includes both regular and merge commits.
fn walk_first_parent_line(
    repo: &Repository,
    head: Oid,
    stop: Oid,
) -> Result<Vec<FirstParentEntry>> {
    let mut entries = Vec::new();
    let mut current = head;

    while current != stop {
        let commit = repo.find_commit(current)?;

        let short_hash = commit
            .as_object()
            .short_id()?
            .as_str()
            .context("short_id is not valid UTF-8")?
            .to_string();
        let message = commit.summary().unwrap_or("").to_string();

        let is_merge = commit.parent_count() > 1;
        let merge_parent = if is_merge {
            Some(commit.parent_id(1)?)
        } else {
            None
        };

        entries.push(FirstParentEntry {
            oid: current,
            short_hash,
            message,
            is_merge,
            merge_parent,
        });

        // Follow first parent
        current = match commit.parent_id(0) {
            Ok(oid) => oid,
            Err(_) => break, // reached root
        };
    }

    // Reverse to oldest-first
    entries.reverse();
    Ok(entries)
}

/// Walk branch commits from `tip` back to `stop` (exclusive), skipping merges.
///
/// Returns entries in newest-first order (like a revwalk).
fn walk_branch_commits(repo: &Repository, tip: Oid, stop: Oid) -> Result<Vec<BranchCommitEntry>> {
    let mut entries = Vec::new();
    let mut current = tip;

    while current != stop {
        let commit = repo.find_commit(current)?;

        // Skip merge commits
        if commit.parent_count() <= 1 {
            let short_hash = commit
                .as_object()
                .short_id()?
                .as_str()
                .context("short_id is not valid UTF-8")?
                .to_string();
            let message = commit.summary().unwrap_or("").to_string();

            entries.push(BranchCommitEntry {
                oid: current,
                short_hash,
                message,
            });
        }

        // Follow first parent
        current = match commit.parent_id(0) {
            Ok(oid) => oid,
            Err(_) => break,
        };
    }

    Ok(entries)
}

#[derive(Debug)]
struct BranchCommitEntry {
    oid: Oid,
    short_hash: String,
    message: String,
}

/// Execute a weave-based rebase.
///
/// Writes the todo content to a temp file and runs git rebase with
/// `internal-write-todo` as the sequence editor.
///
/// `upstream` is the OID to use as the upstream for the rebase. Commits after
/// this OID (exclusive) up to HEAD are rebased. This is passed directly as the
/// `<upstream>` argument to `git rebase`, NOT with a `^` suffix. For root
/// commits, pass `None` to use `--root`.
pub fn run_rebase(workdir: &Path, upstream: Option<&str>, todo_content: &str) -> Result<()> {
    use std::io::Write;
    use std::process::Command;

    let self_exe = git_commands::loom_exe_path()?;

    // Write todo content to a temp file
    let mut temp_file = tempfile::NamedTempFile::new()?;
    temp_file.write_all(todo_content.as_bytes())?;
    temp_file.flush()?;
    let temp_path = temp_file.into_temp_path();

    // Build the sequence editor command
    let exe_str = self_exe.display().to_string().replace('\\', "/");
    let source_path = temp_path.display().to_string().replace('\\', "/");

    let sequence_editor = format!(
        "{} internal-write-todo --source {} ",
        shell_escape::unix::escape(exe_str.into()),
        shell_escape::unix::escape(source_path.into()),
    );

    let mut cmd = Command::new("git");
    cmd.current_dir(workdir)
        .args([
            "rebase",
            "--interactive",
            "--autostash",
            "--keep-empty",
            "--no-autosquash",
            "--rebase-merges",
            "--update-refs",
        ])
        .env("GIT_SEQUENCE_EDITOR", sequence_editor)
        // Suppress editor for new merge commits (those without -C in the todo).
        // `true` is a no-op that leaves the default "Merge branch '...'" message intact.
        // This only affects the rebase process — not the user's shell when rebase
        // pauses at an `edit` command.
        .env("GIT_EDITOR", "true");

    match upstream {
        Some(oid) => {
            cmd.arg(oid);
        }
        None => {
            cmd.arg("--root");
        }
    }

    let output = cmd.output()?;
    if !output.status.success() {
        let _ = git_commands::git_rebase::abort(workdir);
        bail!("Rebase failed with conflicts — aborted");
    }

    // Clean up the temp file
    let _ = temp_path.close();

    Ok(())
}

#[cfg(test)]
#[path = "weave_test.rs"]
mod tests;
