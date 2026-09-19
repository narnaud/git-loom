use std::collections::{HashMap, HashSet};

use crate::core::changeid;

/// Types of entities that can receive short IDs.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum Entity {
    Unstaged,
    Branch(String),
    Commit {
        oid: git2::Oid,
        /// Canonical Change-Id when the commit carries one (Spec 002).
        change_id: Option<String>,
    },
    File(String),
}

#[cfg(test)]
impl Entity {
    /// A commit with no Change-Id.
    pub fn commit(oid: git2::Oid) -> Self {
        Entity::Commit {
            oid,
            change_id: None,
        }
    }
}

/// What the allocator knows about one commit.
struct CommitId {
    id: String,
    /// The commit's Change-Id as letters, for prefix resolution. Present for
    /// every commit with a Change-Id, twins included.
    letters: Option<String>,
}

/// Allocates unique short IDs to entities (Spec 002): persistent letter IDs
/// for commits with a Change-Id, word-based or hash-prefix IDs for the rest,
/// resolving collisions by trying alternative candidates.
pub struct IdAllocator {
    map: HashMap<Entity, String>,
    commits: HashMap<git2::Oid, CommitId>,
}

impl IdAllocator {
    /// Create a new allocator from a list of entities.
    /// IDs are deterministic: same entities in same order produce same IDs.
    pub fn new(entities: Vec<Entity>) -> Self {
        let map = resolve_collisions(entities);
        let commits = map
            .iter()
            .filter_map(|(entity, id)| match entity {
                Entity::Commit { oid, change_id } => Some((
                    *oid,
                    CommitId {
                        id: id.clone(),
                        letters: change_id.as_deref().map(changeid::to_letters),
                    },
                )),
                _ => None,
            })
            .collect();
        IdAllocator { map, commits }
    }

    pub fn get_unstaged(&self) -> &str {
        self.map
            .get(&Entity::Unstaged)
            .map(|s| s.as_str())
            .unwrap_or("zz")
    }

    pub fn get_branch(&self, name: &str) -> &str {
        self.map
            .get(&Entity::Branch(name.to_string()))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    pub fn get_commit(&self, oid: git2::Oid) -> &str {
        self.commits.get(&oid).map(|c| c.id.as_str()).unwrap_or("")
    }

    pub fn get_file(&self, path: &str) -> &str {
        self.map
            .get(&Entity::File(path.to_string()))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Width of the widest commit ID, for column alignment.
    pub fn commit_id_width(&self) -> usize {
        self.commits.values().map(|c| c.id.len()).max().unwrap_or(0)
    }

    /// Every commit `arg` names by Change-Id: the canonical `I…` literal, or
    /// a prefix of at least [`changeid::MIN_LEN`] letters. One hit resolves;
    /// several mean a too-short prefix or twins sharing a Change-Id.
    pub fn find_persistent(&self, arg: &str) -> Vec<git2::Oid> {
        // A literal's letters are as long as the stored ones, so the prefix
        // test is equality for it.
        let needle = match changeid::normalize(arg) {
            Some(id) => changeid::to_letters(&id),
            None if arg.len() >= changeid::MIN_LEN && changeid::is_letters(arg) => arg.to_string(),
            None => return Vec::new(),
        };
        self.commits
            .iter()
            .filter(|(_, c)| c.letters.as_deref().is_some_and(|l| l.starts_with(&needle)))
            .map(|(oid, _)| *oid)
            .collect()
    }
}

/// Generate an ordered list of candidate short IDs for an entity.
///
/// For branches and files, candidates are built from word structure:
/// - Multi-word names (split on `-`, `_`, `/`): first letter of each word
///   (e.g. `feature-alpha` → `fa`). If collision on first letter, shift to
///   next available letter in each word (e.g. `feature-a`, `feature-b` → `fa`, `eb`).
/// - Single-word names: first 2 letters (e.g. `main` → `ma`). If collision on
///   first letter, shift forward (e.g. `main`, `mainstream` → `ma`, `ai`).
///
/// For commits, candidates are successive prefixes of the hex hash (2, 3,
/// 4…); a commit with a Change-Id gets [`persistent_candidates`] instead.
fn generate_candidates(entity: &Entity) -> Vec<String> {
    let candidates = match entity {
        // `zz` belongs to unstaged changes, which commands match literally.
        Entity::Unstaged => return vec!["zz".to_string()],
        Entity::Commit { oid, .. } => {
            let hex = oid.to_string();
            let chars: Vec<char> = hex.chars().collect();
            (2..=chars.len())
                .map(|n| chars[..n].iter().collect())
                .collect()
        }
        Entity::Branch(name) => word_candidates(name),
        Entity::File(path) => {
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            let stem = std::path::Path::new(filename)
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or(filename);
            word_candidates(stem)
        }
    };
    // Short IDs are always lowercase for consistency. Any other entity holding
    // `zz` would be unaddressable when the worktree is clean, so drop it.
    let candidates: Vec<String> = candidates
        .into_iter()
        .map(|c| c.to_lowercase())
        .filter(|c| c != "zz")
        .collect();

    if candidates.is_empty() {
        // Reached when every candidate was `zz` (an entity literally named
        // `zz`), or when the name yielded no candidates at all.
        vec!["zz1".to_string()]
    } else {
        candidates
    }
}

/// For each commit whose Change-Id no other commit shares: prefixes of its
/// letters from the shortest length, at least [`changeid::MIN_LEN`], at which
/// they differ from every other such commit's. Symmetric by construction, so
/// a new commit sharing a prefix lengthens both IDs and can never take an
/// existing one (Spec 002). Twins are left out: a shared Change-Id identifies
/// none of them, so they fall back to hash prefixes.
fn persistent_candidates(entities: &[Entity]) -> HashMap<git2::Oid, Vec<String>> {
    let mut owners: HashMap<&str, Vec<git2::Oid>> = HashMap::new();
    for entity in entities {
        if let Entity::Commit {
            oid,
            change_id: Some(change_id),
        } = entity
        {
            owners.entry(change_id).or_default().push(*oid);
        }
    }
    let unique: Vec<(git2::Oid, Vec<char>)> = owners
        .iter()
        .filter(|(_, oids)| oids.len() == 1)
        .map(|(change_id, oids)| (oids[0], changeid::to_letters(change_id).chars().collect()))
        .collect();

    unique
        .iter()
        .map(|(oid, letters)| {
            let mut n = changeid::MIN_LEN.min(letters.len());
            while n < letters.len()
                && unique
                    .iter()
                    .any(|(other, l)| other != oid && l[..n] == letters[..n])
            {
                n += 1;
            }
            let candidates = (n..=letters.len())
                .map(|n| letters[..n].iter().collect())
                .collect();
            (*oid, candidates)
        })
        .collect()
}

/// Build candidate IDs from a name, splitting on `-`, `_`, `/`.
fn word_candidates(name: &str) -> Vec<String> {
    let words: Vec<Vec<char>> = name
        .split(['-', '_', '/'])
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().collect())
        .collect();

    if words.len() >= 2 {
        multi_word_candidates(&words)
    } else {
        single_word_candidates(name)
    }
}

/// Candidates for multi-word names: first letter of first word, first letter of second word.
/// Then shift indices forward to avoid collisions.
/// Example: `feature-alpha` → `fa`, then `fl`, `fp`, `fh`, `ea`, `el`, etc.
fn multi_word_candidates(words: &[Vec<char>]) -> Vec<String> {
    let mut candidates = Vec::new();

    let word1 = &words[0];
    let word2 = if words.len() >= 2 {
        &words[1]
    } else {
        &words[0]
    };

    for &ch1 in word1 {
        for &ch2 in word2 {
            let candidate: String = [ch1, ch2].iter().collect();
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    // 3+ char prefixes for fallback
    for n in 3..=word1.len().max(word2.len()).max(5) {
        let prefix: String = format!(
            "{}{}",
            word1.iter().take(n).collect::<String>(),
            word2.iter().take(n).collect::<String>()
        )
        .chars()
        .take(n)
        .collect();
        if !candidates.contains(&prefix) {
            candidates.push(prefix);
        }
    }

    candidates
}

/// Candidates for single-word names: first 2 letters, then shift forward.
/// Example: `main` → `ma`, `ai`, `in`, `mn`, `ma` (wraps), then 3-char prefixes.
fn single_word_candidates(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut candidates = Vec::new();

    if chars.is_empty() {
        return candidates;
    }

    if chars.len() == 1 {
        // Minimum 2-char ID: double the character.
        candidates.push(format!("{}{}", chars[0], chars[0]));
        return candidates;
    }

    for i in 0..chars.len() {
        for j in (i + 1)..chars.len() {
            let candidate: String = [chars[i], chars[j]].iter().collect();
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    // 3+ char prefixes for fallback
    for n in 3..=chars.len() {
        let prefix: String = chars[..n].iter().collect();
        if !candidates.contains(&prefix) {
            candidates.push(prefix);
        }
    }

    candidates
}

/// Priority for entity allocation, lower first. Commits come before
/// branches/files: their candidate sets are the most constrained (prefixes of
/// one string), while names have rich word-based alternatives.
fn entity_priority(entity: &Entity) -> u8 {
    match entity {
        Entity::Unstaged => 0,
        Entity::Commit { .. } => 1,
        Entity::Branch(_) | Entity::File(_) => 2,
    }
}

/// Assign unique IDs using collision-aware candidate selection: entities are
/// processed Unstaged, then Commits, then Branches/Files (stable, so the
/// original order holds within a group), each taking the first candidate not
/// already assigned.
fn resolve_collisions(entities: Vec<Entity>) -> HashMap<Entity, String> {
    let mut persistent = persistent_candidates(&entities);
    let mut items: Vec<(Entity, Vec<String>)> = entities
        .into_iter()
        .map(|e| {
            let cands = match &e {
                Entity::Commit { oid, .. } => persistent.remove(oid),
                _ => None,
            }
            .unwrap_or_else(|| generate_candidates(&e));
            (e, cands)
        })
        .collect();

    // Sort by priority: Unstaged first, then Commits, then Branches/Files.
    // Stable sort preserves relative order within each priority group.
    items.sort_by_key(|(e, _)| entity_priority(e));

    let mut used: HashSet<String> = HashSet::new();
    let mut result: HashMap<Entity, String> = HashMap::new();

    for (entity, candidates) in items {
        let id = candidates
            .iter()
            .find(|c| !used.contains(*c))
            .cloned()
            .unwrap_or_else(|| {
                // Fallback: numeric suffix on the first candidate
                let base = candidates.first().map(|s| s.as_str()).unwrap_or("??");
                let mut n = 1;
                loop {
                    let suffixed = format!("{}{}", base, n);
                    if !used.contains(&suffixed) {
                        break suffixed;
                    }
                    n += 1;
                    // Defensive guard against pathological input
                    if n > 10000 {
                        break format!("{}_{}", base, n);
                    }
                }
            });

        used.insert(id.clone());
        result.insert(entity, id);
    }

    result
}

#[cfg(test)]
#[path = "shortid_test.rs"]
mod tests;
