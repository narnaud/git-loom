use std::collections::{HashMap, HashSet};

/// Types of entities that can receive short IDs.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum Entity {
    Unstaged,
    Branch(String),
    Commit(git2::Oid),
    File(String),
}

/// Allocates unique short IDs (2+ characters) to entities, resolving collisions
/// by trying alternative 2-char combinations before falling back to 3+ chars.
pub struct IdAllocator {
    map: HashMap<Entity, String>,
}

impl IdAllocator {
    /// Create a new allocator from a list of entities.
    /// IDs are deterministic: same entities in same order produce same IDs.
    pub fn new(entities: Vec<Entity>) -> Self {
        IdAllocator {
            map: resolve_collisions(entities),
        }
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
        self.map
            .get(&Entity::Commit(oid))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    pub fn get_file(&self, path: &str) -> &str {
        self.map
            .get(&Entity::File(path.to_string()))
            .map(|s| s.as_str())
            .unwrap_or("")
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
/// For commits, candidates are successive prefixes of the hex hash (2, 3, 4…).
fn generate_candidates(entity: &Entity) -> Vec<String> {
    match entity {
        Entity::Unstaged => vec!["zz".to_string()],
        Entity::Commit(oid) => {
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
    }
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
/// Example: `feature-alpha` → `fa`, then `fe`, `ft`, `fu`, `fr`, `ea`, `la`, etc.
fn multi_word_candidates(words: &[Vec<char>]) -> Vec<String> {
    let mut candidates = Vec::new();

    // Use first two words (or first word repeated if only one)
    let word1 = &words[0];
    let word2 = if words.len() >= 2 {
        &words[1]
    } else {
        &words[0]
    };

    // Generate all combinations of (char from word1, char from word2)
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
        candidates.push(chars[0].to_string());
        return candidates;
    }

    // Generate sliding windows of 2 characters
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

/// Assign unique IDs using collision-aware candidate selection.
///
/// Each entity is processed in order. For each entity, find a candidate that:
/// 1. Is not already used
/// 2. Prefers candidates whose first letter is not already used as a first letter
///
/// This implements the smart collision avoidance where `feature-a` and `feature-b`
/// get `fa` and `eb` (skipping 'f' for the second since 'f' is already used).
fn resolve_collisions(entities: Vec<Entity>) -> HashMap<Entity, String> {
    let items: Vec<(Entity, Vec<String>)> = entities
        .into_iter()
        .map(|e| {
            let cands = generate_candidates(&e);
            (e, cands)
        })
        .collect();

    let mut used: HashSet<String> = HashSet::new();
    let mut used_first_letters: HashSet<char> = HashSet::new();
    let mut result: HashMap<Entity, String> = HashMap::new();

    for (entity, candidates) in items {
        // First, try to find a candidate whose first letter is not yet used
        let id = candidates
            .iter()
            .find(|c| {
                if let Some(first_char) = c.chars().next() {
                    !used.contains(*c) && !used_first_letters.contains(&first_char)
                } else {
                    !used.contains(*c)
                }
            })
            .or_else(|| {
                // If all candidates have first letters that are taken, just find any unused candidate
                candidates.iter().find(|c| !used.contains(*c))
            })
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

        // Track the first letter of this ID
        if let Some(first_char) = id.chars().next() {
            used_first_letters.insert(first_char);
        }
        used.insert(id.clone());
        result.insert(entity, id);
    }

    result
}

#[cfg(test)]
#[path = "shortid_test.rs"]
mod tests;
