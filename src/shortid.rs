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
/// - Multi-word names (split on `-`, `_`, `/`): pick one character from each
///   of two words, producing many 2-char combinations (e.g. `feature-alpha`
///   → `fa`, `fl`, `fp`, `ea`, …). Interleaved 3-char prefixes follow.
/// - Single-word names: pick character pairs (i, j) where i < j from the word
///   (e.g. `main` → `ma`, `mi`, `mn`, `ai`, …). 3-char prefixes follow.
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

/// Candidates for multi-word names: one char from each of two words,
/// then interleaved 3+ char prefixes.
fn multi_word_candidates(words: &[Vec<char>]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    // 2-char candidates from pairs of words
    for wi in 0..words.len() {
        for wj in (wi + 1)..words.len() {
            for pi in 0..words[wi].len() {
                for pj in 0..words[wj].len() {
                    let c: String = [words[wi][pi], words[wj][pj]].iter().collect();
                    if !seen.contains(&c) {
                        seen.insert(c.clone());
                        candidates.push(c);
                    }
                }
            }
        }
    }

    // 3+ char candidates: interleaved prefix
    let interleaved = interleave_words(words);
    for n in 3..=interleaved.len() {
        let c: String = interleaved.chars().take(n).collect();
        if !seen.contains(&c) {
            seen.insert(c.clone());
            candidates.push(c);
        }
    }

    candidates
}

/// Candidates for single-word names: character pairs (i < j),
/// then 3+ char prefixes.
fn single_word_candidates(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    // 2-char candidates: pairs of characters (i < j)
    for i in 0..chars.len() {
        for j in (i + 1)..chars.len() {
            let c: String = [chars[i], chars[j]].iter().collect();
            if !seen.contains(&c) {
                seen.insert(c.clone());
                candidates.push(c);
            }
        }
    }

    // 3+ char prefixes
    for n in 3..=chars.len() {
        let c: String = chars[..n].iter().collect();
        if !seen.contains(&c) {
            seen.insert(c.clone());
            candidates.push(c);
        }
    }

    // Very short words: add as-is
    if chars.len() < 2 {
        let c: String = chars.iter().collect();
        if !seen.contains(&c) {
            seen.insert(c.clone());
            candidates.push(c);
        }
    }

    candidates
}

/// Interleave characters from words round-robin: first char of each word,
/// then second char of each word, etc.
fn interleave_words(words: &[Vec<char>]) -> String {
    let max_len = words.iter().map(|w| w.len()).max().unwrap_or(0);
    let mut result = String::new();
    for i in 0..max_len {
        for word in words {
            if i < word.len() {
                result.push(word[i]);
            }
        }
    }
    result
}

/// Assign unique IDs using greedy candidate selection.
///
/// Each entity is processed in order. The first available (non-taken)
/// candidate is assigned. This keeps IDs at 2 characters whenever possible,
/// only falling back to 3+ chars when all 2-char alternatives are exhausted.
fn resolve_collisions(entities: Vec<Entity>) -> HashMap<Entity, String> {
    let items: Vec<(Entity, Vec<String>)> = entities
        .into_iter()
        .map(|e| {
            let cands = generate_candidates(&e);
            (e, cands)
        })
        .collect();

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
