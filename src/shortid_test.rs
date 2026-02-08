use super::*;
use git2::Oid;

fn oid(byte: u8) -> Oid {
    let mut bytes = [0u8; 20];
    bytes[0] = byte;
    Oid::from_bytes(&bytes).unwrap()
}

#[test]
fn unstaged_is_always_zz() {
    let alloc = IdAllocator::new(vec![Entity::Unstaged]);
    assert_eq!(alloc.get_unstaged(), "zz");
}

#[test]
fn branch_uses_first_two_chars() {
    let alloc = IdAllocator::new(vec![
        Entity::Branch("feature-a".to_string()),
        Entity::Branch("bugfix-123".to_string()),
    ]);
    assert_eq!(alloc.get_branch("feature-a"), "fe");
    assert_eq!(alloc.get_branch("bugfix-123"), "bu");
}

#[test]
fn commit_uses_first_two_hex_chars() {
    let alloc = IdAllocator::new(vec![Entity::Commit(oid(0xAB))]);
    let id = alloc.get_commit(oid(0xAB));
    assert_eq!(id.len(), 2);
    assert_eq!(id, "ab");
}

#[test]
fn file_uses_filename_not_path() {
    let alloc = IdAllocator::new(vec![
        Entity::File("src/main.rs".to_string()),
        Entity::File("tests/integration.rs".to_string()),
    ]);
    assert_eq!(alloc.get_file("src/main.rs"), "ma");
    assert_eq!(alloc.get_file("tests/integration.rs"), "in");
}

#[test]
fn collision_extends_to_three_chars() {
    let alloc = IdAllocator::new(vec![
        Entity::Branch("feature-a".to_string()),
        Entity::Branch("feature-b".to_string()),
    ]);
    let id_a = alloc.get_branch("feature-a");
    let id_b = alloc.get_branch("feature-b");
    assert_ne!(id_a, id_b);
    // One should be extended beyond 2 chars
    assert!(id_a.len() >= 3 || id_b.len() >= 3);
}

#[test]
fn triple_collision_all_resolved() {
    let alloc = IdAllocator::new(vec![
        Entity::Branch("main".to_string()),
        Entity::Branch("master".to_string()),
        Entity::Branch("maintenance".to_string()),
    ]);
    let id_main = alloc.get_branch("main");
    let id_master = alloc.get_branch("master");
    let id_maint = alloc.get_branch("maintenance");

    assert_ne!(id_main, id_master);
    assert_ne!(id_main, id_maint);
    assert_ne!(id_master, id_maint);
}

#[test]
fn cross_entity_collision_resolved() {
    let alloc = IdAllocator::new(vec![
        Entity::Branch("main".to_string()),
        Entity::File("src/main.rs".to_string()),
    ]);
    let branch_id = alloc.get_branch("main");
    let file_id = alloc.get_file("src/main.rs");
    assert_ne!(branch_id, file_id);
}

#[test]
fn no_collision_stays_two_chars() {
    let alloc = IdAllocator::new(vec![
        Entity::Branch("alpha".to_string()),
        Entity::Branch("beta".to_string()),
        Entity::File("src/config.rs".to_string()),
        Entity::Commit(oid(0xFF)),
    ]);
    assert_eq!(alloc.get_branch("alpha"), "al");
    assert_eq!(alloc.get_branch("beta"), "be");
    assert_eq!(alloc.get_file("src/config.rs"), "co");
    assert_eq!(alloc.get_commit(oid(0xFF)).len(), 2);
}

#[test]
fn exhausted_source_uses_numeric_suffix() {
    // A branch and file with identical short sources — should not infinite loop
    let alloc = IdAllocator::new(vec![
        Entity::Branch("ab".to_string()),
        Entity::File("ab".to_string()),
    ]);
    let id_branch = alloc.get_branch("ab");
    let id_file = alloc.get_file("ab");
    assert_ne!(id_branch, id_file);
}

#[test]
fn short_source_does_not_hang() {
    // Single-char branch name colliding with a file
    let alloc = IdAllocator::new(vec![
        Entity::Branch("a".to_string()),
        Entity::File("a".to_string()),
    ]);
    let id_branch = alloc.get_branch("a");
    let id_file = alloc.get_file("a");
    assert_ne!(id_branch, id_file);
}
