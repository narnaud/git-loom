use super::*;
use crate::core::test_helpers::{base_info, branch, commit, oid};
use crate::session::tree::test_support::{branch_row, commit_row, local_changes, upstream_row};

const A: u8 = 0x01;
const B: u8 = 0x02;
const C: u8 = 0x03;

/// ```text
/// │╭─ [top]
/// │● C
/// │● B
/// │● A
/// ├╯
/// ```
fn info() -> RepoInfo {
    RepoInfo {
        commits: vec![
            commit(C, "C", Some(B)),
            commit(B, "B", Some(A)),
            commit(A, "A", Some(0xAA)),
        ],
        branches: vec![branch("top", C, None)],
        ..base_info()
    }
}

#[test]
fn a_new_commit_goes_on_the_integration_line_or_a_branch_of_the_tree() {
    let tree = [
        local_changes(1),
        branch_row("top", None),
        commit_row(C),
        branch_row("below", None),
        commit_row(A),
        upstream_row(),
    ];

    assert_eq!(
        commit_dests(&tree),
        [
            CommitDest::Integration,
            CommitDest::Branch("top".to_string()),
            CommitDest::Branch("below".to_string()),
        ]
    );
}

#[test]
fn a_fold_of_no_rows_lands_nowhere() {
    assert_eq!(
        fold_targets(&[], Path::new(""), &[], false),
        Err("select source rows first")
    );
}

#[test]
fn a_commit_does_not_move_to_the_place_it_has() {
    let info = info();

    assert!(!can_move(&info, oid(B), &MoveSlot::Stay));
    assert!(!can_move(&info, oid(B), &MoveSlot::Above(oid(A))));
    assert!(!can_move(&info, oid(B), &MoveSlot::Below(oid(C))));
}

#[test]
fn a_commit_does_not_move_next_to_itself() {
    let info = info();

    assert!(!can_move(&info, oid(B), &MoveSlot::Above(oid(B))));
    assert!(!can_move(&info, oid(B), &MoveSlot::Below(oid(B))));
}

#[test]
fn a_commit_moves_past_its_neighbors() {
    let info = info();

    assert!(can_move(&info, oid(B), &MoveSlot::Above(oid(C))));
    assert!(can_move(&info, oid(B), &MoveSlot::Below(oid(A))));
}

/// `loom fold <tip> <its branch>` would move nothing.
#[test]
fn the_tip_of_a_branch_does_not_move_to_that_branch() {
    let info = info();
    let top = MoveSlot::Branch("top".to_string());

    assert!(!can_move(&info, oid(C), &top));
    assert!(can_move(&info, oid(B), &top));
}
