use std::collections::HashMap;

/// Types of entities that can receive short IDs.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum Entity {
    Unstaged,
    Branch(String),
    Commit(git2::Oid),
    File(String),
}

/// Allocates unique short IDs (2+ characters) to entities, resolving collisions
/// by extending the prefix until all IDs are unique.
pub struct IdAllocator {
    map: HashMap<Entity, String>,
}

impl IdAllocator {
    /// Create a new allocator from a list of entities.
    /// IDs are deterministic: same entities in same order produce same IDs.
    pub fn new(entities: Vec<Entity>) -> Self {
        let items: Vec<(Entity, String)> = entities
            .into_iter()
            .map(|e| {
                let source = source_string(&e);
                (e, source)
            })
            .collect();

        IdAllocator {
            map: resolve_collisions(items),
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

/// The full string from which short IDs are derived.
fn source_string(entity: &Entity) -> String {
    match entity {
        Entity::Unstaged => "zz".to_string(),
        Entity::Branch(name) => name.clone(),
        Entity::Commit(oid) => format!("{}", oid),
        Entity::File(path) => std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string(),
    }
}

/// Resolve collisions by extending IDs until all are unique.
///
/// Algorithm: start each entity with a 2-char prefix of its source string.
/// For any group sharing the same ID, extend by one more character and repeat.
/// When a source is fully exhausted and still collides, append a numeric suffix.
fn resolve_collisions(items: Vec<(Entity, String)>) -> HashMap<Entity, String> {
    // Work with (entity, source, current_id)
    let mut items: Vec<(Entity, String, String)> = items
        .into_iter()
        .map(|(e, source)| {
            let id: String = source.chars().take(2).collect();
            (e, source, id)
        })
        .collect();

    let max_rounds = 200;
    for _ in 0..max_rounds {
        // Find which IDs are duplicated
        let mut id_count: HashMap<String, usize> = HashMap::new();
        for (_, _, id) in &items {
            *id_count.entry(id.clone()).or_insert(0) += 1;
        }

        let has_collision = id_count.values().any(|&c| c > 1);
        if !has_collision {
            break;
        }

        let mut any_extended = false;

        // Extend colliding IDs by one character
        for (_, source, id) in &mut items {
            if id_count.get(id.as_str()).copied().unwrap_or(0) > 1 {
                let new_len = id.chars().count() + 1;
                if new_len <= source.chars().count() {
                    *id = source.chars().take(new_len).collect();
                    any_extended = true;
                }
            }
        }

        // If no IDs could be extended (sources exhausted), break to apply
        // numeric suffixes below.
        if !any_extended {
            break;
        }
    }

    // Final pass: resolve any remaining collisions with numeric suffixes
    let mut used: HashMap<String, usize> = HashMap::new();
    for (_, _, id) in &mut items {
        let count = used.entry(id.clone()).or_insert(0);
        if *count > 0 {
            *id = format!("{}{}", id, count);
        }
        *count += 1;
    }

    items.into_iter().map(|(e, _, id)| (e, id)).collect()
}

#[cfg(test)]
#[path = "shortid_test.rs"]
mod tests;
