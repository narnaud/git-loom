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
fn branch_word_initials() {
    let alloc = IdAllocator::new(vec![
        Entity::Branch("feature-a".to_string()),
        Entity::Branch("bugfix-123".to_string()),
    ]);
    // "feature-a" → words ["feature","a"] → first chars → "fa"
    assert_eq!(alloc.get_branch("feature-a"), "fa");
    // "bugfix-123" → words ["bugfix","123"] → first chars → "b1"
    assert_eq!(alloc.get_branch("bugfix-123"), "b1");
}

#[test]
fn single_word_branch_uses_first_two_chars() {
    let alloc = IdAllocator::new(vec![
        Entity::Branch("alpha".to_string()),
        Entity::Branch("beta".to_string()),
    ]);
    assert_eq!(alloc.get_branch("alpha"), "al");
    assert_eq!(alloc.get_branch("beta"), "be");
}

#[test]
fn commit_uses_first_two_hex_chars() {
    let alloc = IdAllocator::new(vec![Entity::Commit(oid(0xAB))]);
    let id = alloc.get_commit(oid(0xAB));
    assert_eq!(id.len(), 2);
    assert_eq!(id, "ab");
}

#[test]
fn file_uses_stem_not_extension() {
    let alloc = IdAllocator::new(vec![
        Entity::File("src/main.rs".to_string()),
        Entity::File("tests/integration.rs".to_string()),
    ]);
    // stem "main" → pairs (0,1)=ma
    assert_eq!(alloc.get_file("src/main.rs"), "ma");
    // stem "integration" → pairs (0,1)=in
    assert_eq!(alloc.get_file("tests/integration.rs"), "in");
}

#[test]
fn common_prefix_resolved_at_two_chars() {
    // feature-a and feature-b resolve immediately: fa vs fb
    let alloc = IdAllocator::new(vec![
        Entity::Branch("feature-a".to_string()),
        Entity::Branch("feature-b".to_string()),
    ]);
    assert_eq!(alloc.get_branch("feature-a"), "fa");
    assert_eq!(alloc.get_branch("feature-b"), "fb");
}

#[test]
fn same_first_word_different_second_word_stays_two_chars() {
    // feature-alpha and feature-awesome: both start "fa",
    // second entity gets alternative 2-char like "fw"
    let alloc = IdAllocator::new(vec![
        Entity::Branch("feature-alpha".to_string()),
        Entity::Branch("feature-awesome".to_string()),
    ]);
    let id_alpha = alloc.get_branch("feature-alpha");
    let id_awesome = alloc.get_branch("feature-awesome");
    assert_eq!(id_alpha, "fa");
    assert_eq!(id_awesome.len(), 2, "expected 2-char ID, got '{}'", id_awesome);
    assert_ne!(id_alpha, id_awesome);
}

#[test]
fn triple_collision_all_two_chars() {
    // main, master, maintenance — all single-word, start "ma"
    // main→ma, master→ms (next pair), maintenance→mi (next pair)
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
    // All should stay at 2 chars
    assert_eq!(id_main.len(), 2, "main: '{}'", id_main);
    assert_eq!(id_master.len(), 2, "master: '{}'", id_master);
    assert_eq!(id_maint.len(), 2, "maintenance: '{}'", id_maint);
}

#[test]
fn cross_entity_collision_resolved() {
    // Branch "main" and file "src/main.rs" (stem "main") both start "ma"
    let alloc = IdAllocator::new(vec![
        Entity::Branch("main".to_string()),
        Entity::File("src/main.rs".to_string()),
    ]);
    let branch_id = alloc.get_branch("main");
    let file_id = alloc.get_file("src/main.rs");
    assert_ne!(branch_id, file_id);
    // Both should stay at 2 chars
    assert_eq!(branch_id.len(), 2);
    assert_eq!(file_id.len(), 2);
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
    let alloc = IdAllocator::new(vec![
        Entity::Branch("a".to_string()),
        Entity::File("a".to_string()),
    ]);
    let id_branch = alloc.get_branch("a");
    let id_file = alloc.get_file("a");
    assert_ne!(id_branch, id_file);
}

#[test]
fn slash_separator_works() {
    let alloc = IdAllocator::new(vec![
        Entity::Branch("feature/login".to_string()),
        Entity::Branch("feature/logout".to_string()),
    ]);
    let id_login = alloc.get_branch("feature/login");
    let id_logout = alloc.get_branch("feature/logout");
    // Both primary "fl", second gets alternative 2-char
    assert_ne!(id_login, id_logout);
    assert_eq!(id_login.len(), 2);
    assert_eq!(id_logout.len(), 2);
}

#[test]
fn underscore_separator_works() {
    let alloc = IdAllocator::new(vec![
        Entity::Branch("my_feature".to_string()),
        Entity::Branch("my_fix".to_string()),
    ]);
    let id_feat = alloc.get_branch("my_feature");
    let id_fix = alloc.get_branch("my_fix");
    assert_ne!(id_feat, id_fix);
    assert_eq!(id_feat.len(), 2);
    assert_eq!(id_fix.len(), 2);
}

#[test]
fn file_with_underscores_uses_word_initials() {
    let alloc = IdAllocator::new(vec![
        Entity::File("new_file.txt".to_string()),
    ]);
    // stem "new_file" → words ["new","file"] → "nf"
    assert_eq!(alloc.get_file("new_file.txt"), "nf");
}
