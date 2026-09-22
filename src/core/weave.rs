use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use git2::{Oid, Repository};

use crate::core::msg;
use crate::core::repo;
use crate::git;

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

#[derive(Debug, Clone)]
pub struct CommitEntry {
    pub oid: Oid,
    pub short_hash: String,
    pub message: String,
    pub command: Command,
    /// Non-woven branch names at this commit, serialized as `update-ref` lines.
    pub update_refs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BranchSection {
    /// "onto", or the label of the section this one is stacked on.
    pub reset_target: String,
    /// Oldest first.
    pub commits: Vec<CommitEntry>,
    /// Canonical label, used in `label` and `merge` directives.
    pub label: String,
    /// All branch refs at this section's tip (co-located branches).
    pub branch_names: Vec<String>,
}

/// An entry on the integration (first-parent) line.
#[derive(Debug, Clone)]
pub enum IntegrationEntry {
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
    /// The merge-base: the todo's "onto" target.
    pub base_oid: Oid,
    /// Woven branch sections in dependency order.
    pub branch_sections: Vec<BranchSection>,
    pub integration_line: Vec<IntegrationEntry>,
    /// Branches parked at the base: `update-ref` lines right after
    /// `reset onto`, for branches left without a commit (see `EmptiedRefs`).
    pub base_refs: Vec<String>,
}

/// Side of the anchor commit a relative move lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Above,
    Below,
}

impl Position {
    pub fn as_str(&self) -> &'static str {
        match self {
            Position::Above => "above",
            Position::Below => "below",
        }
    }
}

/// Where a commit sits in the graph: section index and position within it, or
/// an index into the integration line.
enum Slot {
    Section(usize, usize),
    Integration(usize),
}

/// What becomes of a branch ref left without a commit when its last commit
/// is removed from the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptiedRefs {
    /// `update-ref` it right after the reset it built on, so the branch
    /// survives as an empty branch at its base.
    Park,
    /// Leave it out of the todo, so the rebase does not touch it.
    Detach,
}

impl Weave {
    pub fn to_todo(&self) -> String {
        let mut out = String::new();

        out.push_str("label onto\n");

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

        out.push('\n');
        out.push_str("reset onto\n");
        flush_refs(&mut out, &self.base_refs);
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
                            git::short_hash(&oid.to_string()),
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

    /// Build the weave from the current repository state. Prefer
    /// `from_repo_with_info` when `RepoInfo` is already available.
    pub fn from_repo(repo: &Repository) -> Result<Self> {
        let info = repo::gather_repo_info(repo, false, 1)?;
        Self::from_repo_with_info(repo, &info)
    }

    /// Build the weave by walking the first-parent line from HEAD to the
    /// merge-base, collecting branch sections and integration-line entries.
    pub fn from_repo_with_info(repo: &Repository, info: &repo::RepoInfo) -> Result<Self> {
        let head_oid = repo::head_oid(repo)?;
        let merge_base_oid = base_oid(repo, info)?;

        let first_parent_entries = walk_first_parent_line(repo, head_oid, merge_base_oid)?;

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

        let mut assigned_branches: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for entry in &first_parent_entries {
            if entry.is_merge {
                if let Some(merge_parent_oid) = entry.merge_parent {
                    let branch_names_at_tip = branch_tips
                        .get(&merge_parent_oid)
                        .cloned()
                        .unwrap_or_default();

                    let branch_commits =
                        walk_branch_commits(repo, merge_parent_oid, merge_base_oid)?;

                    if !branch_commits.is_empty() || !branch_names_at_tip.is_empty() {
                        let label = if !branch_names_at_tip.is_empty() {
                            branch_names_at_tip[0].clone()
                        } else {
                            // No branch ref at the merge parent — use a generated label
                            format!("section-{}", git::short_hash(&merge_parent_oid.to_string()))
                        };

                        let todo_commits: Vec<CommitEntry> = branch_commits
                            .into_iter()
                            .rev()
                            .map(|c| {
                                let mut update_refs = Vec::new();
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

                        integration_line.push(IntegrationEntry::Merge {
                            original_oid: Some(entry.oid),
                            label,
                        });
                    }
                }
            } else {
                let mut update_refs = Vec::new();
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
            base_refs: Vec::new(),
        })
    }

    // ── Mutation methods ─────────────────────────────────────────────────

    /// Remove branch-section commits already in the new upstream (merged or
    /// cherry-picked); empty sections and their merges go too.
    ///
    /// Returns branches that lost all their commits: they vanish from the todo,
    /// so the rebase leaves their refs untouched.
    pub fn filter_upstream_commits(
        &mut self,
        repo: &Repository,
        workdir: &Path,
        new_upstream_oid: Oid,
    ) -> Result<Vec<String>> {
        // Strategy 1: exact ancestor check (fast, no processes)
        let mut candidates: Vec<Oid> = Vec::new();
        let mut to_drop = Vec::new();
        for section in &self.branch_sections {
            for commit in &section.commits {
                if repo::contains(repo, new_upstream_oid, commit.oid)? {
                    to_drop.push(commit.oid);
                } else {
                    candidates.push(commit.oid);
                }
            }
        }

        // Strategy 2: git cherry for the rest — O(feature commits), not O(upstream)
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

        let mut emptied = Vec::new();
        for oid in to_drop {
            emptied.extend(
                self.drop_commit(oid, EmptiedRefs::Detach)
                    .expect("the oid comes from the sections themselves"),
            );
        }
        Ok(emptied)
    }

    /// Remove a commit from the graph, dropping its section and merge entry if
    /// it was the section's last commit.
    ///
    /// Returns the branches left without a commit, handled per `emptied`, or
    /// `None` if the commit is not in the graph.
    #[must_use]
    pub fn drop_commit(&mut self, oid: Oid, emptied: EmptiedRefs) -> Option<Vec<String>> {
        let (_, mut names, section) = self.remove_commit(oid, emptied)?;

        // A section left empty goes, with its merge
        if let Some(i) = section
            && self.branch_sections[i].commits.is_empty()
        {
            names.extend(self.remove_empty_section(i, emptied));
        }
        Some(names)
    }

    /// Remove a section left without commits, along with its merge entry;
    /// sections stacked on it move down to what it was built on. Returns its
    /// branch names, handled per `emptied`.
    fn remove_empty_section(&mut self, idx: usize, emptied: EmptiedRefs) -> Vec<String> {
        let section = self.branch_sections.remove(idx);
        for s in &mut self.branch_sections {
            if s.reset_target == section.label {
                s.reset_target = section.reset_target.clone();
            }
        }
        self.integration_line.retain(
            |e| !matches!(e, IntegrationEntry::Merge { label: l, .. } if *l == section.label),
        );
        if emptied == EmptiedRefs::Park {
            self.park_refs(&section.reset_target, section.branch_names.clone());
        }
        section.branch_names
    }

    /// Take a commit out of the graph, leaving its section in place even when
    /// empty. An inner branch ending at the commit ends at the one before it;
    /// if none is left it is handled per `emptied` — moving it onto the next
    /// commit would give the branch a commit it never contained.
    ///
    /// Returns the commit (`update_refs` cleared), the emptied branches, and
    /// the index of the section it came from.
    fn remove_commit(
        &mut self,
        oid: Oid,
        emptied: EmptiedRefs,
    ) -> Option<(CommitEntry, Vec<String>, Option<usize>)> {
        for i in 0..self.branch_sections.len() {
            if let Some(pos) = self.branch_sections[i]
                .commits
                .iter()
                .position(|c| c.oid == oid)
            {
                let mut removed = self.branch_sections[i].commits.remove(pos);
                let refs = std::mem::take(&mut removed.update_refs);
                if pos > 0 {
                    self.branch_sections[i].commits[pos - 1]
                        .update_refs
                        .extend(refs);
                    return Some((removed, Vec::new(), Some(i)));
                }
                if emptied == EmptiedRefs::Park {
                    let target = self.branch_sections[i].reset_target.clone();
                    self.park_refs(&target, refs.clone());
                }
                return Some((removed, refs, Some(i)));
            }
        }

        let pos = self
            .integration_line
            .iter()
            .position(|e| matches!(e, IntegrationEntry::Pick(c) if c.oid == oid))?;
        let IntegrationEntry::Pick(mut removed) = self.integration_line.remove(pos) else {
            unreachable!("position matched a Pick");
        };
        let refs = std::mem::take(&mut removed.update_refs);
        if refs.is_empty() {
            return Some((removed, Vec::new(), None));
        }
        // Move the refs back to the nearest earlier Pick
        let target =
            (0..pos).rfind(|&j| matches!(self.integration_line[j], IntegrationEntry::Pick(_)));
        if let Some(j) = target
            && let IntegrationEntry::Pick(ref mut c) = self.integration_line[j]
        {
            c.update_refs.extend(refs);
            return Some((removed, Vec::new(), None));
        }
        if emptied == EmptiedRefs::Park {
            self.base_refs.extend(refs.clone());
        }
        Some((removed, refs, None))
    }

    /// Park branch refs at what `reset_target` resolves to: the base for
    /// `onto`, otherwise the tip of the section carrying that label (an empty
    /// section defers to its own reset target).
    fn park_refs(&mut self, reset_target: &str, names: Vec<String>) {
        let mut target = reset_target.to_string();
        let mut seen = HashSet::new();
        loop {
            // "onto", an unknown label, or a cycle: park at the base.
            if target == "onto" || !seen.insert(target.clone()) {
                self.base_refs.extend(names);
                return;
            }
            let Some(section) = self.branch_sections.iter_mut().find(|s| s.label == target) else {
                self.base_refs.extend(names);
                return;
            };
            if let Some(last) = section.commits.last_mut() {
                last.update_refs.extend(names);
                return;
            }
            target = section.reset_target.clone();
        }
    }

    /// Whether `branch_name` matches a section's branch names or label.
    pub fn has_branch_section(&self, branch_name: &str) -> bool {
        self.section_index(branch_name).is_some()
    }

    /// Whether `branch_name` is an inner (stacked) ref, i.e. a branch whose tip
    /// is a commit inside another branch's section.
    pub fn is_inner_branch(&self, branch_name: &str) -> bool {
        self.inner_ref_position(branch_name).is_some()
    }

    /// Branch keeping an inner (stacked) branch's commits. `None` when no ref
    /// sits at its section's tip: the section label is generated there, so it
    /// must never be shown as a branch name.
    pub fn inner_branch_keeper(&self, branch_name: &str) -> Option<&str> {
        let (s, _) = self.inner_ref_position(branch_name)?;
        self.branch_sections[s]
            .branch_names
            .first()
            .map(String::as_str)
    }

    /// Section and commit indices of the commit carrying `branch_name` as an
    /// inner (stacked) ref.
    fn inner_ref_position(&self, branch_name: &str) -> Option<(usize, usize)> {
        self.branch_sections
            .iter()
            .enumerate()
            .find_map(|(s, section)| {
                section
                    .commits
                    .iter()
                    .position(|c| c.update_refs.iter().any(|r| r == branch_name))
                    .map(|pos| (s, pos))
            })
    }

    /// Index of the section `branch_name` labels or belongs to.
    fn section_index(&self, branch_name: &str) -> Option<usize> {
        self.branch_sections
            .iter()
            .position(|s| s.branch_names.iter().any(|n| n == branch_name) || s.label == branch_name)
    }

    /// Position of the last commit carrying a branch ref. Everything at or
    /// below it stays when the section is dropped.
    fn inner_branch_boundary(&self, idx: usize) -> Option<usize> {
        self.branch_sections[idx]
            .commits
            .iter()
            .rposition(|c| !c.update_refs.is_empty())
    }

    /// How many commits [`Self::drop_branch`] would remove from history: the
    /// section, minus the part an inner branch keeps, and minus commits the
    /// integration line picks too — a branch based on an integration commit
    /// carries it in its section, but it survives the drop. None if no section
    /// matches.
    pub fn branch_drop_size(&self, branch_name: &str) -> Option<usize> {
        let idx = self.section_index(branch_name)?;
        let kept = self
            .inner_branch_boundary(idx)
            .map_or(0, |boundary| boundary + 1);
        let on_integration_line: HashSet<Oid> = self
            .integration_line
            .iter()
            .filter_map(|e| match e {
                IntegrationEntry::Pick(c) => Some(c.oid),
                IntegrationEntry::Merge { .. } => None,
            })
            .collect();
        Some(
            self.branch_sections[idx].commits[kept..]
                .iter()
                .filter(|c| !on_integration_line.contains(&c.oid))
                .count(),
        )
    }

    /// Remove an entire branch section and its merge entry. False if no section
    /// matches.
    #[must_use]
    pub fn drop_branch(&mut self, branch_name: &str) -> bool {
        let Some(idx) = self.section_index(branch_name) else {
            return false;
        };

        let old_label = self.branch_sections[idx].label.clone();

        let inner_branch_boundary = self.inner_branch_boundary(idx);

        if let Some(boundary) = inner_branch_boundary {
            // Commits after the inner boundary belong to the dropped branch.
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
        true
    }

    /// Move a commit to the tip of a branch.
    ///
    /// An inner (stacked) target gets the commit right after its tip and
    /// advances to it, so commits stacked above replay on top of it. If the
    /// target is co-located with other branches the section is split, and the
    /// target gets a new stacked section holding the moved commit.
    ///
    /// Returns the branches left without a commit, parked at their base.
    pub fn move_commit(&mut self, oid: Oid, to_branch: &str) -> anyhow::Result<Vec<String>> {
        // Validate the target exists BEFORE removing the source
        let mut section_idx = self
            .branch_sections
            .iter()
            .position(|s| s.label == to_branch || s.branch_names.contains(&to_branch.to_string()));
        if section_idx.is_none() {
            let Some((s, pos)) = self.inner_ref_position(to_branch) else {
                anyhow::bail!(
                    "Cannot move commit: target branch '{}' not found in weave graph",
                    to_branch
                );
            };
            // Already the tip of the target: nothing to move. This is also
            // the precondition for the insert below — removing that commit
            // would carry `to_branch` out of the section with it, and there
            // would be no tip left to insert after.
            if self.branch_sections[s].commits[pos].oid == oid {
                return Ok(Vec::new());
            }
        }

        // Find and remove the commit from its current location. Inner
        // branches at it stay behind: they end at the commit before, or are
        // parked at the section's base.
        let Some((mut commit, mut parked, source_idx)) = self.remove_commit(oid, EmptiedRefs::Park)
        else {
            anyhow::bail!(
                "Cannot move commit: source commit {} not found in weave graph",
                oid
            );
        };

        commit.command = Command::Pick;

        // The source section left empty goes, with its merge — unless it is
        // the target, which is about to get the commit back.
        if let Some(i) = source_idx
            && Some(i) != section_idx
            && self.branch_sections[i].commits.is_empty()
        {
            parked.extend(self.remove_empty_section(i, EmptiedRefs::Park));
            if let Some(idx) = &mut section_idx
                && i < *idx
            {
                *idx -= 1;
            }
        }

        let Some(section_idx) = section_idx else {
            // Inner target: the commit goes right after the target's tip and
            // takes the ref, leaving any co-located inner refs where they are.
            let (s, pos) = self
                .inner_ref_position(to_branch)
                .expect("the moved commit is not the target's tip, so its ref stayed put");
            self.branch_sections[s].commits[pos]
                .update_refs
                .retain(|r| r != to_branch);
            commit.update_refs.push(to_branch.to_string());
            self.branch_sections[s].commits.insert(pos + 1, commit);
            return Ok(parked);
        };

        if self.branch_sections[section_idx].branch_names.len() > 1
            && self.branch_sections[section_idx]
                .branch_names
                .contains(&to_branch.to_string())
        {
            let old_label = self.branch_sections[section_idx].label.clone();

            self.branch_sections[section_idx]
                .branch_names
                .retain(|n| n != to_branch);

            if old_label == to_branch
                && let Some(first_remaining) =
                    self.branch_sections[section_idx].branch_names.first()
            {
                self.branch_sections[section_idx].label = first_remaining.clone();
            }

            let base_label = self.branch_sections[section_idx].label.clone();

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

            // The commit was all the remaining branches had: park them
            if self.branch_sections[section_idx].commits.is_empty() {
                parked.extend(self.remove_empty_section(section_idx, EmptiedRefs::Park));
            }
        } else {
            self.branch_sections[section_idx].commits.push(commit);
        }
        Ok(parked)
    }

    /// Move commits next to `anchor`, keeping their given order (Spec 007).
    ///
    /// `Above` puts the block right after the anchor and carries the inner
    /// (stacked) refs that ended there up to its top. A section tip's own
    /// branches advance with the section instead, so `--above <tip>` moves
    /// every branch co-located there, where `move_commit` splits the section
    /// and advances only the one named. `Below` inserts right before and
    /// leaves the anchor's refs alone. Refs the removals park onto the anchor
    /// stay there, and branches ending at a moved commit stay behind as in
    /// `move_commit`. Errors when the block already sits there.
    ///
    /// `oids` must be deduplicated and oldest-first: a removal hands the
    /// commit's refs to the one below it, so any other order parks refs on
    /// commits that are themselves leaving.
    ///
    /// Returns the branches left without a commit, parked at their base.
    pub fn move_commits_relative(
        &mut self,
        oids: &[Oid],
        anchor: Oid,
        position: Position,
    ) -> anyhow::Result<Vec<String>> {
        if oids.contains(&anchor) {
            anyhow::bail!("Source and target are the same commit");
        }
        for (i, oid) in oids.iter().enumerate() {
            self.require_commit(*oid)?;
            // Before any removal: a second pass over the same commit would
            // find it gone, and every other error here leaves the graph whole.
            if oids[..i].contains(oid) {
                anyhow::bail!(
                    "Cannot move commit: source commit {} is listed twice",
                    crate::git::short_hash(&oid.to_string())
                );
            }
        }
        self.require_commit(anchor)?;
        if self.block_sits_at(oids, anchor, position) {
            let anchor_hex = anchor.to_string();
            let anchor_short = crate::git::short_hash(&anchor_hex);
            match oids {
                [only] => anyhow::bail!(
                    "Commit `{}` is already directly {} `{}`",
                    crate::git::short_hash(&only.to_string()),
                    position.as_str(),
                    anchor_short
                ),
                _ => anyhow::bail!(
                    "Commits are already in place {} `{}`",
                    position.as_str(),
                    anchor_short
                ),
            }
        }

        let anchor_refs = self
            .find_commit(anchor)
            .map(|c| c.update_refs.clone())
            .unwrap_or_default();

        let mut parked = Vec::new();
        let mut block = Vec::with_capacity(oids.len());
        for oid in oids {
            // Unreachable: the sources are checked present and distinct
            // above. Bail rather than panic so a graph bug cannot abort loom
            // mid-plan.
            let Some((mut commit, names, section)) = self.remove_commit(*oid, EmptiedRefs::Park)
            else {
                anyhow::bail!(
                    "Cannot move commit: source commit {} is not in the weave graph",
                    crate::git::short_hash(&oid.to_string())
                );
            };
            commit.command = Command::Pick;
            parked.extend(names);
            if let Some(i) = section
                && self.branch_sections[i].commits.is_empty()
            {
                parked.extend(self.remove_empty_section(i, EmptiedRefs::Park));
            }
            block.push(commit);
        }

        if position == Position::Above
            && let Some(top) = block.last_mut()
        {
            top.update_refs.extend(anchor_refs.iter().cloned());
        }

        // Unreachable: the anchor is never one of the removed commits. Bail
        // rather than panic, as the removals above do.
        let Some(slot) = self.locate(anchor) else {
            anyhow::bail!(
                "Cannot move commit: target commit {} left the weave graph",
                crate::git::short_hash(&anchor.to_string())
            );
        };
        match slot {
            Slot::Section(s, pos) => {
                let commits = &mut self.branch_sections[s].commits;
                let at = match position {
                    Position::Above => {
                        commits[pos]
                            .update_refs
                            .retain(|r| !anchor_refs.contains(r));
                        pos + 1
                    }
                    Position::Below => pos,
                };
                commits.splice(at..at, block);
            }
            Slot::Integration(i) => {
                let at = match position {
                    Position::Above => {
                        if let IntegrationEntry::Pick(c) = &mut self.integration_line[i] {
                            c.update_refs.retain(|r| !anchor_refs.contains(r));
                        }
                        i + 1
                    }
                    Position::Below => i,
                };
                self.integration_line
                    .splice(at..at, block.into_iter().map(IntegrationEntry::Pick));
            }
        }
        Ok(parked)
    }

    /// Whether `oids`, in order, are the entries right `position` of `anchor`.
    /// A merge entry on the integration line breaks the adjacency.
    fn block_sits_at(&self, oids: &[Oid], anchor: Oid, position: Position) -> bool {
        let Some(slot) = self.locate(anchor) else {
            return false;
        };
        let neighbours: Vec<Option<Oid>> = match slot {
            Slot::Section(s, pos) => {
                let commits = &self.branch_sections[s].commits;
                let range = match position {
                    Position::Above => pos + 1..(pos + 1 + oids.len()).min(commits.len()),
                    Position::Below => pos.saturating_sub(oids.len())..pos,
                };
                commits[range].iter().map(|c| Some(c.oid)).collect()
            }
            Slot::Integration(i) => {
                let entries = &self.integration_line;
                let range = match position {
                    Position::Above => i + 1..(i + 1 + oids.len()).min(entries.len()),
                    Position::Below => i.saturating_sub(oids.len())..i,
                };
                entries[range]
                    .iter()
                    .map(|e| match e {
                        IntegrationEntry::Pick(c) => Some(c.oid),
                        IntegrationEntry::Merge { .. } => None,
                    })
                    .collect()
            }
        };
        neighbours == oids.iter().map(|o| Some(*o)).collect::<Vec<_>>()
    }

    fn locate(&self, oid: Oid) -> Option<Slot> {
        for (s, section) in self.branch_sections.iter().enumerate() {
            if let Some(pos) = section.commits.iter().position(|c| c.oid == oid) {
                return Some(Slot::Section(s, pos));
            }
        }
        self.integration_line
            .iter()
            .position(|e| matches!(e, IntegrationEntry::Pick(c) if c.oid == oid))
            .map(Slot::Integration)
    }

    fn find_commit(&self, oid: Oid) -> Option<&CommitEntry> {
        match self.locate(oid)? {
            Slot::Section(s, pos) => Some(&self.branch_sections[s].commits[pos]),
            Slot::Integration(i) => match &self.integration_line[i] {
                IntegrationEntry::Pick(c) => Some(c),
                IntegrationEntry::Merge { .. } => None,
            },
        }
    }

    /// Whether the weave holds `oid` as a commit it can rewrite.
    pub fn contains_commit(&self, oid: Oid) -> bool {
        self.branch_sections
            .iter()
            .any(|s| s.commits.iter().any(|c| c.oid == oid))
            || self
                .integration_line
                .iter()
                .any(|entry| matches!(entry, IntegrationEntry::Pick(c) if c.oid == oid))
    }

    /// Error out unless the weave can rewrite `oid`.
    ///
    /// A caller that changes the repository before it rebases — `fold` commits
    /// a `fixup!` first — must ask this before touching anything, or a target
    /// the weave rejects leaves that change behind.
    pub fn require_commit(&self, oid: Oid) -> anyhow::Result<()> {
        if self.contains_commit(oid) {
            return Ok(());
        }
        anyhow::bail!(
            "Commit {} is not one loom can rewrite — it is not in the weave \
             graph, so it sits on the upstream side of the integration base",
            crate::git::short_hash(&oid.to_string())
        )
    }

    /// Change the source commit to Fixup and move it right after the target.
    pub fn fixup_commit(&mut self, source_oid: Oid, target_oid: Oid) -> anyhow::Result<()> {
        // Validate target exists BEFORE removing the source
        self.require_commit(target_oid)?;

        // Refs at the source travel with it: a branch ending there ends at
        // the squashed target afterwards.
        let Some(mut commit) = self.take_commit(source_oid) else {
            anyhow::bail!(
                "Cannot fixup commit: source commit {} not found in weave graph",
                source_oid
            );
        };
        commit.command = Command::Fixup;

        for section in &mut self.branch_sections {
            if let Some(pos) = section.commits.iter().position(|c| c.oid == target_oid) {
                section.commits.insert(pos + 1, commit);
                return Ok(());
            }
        }

        for (i, entry) in self.integration_line.iter().enumerate() {
            if let IntegrationEntry::Pick(c) = entry
                && c.oid == target_oid
            {
                self.integration_line
                    .insert(i + 1, IntegrationEntry::Pick(commit));
                return Ok(());
            }
        }

        anyhow::bail!(
            "Cannot fixup commit: target commit {} disappeared during operation",
            target_oid
        )
    }

    /// Mark `oid` for an `edit` stop, reporting whether it is in the graph at
    /// all. Ignoring a `false` is only safe when the same commit is passed as
    /// `expect_stop` to `run_rebase_expecting_edit`, whose `ensure_todo_edits`
    /// refuses in its place — and does so through the same error path as a
    /// failed rebase, so the caller's rollback still runs. A second `edit` in
    /// one todo has no such backstop.
    #[must_use]
    pub fn edit_commit(&mut self, oid: Oid) -> bool {
        self.set_command(oid, Command::Edit)
    }

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

    /// Add a merge entry on the integration line, at `position` or — when
    /// `None` — after all existing merges, so the new branch sits below any
    /// loose commits in the resulting history.
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

    /// Weave a non-woven branch into the integration line: the integration-line
    /// picks up to and including the branch tip become a new branch section, and a
    /// merge entry is added for it.
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

    /// Reassign a branch section from one branch name to another, renaming its
    /// label and merge line. Returns false if no section matches `drop_branch`.
    #[must_use]
    pub fn reassign_branch(&mut self, drop_branch: &str, keep_branch: &str) -> bool {
        let Some(section) = self
            .branch_sections
            .iter_mut()
            .find(|s| s.label == drop_branch || s.branch_names.contains(&drop_branch.to_string()))
        else {
            return false;
        };

        let old_label = section.label.clone();

        if section.label == drop_branch {
            section.label = keep_branch.to_string();
        }

        section.branch_names.retain(|n| n != drop_branch);

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
        true
    }

    /// Swap two commits in the same container: one branch section, or both on the
    /// integration line. Errors otherwise, or if either commit is missing.
    pub fn swap_commits(&mut self, oid_a: Oid, oid_b: Oid) -> Result<()> {
        if oid_a == oid_b {
            bail!("Cannot swap a commit with itself");
        }

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

    /// Take a commit out of the graph as is, `update_refs` included.
    fn take_commit(&mut self, oid: Oid) -> Option<CommitEntry> {
        for section in &mut self.branch_sections {
            if let Some(pos) = section.commits.iter().position(|c| c.oid == oid) {
                return Some(section.commits.remove(pos));
            }
        }
        let i = self
            .integration_line
            .iter()
            .position(|e| matches!(e, IntegrationEntry::Pick(c) if c.oid == oid))?;
        match self.integration_line.remove(i) {
            IntegrationEntry::Pick(commit) => Some(commit),
            _ => unreachable!("position matched a Pick"),
        }
    }

    /// Add a branch name to a commit's `update_refs` so `--update-refs` keeps it in
    /// sync — how a caller tracks a commit's new OID across a rebase.
    ///
    /// False when `oid` is not in the graph: nothing then updates the ref, and
    /// a caller reading it back would report the hash it started from.
    #[must_use]
    pub fn track_commit(&mut self, oid: Oid, ref_name: &str) -> bool {
        for section in &mut self.branch_sections {
            for commit in &mut section.commits {
                if commit.oid == oid {
                    commit.update_refs.push(ref_name.to_string());
                    return true;
                }
            }
        }

        for entry in &mut self.integration_line {
            if let IntegrationEntry::Pick(commit) = entry
                && commit.oid == oid
            {
                commit.update_refs.push(ref_name.to_string());
                return true;
            }
        }
        false
    }

    fn set_command(&mut self, oid: Oid, command: Command) -> bool {
        for section in &mut self.branch_sections {
            for commit in &mut section.commits {
                if commit.oid == oid {
                    commit.command = command;
                    return true;
                }
            }
        }

        for entry in &mut self.integration_line {
            if let IntegrationEntry::Pick(commit) = entry
                && commit.oid == oid
            {
                commit.command = command;
                return true;
            }
        }
        false
    }
}

/// Emit commit lines, returning the refs still pending after the last one.
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

fn flush_refs(out: &mut String, refs: &[String]) {
    for r in refs {
        out.push_str(&format!("update-ref refs/heads/{}\n", r));
    }
}

/// `branch \`a\`` or `branches \`a\`, \`b\``, for messages about the
/// branches `drop_commit` left without a commit.
pub fn describe_branches(names: &[String]) -> String {
    let list = names
        .iter()
        .map(|b| format!("`{b}`"))
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() == 1 {
        format!("branch {list}")
    } else {
        format!("branches {list}")
    }
}

#[derive(Debug)]
struct FirstParentEntry {
    oid: Oid,
    short_hash: String,
    message: String,
    is_merge: bool,
    /// For merge commits: the second parent (branch being merged).
    merge_parent: Option<Oid>,
}

/// Find where the integration line meets the upstream.
///
/// Not `git merge-base`: that returns the best common ancestor anywhere in the
/// graph. When a woven branch lands upstream as a fast-forward the upstream tip
/// *is* that branch's tip — a second parent — and the merge-base lands on the
/// branch side; walking to it would mistake the rest of the integration line
/// for a branch and discard the whole weave.
///
/// So follow first parents and stop at the first commit the upstream already
/// contains, falling back to `merge_base` if the line reaches a root first.
fn integration_base(repo: &Repository, head: Oid, upstream: Oid, merge_base: Oid) -> Result<Oid> {
    let mut current = head;
    loop {
        if current == merge_base || repo::contains(repo, upstream, current).unwrap_or(false) {
            return Ok(current);
        }
        let commit = repo.find_commit(current)?;
        match commit.parent_id(0) {
            Ok(parent) => current = parent,
            Err(_) => return Ok(merge_base),
        }
    }
}

/// The base a weave built from `info` would rewrite from.
///
/// Not always the merge-base: once a woven branch lands upstream, the
/// merge-base is that branch's own tip, which is still inside the weave.
pub fn base_oid(repo: &Repository, info: &repo::RepoInfo) -> Result<Oid> {
    integration_base(
        repo,
        repo::head_oid(repo)?,
        info.upstream.tip_oid,
        info.upstream.merge_base_oid,
    )
}

/// Walk the first-parent line from `head` to `stop` (exclusive), oldest-first,
/// merges included. `merge_parent` always names the branch side, even when an
/// upstream merge has its parents the other way round.
fn walk_first_parent_line(
    repo: &Repository,
    head: Oid,
    stop: Oid,
) -> Result<Vec<FirstParentEntry>> {
    let mut entries = Vec::new();
    let mut current = head;
    let mut visited: HashSet<Oid> = HashSet::new();

    while current != stop {
        if !visited.insert(current) {
            bail!("cycle detected in commit graph at {}", current);
        }
        let commit = repo.find_commit(current)?;

        let short_hash = commit
            .as_object()
            .short_id()?
            .as_str()
            .context("short_id is not valid UTF-8")?
            .to_string();
        let message = repo::commit_subject(&commit);

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

    entries.reverse();
    Ok(entries)
}

/// Walk branch commits from `tip` back to `stop` (exclusive), newest first,
/// skipping merges. A branch forked before `stop` stops at
/// `merge_base(tip, stop)` instead, so the walk stays out of shared history.
fn walk_branch_commits(repo: &Repository, tip: Oid, stop: Oid) -> Result<Vec<BranchCommitEntry>> {
    let actual_stop = if tip == stop {
        stop
    } else {
        repo.merge_base(tip, stop).unwrap_or(stop)
    };

    let mut entries = Vec::new();
    let mut current = tip;
    let mut visited: HashSet<Oid> = HashSet::new();

    while current != actual_stop {
        if !visited.insert(current) {
            bail!("cycle detected in commit graph at {}", current);
        }
        let commit = repo.find_commit(current)?;

        if commit.parent_count() <= 1 {
            let short_hash = commit
                .as_object()
                .short_id()?
                .as_str()
                .context("short_id is not valid UTF-8")?
                .to_string();
            let message = repo::commit_subject(&commit);

            entries.push(BranchCommitEntry {
                oid: current,
                short_hash,
                message,
            });
        }

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

/// Start an interactive rebase that pauses at `commit_oid` (`edit`), using the
/// weave when there is one and a minimal linear todo otherwise.
pub fn start_edit_rebase(repo: &Repository, workdir: &Path, commit_oid: Oid) -> Result<()> {
    if let Ok(mut graph) = Weave::from_repo(repo) {
        if !graph.edit_commit(commit_oid) {
            return Err(not_in_the_weave(commit_oid));
        }
        let todo = graph.to_todo();
        return run_rebase_expecting_edit(
            workdir,
            Some(&graph.base_oid.to_string()),
            &todo,
            commit_oid,
            &[],
        );
    }

    build_and_run_linear_edit(repo, workdir, commit_oid)
}

/// Build a linear todo from HEAD to the target's parent (or root) and rebase.
/// For repos with no upstream, where `Weave::from_repo` does not work.
fn build_and_run_linear_edit(repo: &Repository, workdir: &Path, commit_oid: Oid) -> Result<()> {
    let head_oid = repo::head_oid(repo)?;
    let commit = repo.find_commit(commit_oid)?;

    // Determine upstream (parent of target, or --root for root commits)
    let upstream: Option<String> = if commit.parent_count() > 0 {
        Some(commit.parent_id(0)?.to_string())
    } else {
        None
    };

    let stop = upstream.as_ref().and_then(|s| Oid::from_str(s).ok());

    let mut entries = Vec::new();
    let mut current = head_oid;
    let mut visited: HashSet<Oid> = HashSet::new();

    loop {
        if Some(current) == stop {
            break;
        }
        if !visited.insert(current) {
            bail!("cycle detected in commit graph at {}", current);
        }

        let c = repo.find_commit(current)?;
        let short = c
            .as_object()
            .short_id()?
            .as_str()
            .context("Short ID is not valid UTF-8")?
            .to_string();
        let msg = repo::commit_subject(&c);
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

    let mut todo = String::from("label onto\n\nreset onto\n");
    for line in &entries {
        todo.push_str(line);
        todo.push('\n');
    }

    run_rebase_expecting_edit(workdir, upstream.as_deref(), &todo, commit_oid, &[])
}

pub use crate::git::RebaseOutcome;

/// Execute a weave-based rebase, aborting automatically if it stops.
///
/// The todo has no `edit`/`break`, so anything short of `Completed` is a
/// failure; callers that do drive `edit` steps want
/// [`run_rebase_expecting_edit`]. For out-of-scope callers only — a resumable
/// command saves `LoomState` and calls `run_rebase` directly.
pub fn run_rebase_or_abort(
    workdir: &Path,
    upstream: Option<&str>,
    todo_content: &str,
) -> Result<()> {
    // The autostash replay re-stages nothing, whichever way this ends (Spec 004).
    let saved_staged = git::diff_cached(workdir)?;
    match run_rebase(workdir, upstream, todo_content) {
        Ok(RebaseOutcome::Completed) => {
            git::restore_staged_after_rebase(workdir, &saved_staged);
            Ok(())
        }
        Ok(RebaseOutcome::Stopped | RebaseOutcome::Paused) => {
            // `abort_after_failure` has aborted already, so this does not go
            // through `rebase_abort_then_cleanup`: a second abort would report
            // the failure twice.
            let err = git::abort_after_failure(workdir);
            git::restore_or_park_after_abort(workdir, &saved_staged, &err);
            Err(err)
        }
        // A pre-flight refusal never autostashed, so that index is not ours.
        Err(e) if git::rebase_never_started(&e) => Err(e),
        Err(e) => {
            // The restore goes after, not in the cleanup closure: a failed abort
            // skips that closure, and this is the one caller with no `LoomState`
            // for `loom abort` to find the patch in.
            let err = git::rebase_abort_then_cleanup(workdir, e, || {});
            git::restore_or_park_after_abort(workdir, &saved_staged, &err);
            Err(err)
        }
    }
}

/// Execute a weave-based rebase whose todo this caller filled with `edit`
/// steps, aborting automatically if it stops for any other reason.
///
/// Stopping at the replay of `expect_stop` is the point of the call, and the
/// commits this todo marks `edit` are protected from being dropped as empty;
/// `also_target` adds the commits the operation lands on, edited or not, so the
/// refusal does not offer to drop them. The caller drives the rebase from there
/// and finishes it with [`git::continue_rebase_expecting_edit`]. Any other
/// outcome — a stop on another commit, a rebase that never stopped — is
/// refused before the caller rewrites anything (see [`git::verify_paused_at`]).
pub fn run_rebase_expecting_edit(
    workdir: &Path,
    upstream: Option<&str>,
    todo_content: &str,
    expect_stop: Oid,
    also_target: &[&str],
) -> Result<()> {
    let edited = edited_commits(todo_content);
    ensure_todo_edits(&edited, expect_stop)?;
    let expected = expect_stop.to_string();

    let targets: Vec<String> = also_target.iter().map(|hash| hash.to_string()).collect();

    // Protect every commit this todo rewrites — a second `edit` in the same
    // rebase is as much the caller's as the one it stops at first. These are
    // the todo's own hashes, abbreviated, which `shas_match` handles.
    let protected = git::Protected::named(&edited).targeting(&targets);
    let outcome = halt_on_empty(workdir, upstream, todo_content, protected)?;

    match outcome {
        RebaseOutcome::Paused => git::verify_paused_at(workdir, &expected),
        RebaseOutcome::Completed => Err(git::finished_without_stopping(&expected)),
        RebaseOutcome::Stopped => Err(git::abort_after_failure(workdir)),
    }
}

/// Execute a weave-based rebase whose result speaks for `protected`, by reading
/// those commits back through a ref or by counting them (Spec 004).
///
/// Does NOT abort on a conflict — the outcome is the caller's, and a resumable
/// one must carry `protected` in its `LoomState` so `loom continue` keeps
/// protecting them. `protected` holds full object names: the empty-stop skip
/// matches on the shorter string, so a short ID would over-protect.
pub fn run_rebase_protecting(
    workdir: &Path,
    upstream: Option<&str>,
    todo_content: &str,
    protected: git::Protected<'_>,
) -> Result<RebaseOutcome> {
    // Checked in release too: this runs once per rebase, and a short ID here
    // silently protects every commit sharing its prefix. 40 for SHA-1, 64 for
    // SHA-256.
    if let Some(bad) = protected
        .named
        .iter()
        .chain(protected.targets)
        .find(|hash| !matches!(hash.len(), 40 | 64) || !hash.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return git::before_rebase_starts(Err(anyhow::anyhow!(
            "Internal error: `{bad}` is not a full object name"
        )));
    }
    halt_on_empty(workdir, upstream, todo_content, protected)
}

/// Run the todo with `--empty=stop` and carry it past every empty replay that
/// is not `protected` (Spec 004).
fn halt_on_empty(
    workdir: &Path,
    upstream: Option<&str>,
    todo_content: &str,
    protected: git::Protected<'_>,
) -> Result<RebaseOutcome> {
    let git_dir = git::absolute_git_dir(workdir)?;
    git::carry_past_known_stops(
        workdir,
        &git_dir,
        protected,
        None,
        run_rebase_with_empty(workdir, upstream, todo_content, git::empty_stop_value())?,
    )
}

/// The commits a todo marks `edit` — the ones a caller drives and must not
/// lose — as abbreviated as the todo spells them.
fn edited_commits(todo_content: &str) -> Vec<String> {
    todo_content
        .lines()
        .filter_map(|line| line.strip_prefix("edit "))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// Refuse before the rebase starts if the todo carries no `edit` for
/// `expect_stop`.
///
/// A commit outside the graph — an upstream commit, a stale SHA, a branch loom
/// does not manage — leaves the todo with nothing to stop at, and the rebase
/// runs to the end with no stop to verify. `Weave::edit_commit` reports that
/// directly; this reads the todo text because `build_and_run_linear_edit`
/// hand-builds one without a graph.
fn ensure_todo_edits(edited: &[String], expect_stop: Oid) -> Result<()> {
    let full = expect_stop.to_string();
    // Whatever length `core.abbrev` gives the todo, git chose it to be
    // unambiguous in this repository, so a shared prefix is this commit.
    let marked = edited
        .iter()
        .any(|hash| !hash.is_empty() && full.starts_with(hash.as_str()));

    if marked {
        return Ok(());
    }
    Err(not_in_the_weave(expect_stop))
}

/// The target is not among the commits a rewrite can reach.
pub fn not_in_the_weave(oid: Oid) -> anyhow::Error {
    anyhow::anyhow!(
        "Commit `{}` is not one of the commits loom rewrites — nothing was rewritten\n\
         If history moved, the SHA may be stale — run `loom` to see the current commits",
        git::short_hash(&oid.to_string())
    )
}

/// Execute a weave-based rebase, writing the todo to a temp file and running
/// `git rebase` with `internal-write-todo` as the sequence editor.
///
/// `upstream` is passed as git's `<upstream>` argument verbatim — no `^` suffix
/// — so commits after it up to HEAD are rebased; `None` means `--root`.
///
/// Returns `Paused` when it stopped at an `edit` the todo asked for and
/// `Stopped` when it stopped part-way. Does NOT abort. A commit whose changes
/// the new base already has is dropped by the sequencer, and a conflict
/// `rerere` already resolved is carried past (see
/// [`git::continue_rerere_stops`]).
pub fn run_rebase(
    workdir: &Path,
    upstream: Option<&str>,
    todo_content: &str,
) -> Result<RebaseOutcome> {
    // Tagged: a caller's undo must know this failure rewrote nothing.
    let git_dir = git::before_rebase_starts(git::absolute_git_dir(workdir))?;
    let outcome = run_rebase_with_empty(workdir, upstream, todo_content, "drop")?;
    git::continue_rerere_stops(workdir, &git_dir, None, outcome)
}

/// [`run_rebase`] with git's `--empty` mode chosen: `stop` reports a commit
/// that replayed empty instead of dropping it (see [`git::carry_past_known_stops`]).
fn run_rebase_with_empty(
    workdir: &Path,
    upstream: Option<&str>,
    todo_content: &str,
    empty: &str,
) -> Result<RebaseOutcome> {
    use std::io::Write;
    use std::process::Command;
    use std::time::Instant;

    use crate::trace as loom_trace;

    // Everything up to the spawn below is pre-flight, and its failures are
    // tagged so a caller's undo knows nothing was rewritten.
    let prepared = git::before_rebase_starts((|| {
        // Neither way this rebase moves a branch ref goes through git's check
        // against moving a branch checked out in another worktree, so do it
        // here, before anything is rewritten.
        git::ensure_not_checked_out_elsewhere(workdir, &rewritten_branches(workdir, todo_content))?;

        // Resolve the git dir up front: both exits below need it, and failing
        // afterwards would strand a resumable command's state file with its
        // rebase already done.
        let git_dir = git::absolute_git_dir(workdir)?;
        let self_exe = git::loom_exe_path()?;

        let mut temp_file = tempfile::NamedTempFile::new()?;
        temp_file.write_all(todo_content.as_bytes())?;
        temp_file.flush()?;
        Ok((git_dir, self_exe, temp_file.into_temp_path()))
    })())?;
    let (git_dir, self_exe, temp_path) = prepared;

    let exe_str = self_exe.display().to_string().replace('\\', "/");
    let source_path = temp_path.display().to_string().replace('\\', "/");

    let sequence_editor = format!(
        "{} internal-write-todo --source {} ",
        shell_escape::unix::escape(exe_str.into()),
        shell_escape::unix::escape(source_path.into()),
    );

    let upstream_arg = upstream.unwrap_or("--root");
    let empty_arg = format!("--empty={empty}");
    let log_args = format!(
        "rebase --interactive --autostash --keep-empty {empty_arg} --no-autosquash --rebase-merges --update-refs {upstream_arg}"
    );

    let mut cmd = Command::new("git");
    cmd.current_dir(workdir)
        .args(crate::git::FORCED_CONFIG)
        .args([
            "rebase",
            "--interactive",
            "--autostash",
            "--keep-empty",
            &empty_arg,
            "--no-autosquash",
            "--rebase-merges",
            "--update-refs",
        ])
        .env("GIT_SEQUENCE_EDITOR", sequence_editor)
        // Suppress the editor for new merge commits (no -C in the todo), keeping
        // the default "Merge branch '...'" message. Does not affect the user's
        // shell when the rebase pauses at `edit`.
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

    // Clean up the temp file. Don't abort the rebase here; callers decide
    // whether to abort (out-of-scope) or pause (resumable).
    let _ = temp_path.close();

    let result = if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("git rebase failed"))
    };
    git::rebase_outcome(&git_dir, result)
}

/// Branches this rebase will move: every `update-ref refs/heads/<name>` line in
/// the todo, plus HEAD's own branch, which moves when the rebase completes.
fn rewritten_branches(workdir: &Path, todo: &str) -> Vec<String> {
    let mut branches: Vec<String> = todo
        .lines()
        .filter_map(|line| line.trim().strip_prefix("update-ref refs/heads/"))
        .map(str::to_string)
        .collect();
    if let Ok(current) = git::current_branch(workdir) {
        branches.push(current);
    }
    branches
}

/// OIDs in `base..HEAD` that have cherry-pick equivalents in `upstream`.
///
/// From `git cherry <upstream> HEAD <base>`, whose `- <sha>` lines mark the
/// commits already upstream. O(feature commits), and it shares git's own
/// patch-ID logic, so `diff.algorithm` stays consistent.
fn cherry_pick_equivalents(workdir: &Path, upstream: &Oid, base: &Oid) -> Option<HashSet<Oid>> {
    let stdout = git::run_git_stdout(
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
