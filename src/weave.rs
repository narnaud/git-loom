use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use git2::{Oid, Repository};

use crate::git;
use crate::git_commands;
use crate::msg;

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
            let remaining = emit_commits_with_refs(&mut out, &section.commits);
            flush_refs(&mut out, &remaining);
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
                        flush_refs(&mut out, &pending_refs);
                        pending_refs.clear();
                    }
                    out.push_str(&format!(
                        "{} {} # {}\n",
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
                    flush_refs(&mut out, &pending_refs);
                    pending_refs.clear();
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
        flush_refs(&mut out, &pending_refs);

        out
    }

    /// Build the weave from the current repository state.
    ///
    /// Convenience wrapper that calls `gather_repo_info` internally.
    /// When `RepoInfo` is already available, prefer `from_repo_with_info`
    /// to avoid a redundant graph walk.
    pub fn from_repo(repo: &Repository) -> Result<Self> {
        let info = git::gather_repo_info(repo, false, 1)?;
        Self::from_repo_with_info(repo, &info)
    }

    /// Build the weave from a pre-gathered `RepoInfo`.
    ///
    /// Walks the first-parent line from HEAD to the merge-base, collecting
    /// branch sections (from merge commits) and integration-line entries.
    pub fn from_repo_with_info(repo: &Repository, info: &git::RepoInfo) -> Result<Self> {
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

    /// Remove branch-section commits that are already in the new upstream
    /// (merged or cherry-picked). Empty sections and their merges are removed.
    ///
    /// Uses two strategies:
    /// 1. Exact OID ancestry (commit was directly merged)
    /// 2. `git cherry` for cherry-pick detection (only when candidates remain)
    pub fn filter_upstream_commits(
        &mut self,
        repo: &Repository,
        workdir: &Path,
        new_upstream_oid: Oid,
    ) -> Result<()> {
        // Strategy 1: exact ancestor check (fast, no processes)
        let mut candidates: Vec<Oid> = Vec::new();
        let mut to_drop = Vec::new();
        for section in &self.branch_sections {
            for commit in &section.commits {
                match repo.graph_descendant_of(new_upstream_oid, commit.oid) {
                    Ok(true) => to_drop.push(commit.oid),
                    Ok(false) => candidates.push(commit.oid),
                    Err(e) => {
                        return Err(e.into());
                    }
                }
            }
        }

        // Strategy 2: git cherry for remaining candidates (O(feature commits), not O(upstream commits))
        if !candidates.is_empty() {
            let candidate_set: HashSet<Oid> = candidates.into_iter().collect();
            match cherry_pick_equivalents(workdir, &new_upstream_oid, &self.base_oid) {
                Some(equivalent) => {
                    to_drop.extend(equivalent.intersection(&candidate_set).copied());
                }
                None => {
                    msg::warn(
                        "Could not run git cherry — \
                         cherry-picked commits may not be detected",
                    );
                }
            }
        }

        for oid in to_drop {
            self.drop_commit(oid);
        }
        Ok(())
    }

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
                let removed = self.branch_sections[i].commits.remove(pos);

                // Transfer update_refs to an adjacent commit
                if !removed.update_refs.is_empty() && !self.branch_sections[i].commits.is_empty() {
                    let target_pos = if pos > 0 { pos - 1 } else { 0 };
                    self.branch_sections[i].commits[target_pos]
                        .update_refs
                        .extend(removed.update_refs);
                }

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
        if let Some(pos) = self
            .integration_line
            .iter()
            .position(|e| matches!(e, IntegrationEntry::Pick(c) if c.oid == oid))
            && let IntegrationEntry::Pick(removed) = self.integration_line.remove(pos)
            && !removed.update_refs.is_empty()
        {
            // Find the nearest adjacent Pick to transfer refs to
            let target = (pos..self.integration_line.len())
                .chain((0..pos).rev())
                .find(|&j| matches!(self.integration_line[j], IntegrationEntry::Pick(_)));
            if let Some(j) = target
                && let IntegrationEntry::Pick(ref mut c) = self.integration_line[j]
            {
                c.update_refs.extend(removed.update_refs);
            }
        }
    }

    /// Remove an entire branch section and its merge entry.
    pub fn drop_branch(&mut self, branch_name: &str) {
        // Find the section that has this branch name
        let Some(idx) = self.branch_sections.iter().position(|s| {
            s.branch_names.contains(&branch_name.to_string()) || s.label == branch_name
        }) else {
            return;
        };

        let old_label = self.branch_sections[idx].label.clone();

        // Check if an inner branch exists in this section's update_refs.
        // In a stacked topology (e.g. feat1 stacked under feat2), commits
        // belonging to the inner branch have update_refs marking their tip.
        let inner_branch_boundary = self.branch_sections[idx]
            .commits
            .iter()
            .rposition(|c| !c.update_refs.is_empty());

        if let Some(boundary) = inner_branch_boundary {
            // Keep commits up to and including the inner branch boundary.
            // The remaining commits (after the boundary) belong to the
            // dropped branch.
            let inner_ref = self.branch_sections[idx].commits[boundary]
                .update_refs
                .first()
                .cloned()
                .unwrap();
            self.branch_sections[idx].commits.truncate(boundary + 1);
            self.branch_sections[idx].label = inner_ref.clone();
            self.branch_sections[idx].branch_names = vec![inner_ref.clone()];
            // Remove only the chosen inner branch from update_refs; preserve
            // any other co-located refs at this commit.
            let inner = inner_ref.clone();
            self.branch_sections[idx].commits[boundary]
                .update_refs
                .retain(|r| *r != inner);

            // Update the merge entry to reference the inner branch
            for entry in &mut self.integration_line {
                if let IntegrationEntry::Merge {
                    label,
                    original_oid,
                } = entry
                    && *label == old_label
                {
                    *label = inner_ref.clone();
                    *original_oid = None;
                }
            }
        } else {
            // No inner branches — remove the entire section and its merge
            self.branch_sections.remove(idx);
            self.integration_line.retain(
                |e| !matches!(e, IntegrationEntry::Merge { label: l, .. } if *l == old_label),
            );
        }
    }

    /// Move a commit to the tip of a branch section.
    ///
    /// If the target branch is co-located with other branches (multiple branch
    /// names in the same section), the section is split: original commits stay
    /// with the remaining branches, and a new stacked section is created for the
    /// target branch containing the moved commit.
    pub fn move_commit(&mut self, oid: Oid, to_branch: &str) -> anyhow::Result<()> {
        // Validate target section exists BEFORE removing the source
        let section_idx = self
            .branch_sections
            .iter()
            .position(|s| s.label == to_branch || s.branch_names.contains(&to_branch.to_string()));
        let Some(section_idx) = section_idx else {
            anyhow::bail!(
                "Cannot move commit: target branch section '{}' not found in weave graph",
                to_branch
            );
        };

        // Find and remove the commit from its current location
        let commit = self.remove_commit(oid);
        let Some(mut commit) = commit else {
            anyhow::bail!(
                "Cannot move commit: source commit {} not found in weave graph",
                oid
            );
        };

        // Ensure command is Pick (not Fixup etc.)
        commit.command = Command::Pick;

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

            // Update the merge entry to reference the outermost (stacked) section.
            // Clear original_oid so the rebase generates a fresh merge message
            // with the new branch name (instead of reusing "Merge branch '<old>'").
            for entry in &mut self.integration_line {
                if let IntegrationEntry::Merge {
                    label,
                    original_oid,
                } = entry
                    && *label == old_label
                {
                    *label = to_branch.to_string();
                    *original_oid = None;
                }
            }
        } else {
            // Simple case: only one branch in the section, just append
            self.branch_sections[section_idx].commits.push(commit);
        }
        Ok(())
    }

    /// Change the source commit to Fixup and move it right after the target.
    pub fn fixup_commit(&mut self, source_oid: Oid, target_oid: Oid) -> anyhow::Result<()> {
        // Validate target exists BEFORE removing the source
        let target_in_sections = self
            .branch_sections
            .iter()
            .any(|s| s.commits.iter().any(|c| c.oid == target_oid));
        let target_in_integration = self
            .integration_line
            .iter()
            .any(|entry| matches!(entry, IntegrationEntry::Pick(c) if c.oid == target_oid));
        if !target_in_sections && !target_in_integration {
            anyhow::bail!(
                "Cannot fixup commit: target commit {} not found in weave graph",
                target_oid
            );
        }

        let commit = self.remove_commit(source_oid);
        let Some(mut commit) = commit else {
            anyhow::bail!(
                "Cannot fixup commit: source commit {} not found in weave graph",
                source_oid
            );
        };
        commit.command = Command::Fixup;

        // Find the target commit and insert the fixup after it
        for section in &mut self.branch_sections {
            if let Some(pos) = section.commits.iter().position(|c| c.oid == target_oid) {
                section.commits.insert(pos + 1, commit);
                return Ok(());
            }
        }

        // Check integration line
        for (i, entry) in self.integration_line.iter().enumerate() {
            if let IntegrationEntry::Pick(c) = entry
                && c.oid == target_oid
            {
                self.integration_line
                    .insert(i + 1, IntegrationEntry::Pick(commit));
                return Ok(());
            }
        }

        // Should be unreachable given the pre-validation, but be safe
        anyhow::bail!(
            "Cannot fixup commit: target commit {} disappeared during operation",
            target_oid
        )
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
    /// If `position` is `None`, inserts before any loose `Pick` entries (after
    /// all existing `Merge` entries), so the new branch sits below loose commits
    /// in the resulting history.
    /// If `position` is `Some(idx)`, inserts at that exact index.
    pub fn add_merge(&mut self, label: String, original_oid: Option<Oid>, position: Option<usize>) {
        let entry = IntegrationEntry::Merge {
            original_oid,
            label,
        };
        let idx = position.unwrap_or_else(|| {
            self.integration_line
                .iter()
                .position(|e| matches!(e, IntegrationEntry::Pick(_)))
                .unwrap_or(self.integration_line.len())
        });
        self.integration_line.insert(idx, entry);
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

        // Collect all Pick entries from 0..=branch_idx into the branch section.
        // Also count existing Merge entries in that range — the new merge will be
        // inserted right after them, before any loose Picks that follow branch_idx.
        let mut section_commits = Vec::new();
        let mut indices_to_remove = Vec::new();
        let mut insert_pos = 0;

        for i in 0..=branch_idx {
            if let IntegrationEntry::Pick(commit) = &self.integration_line[i] {
                let mut commit = commit.clone();
                // Remove this branch from the commit's update_refs
                commit.update_refs.retain(|r| r != branch_name);
                section_commits.push(commit);
                indices_to_remove.push(i);
            } else {
                // A Merge entry in this range stays; the new merge goes after it.
                insert_pos += 1;
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

        // Insert merge before any loose commits that follow the branch tip,
        // so those commits sit on top of the merge in the resulting history.
        self.integration_line.insert(
            insert_pos,
            IntegrationEntry::Merge {
                original_oid: None,
                label: branch_name.to_string(),
            },
        );
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

            // Update the merge entry label and clear original_oid so the
            // rebase generates a fresh merge message with the new branch name.
            let new_label = section.label.clone();
            for entry in &mut self.integration_line {
                if let IntegrationEntry::Merge {
                    label,
                    original_oid,
                } = entry
                    && *label == old_label
                {
                    *label = new_label.clone();
                    *original_oid = None;
                }
            }
        }
    }

    /// Swap two commits within the same sequence.
    ///
    /// Both commits must be in the same container: either the same branch section
    /// or both on the integration line. Returns an error if they are in different
    /// locations or if either commit is not found.
    pub fn swap_commits(&mut self, oid_a: Oid, oid_b: Oid) -> Result<()> {
        if oid_a == oid_b {
            bail!("Cannot swap a commit with itself");
        }

        // Locate both commits in branch sections
        let mut sec_a: Option<(usize, usize)> = None;
        let mut sec_b: Option<(usize, usize)> = None;
        for (si, section) in self.branch_sections.iter().enumerate() {
            for (pi, commit) in section.commits.iter().enumerate() {
                if commit.oid == oid_a {
                    sec_a = Some((si, pi));
                }
                if commit.oid == oid_b {
                    sec_b = Some((si, pi));
                }
            }
        }

        // Locate both commits on the integration line
        let mut int_a: Option<usize> = None;
        let mut int_b: Option<usize> = None;
        for (i, entry) in self.integration_line.iter().enumerate() {
            if let IntegrationEntry::Pick(c) = entry {
                if c.oid == oid_a {
                    int_a = Some(i);
                }
                if c.oid == oid_b {
                    int_b = Some(i);
                }
            }
        }

        match (sec_a, sec_b, int_a, int_b) {
            (Some((si_a, pi_a)), Some((si_b, pi_b)), _, _) if si_a == si_b => {
                self.branch_sections[si_a].commits.swap(pi_a, pi_b);
                Ok(())
            }
            (Some(_), Some(_), _, _) => {
                bail!("Cannot swap commits from different branch sections")
            }
            (None, None, Some(i), Some(j)) => {
                self.integration_line.swap(i, j);
                Ok(())
            }
            _ => {
                if sec_a.is_none() && int_a.is_none() {
                    bail!("Commit {} not found in weave graph", oid_a)
                } else if sec_b.is_none() && int_b.is_none() {
                    bail!("Commit {} not found in weave graph", oid_b)
                } else {
                    bail!(
                        "Cannot swap commits from different locations (branch section vs integration line)"
                    )
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

    /// Add a branch name to a commit's `update_refs` so that `--update-refs`
    /// keeps the ref in sync through the rebase.  Used to track a commit's new
    /// OID after a fixup rebase (where the commit is rewritten).
    pub fn track_commit(&mut self, oid: Oid, ref_name: &str) {
        for section in &mut self.branch_sections {
            for commit in &mut section.commits {
                if commit.oid == oid {
                    commit.update_refs.push(ref_name.to_string());
                    return;
                }
            }
        }

        for entry in &mut self.integration_line {
            if let IntegrationEntry::Pick(commit) = entry
                && commit.oid == oid
            {
                commit.update_refs.push(ref_name.to_string());
                return;
            }
        }
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

/// Emit commit lines and collect pending update-ref entries.
///
/// Returns any refs that are still pending after the last commit
/// (i.e. refs attached to the final pick/fixup sequence).
fn emit_commits_with_refs(out: &mut String, commits: &[CommitEntry]) -> Vec<String> {
    let mut pending_refs: Vec<String> = Vec::new();
    for commit in commits {
        if commit.command != Command::Fixup && !pending_refs.is_empty() {
            flush_refs(out, &pending_refs);
            pending_refs.clear();
        }
        out.push_str(&format!(
            "{} {} # {}\n",
            commit.command.as_str(),
            commit.short_hash,
            commit.message
        ));
        pending_refs.extend(commit.update_refs.iter().cloned());
    }
    pending_refs
}

/// Emit update-ref lines for accumulated pending refs.
fn flush_refs(out: &mut String, refs: &[String]) {
    for r in refs {
        out.push_str(&format!("update-ref refs/heads/{}\n", r));
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
///
/// For merge commits with inverted parent ordering (feature branch as first
/// parent instead of the integration line), the parents are swapped so that
/// `merge_parent` always points to the branch side and the walk continues
/// along the line that leads back to `stop`.
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

        if is_merge {
            let p0 = commit.parent_id(0)?;
            let p1 = commit.parent_id(1)?;

            // Determine which parent leads back to `stop` (the integration
            // line) and which is the branch side. Loom merges always have
            // p0 = integration, p1 = branch, but upstream merges may have
            // inverted parent ordering.
            let (continue_parent, branch_parent) =
                if p0 == stop || repo.graph_descendant_of(p0, stop).unwrap_or(false) {
                    // Normal: first parent leads to stop
                    (p0, p1)
                } else if p1 == stop || repo.graph_descendant_of(p1, stop).unwrap_or(false) {
                    // Inverted: second parent leads to stop, swap
                    (p1, p0)
                } else {
                    bail!(
                        "Neither parent of merge {} leads to merge-base {}",
                        current,
                        stop
                    );
                };

            entries.push(FirstParentEntry {
                oid: current,
                short_hash,
                message,
                is_merge,
                merge_parent: Some(branch_parent),
            });

            current = continue_parent;
        } else {
            entries.push(FirstParentEntry {
                oid: current,
                short_hash,
                message,
                is_merge,
                merge_parent: None,
            });

            // Follow first parent
            current = match commit.parent_id(0) {
                Ok(oid) => oid,
                Err(_) => {
                    bail!(
                        "First-parent walk from {} did not reach merge-base {}",
                        head,
                        stop
                    );
                }
            };
        }
    }

    // Reverse to oldest-first
    entries.reverse();
    Ok(entries)
}

/// Walk branch commits from `tip` back to `stop` (exclusive), skipping merges.
///
/// Returns entries in newest-first order (like a revwalk).
///
/// When the branch was forked from before `stop` (e.g. an upstream merge with
/// inverted parent ordering), the actual fork point is computed via
/// `merge_base(tip, stop)` and used as the stop instead.
fn walk_branch_commits(repo: &Repository, tip: Oid, stop: Oid) -> Result<Vec<BranchCommitEntry>> {
    // For loom-woven branches, tip descends from stop (the branch was forked
    // from the merge base). For branches forked earlier, compute the actual
    // fork point so we don't walk past it into shared history.
    let actual_stop = if tip == stop {
        stop
    } else {
        repo.merge_base(tip, stop).unwrap_or(stop)
    };

    let mut entries = Vec::new();
    let mut current = tip;

    while current != actual_stop {
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

/// Start an interactive rebase that pauses at the given commit (edit command).
///
/// Tries `Weave::from_repo()` first for full topology-aware rebase on
/// integration branches. Falls back to a minimal linear todo for non-integration
/// repos (no upstream tracking).
pub fn start_edit_rebase(repo: &Repository, workdir: &Path, commit_oid: Oid) -> Result<()> {
    // Try Weave::from_repo first (for integration branches)
    if let Ok(mut graph) = Weave::from_repo(repo) {
        graph.edit_commit(commit_oid);
        let todo = graph.to_todo();
        return run_rebase_or_abort(workdir, Some(&graph.base_oid.to_string()), &todo);
    }

    // Fallback: build a minimal linear todo for non-integration repos
    build_and_run_linear_edit(repo, workdir, commit_oid)
}

/// Build a linear todo and run rebase for a commit range containing the target.
///
/// Used for non-integration repos where `Weave::from_repo()` is not available.
/// Walks the first-parent line from HEAD to the target's parent (or root).
fn build_and_run_linear_edit(repo: &Repository, workdir: &Path, commit_oid: Oid) -> Result<()> {
    let head_oid = git::head_oid(repo)?;
    let commit = repo.find_commit(commit_oid)?;

    // Determine upstream (parent of target, or --root for root commits)
    let upstream: Option<String> = if commit.parent_count() > 0 {
        Some(commit.parent_id(0)?.to_string())
    } else {
        None
    };

    let stop = upstream.as_ref().and_then(|s| Oid::from_str(s).ok());

    // Walk from HEAD backward, collecting commits in the range
    let mut entries = Vec::new();
    let mut current = head_oid;

    loop {
        if Some(current) == stop {
            break;
        }

        let c = repo.find_commit(current)?;
        let short = c
            .as_object()
            .short_id()?
            .as_str()
            .context("Short ID is not valid UTF-8")?
            .to_string();
        let msg = c.summary().unwrap_or("").to_string();
        let cmd = if current == commit_oid {
            "edit"
        } else {
            "pick"
        };
        entries.push(format!("{} {} # {}", cmd, short, msg));

        if c.parent_count() == 0 {
            break;
        }
        current = c.parent_id(0)?;
    }

    entries.reverse(); // oldest first

    // Build the todo string
    let mut todo = String::from("label onto\n\nreset onto\n");
    for line in &entries {
        todo.push_str(line);
        todo.push('\n');
    }

    run_rebase_or_abort(workdir, upstream.as_deref(), &todo)
}

/// Outcome of a weave-based rebase.
pub use crate::git_commands::git_rebase::RebaseOutcome;

/// Execute a weave-based rebase, aborting automatically on conflict.
///
/// This is the legacy wrapper for out-of-scope callers (`reword`, `split`,
/// and excluded `fold` paths). Use `run_rebase` directly for resumable commands.
pub fn run_rebase_or_abort(
    workdir: &Path,
    upstream: Option<&str>,
    todo_content: &str,
) -> Result<()> {
    match run_rebase(workdir, upstream, todo_content)? {
        RebaseOutcome::Completed => Ok(()),
        RebaseOutcome::Conflicted => {
            let _ = git_commands::git_rebase::abort(workdir);
            bail!("Rebase failed with conflicts — aborted");
        }
    }
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
///
/// Returns `RebaseOutcome::Completed` on success, `RebaseOutcome::Conflicted`
/// if the rebase stopped due to a conflict. Does NOT abort on conflict.
pub fn run_rebase(
    workdir: &Path,
    upstream: Option<&str>,
    todo_content: &str,
) -> Result<RebaseOutcome> {
    use std::io::Write;
    use std::process::Command;
    use std::time::Instant;

    use crate::trace as loom_trace;

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

    // Build args string for logging
    let upstream_arg = upstream.unwrap_or("--root");
    let log_args = format!(
        "rebase --interactive --autostash --keep-empty --empty=drop --no-autosquash --rebase-merges --update-refs {}",
        upstream_arg
    );

    let mut cmd = Command::new("git");
    cmd.current_dir(workdir)
        .args([
            "rebase",
            "--interactive",
            "--autostash",
            "--keep-empty",
            "--empty=drop",
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

    let start = Instant::now();
    let output = cmd.output()?;
    let duration_ms = start.elapsed().as_millis();
    let stderr = String::from_utf8_lossy(&output.stderr);
    loom_trace::log_command(
        "git",
        &log_args,
        duration_ms,
        output.status.success(),
        &stderr,
    );
    // Read the original git todo from sidecar file (if handle_write_todo saved it)
    let sidecar = temp_path.with_extension("original");
    if let Ok(original_todo) = std::fs::read_to_string(&sidecar) {
        let filtered: String = original_todo
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        loom_trace::annotate("original git todo", &filtered);
        let _ = std::fs::remove_file(&sidecar);
    }

    loom_trace::annotate("generated todo", todo_content);

    if !output.status.success() {
        // Clean up the temp file — don't abort the rebase here; callers
        // decide whether to abort (out-of-scope) or pause (resumable).
        let _ = temp_path.close();
        return Ok(RebaseOutcome::Conflicted);
    }

    // Clean up the temp file
    let _ = temp_path.close();

    Ok(RebaseOutcome::Completed)
}

/// Returns OIDs in `base..HEAD` that have cherry-pick equivalents in `upstream`.
///
/// Runs `git cherry <upstream> HEAD <base>`, which outputs one line per commit:
///   `- <sha>` — equivalent already in upstream (cherry-picked)
///   `+ <sha>` — not yet in upstream
///
/// This is O(feature commits), unlike the old patch-ID pipeline which was
/// O(upstream commits). `git cherry` uses the same patch-ID logic internally
/// and respects diff.algorithm consistently.
fn cherry_pick_equivalents(workdir: &Path, upstream: &Oid, base: &Oid) -> Option<HashSet<Oid>> {
    let stdout = git_commands::run_git_stdout(
        workdir,
        &["cherry", &upstream.to_string(), "HEAD", &base.to_string()],
    )
    .ok()?;

    Some(
        stdout
            .lines()
            .filter_map(|line| {
                line.strip_prefix("- ")
                    .and_then(|sha| Oid::from_str(sha.trim()).ok())
            })
            .collect(),
    )
}

#[cfg(test)]
#[path = "weave_test.rs"]
mod tests;
