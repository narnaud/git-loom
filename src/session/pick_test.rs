use std::path::PathBuf;

use super::*;
use crate::core::diff::DiffHunk;
use crate::core::hunks::{HunkEntry, HunkOrigin};
use crate::core::repo::FileChange;
use crate::core::test_helpers::{base_info, oid};
use crate::session::tree::test_support::{branch_row, commit_row, local_changes, working_file};

const A: u8 = 0x01;
const B: u8 = 0x02;

/// `[local changes]` with one file, over branch `top` and commits `B` and `A`.
fn tree() -> Vec<Row> {
    vec![
        local_changes(1),
        working_file("a.rs"),
        branch_row("top", None),
        commit_row(B),
        commit_row(A),
    ]
}

/// A file with one hunk per flag, selected where it is set.
fn file(path: &str, selected: &[bool]) -> FileEntry {
    FileEntry {
        path: path.to_string(),
        hunks: selected
            .iter()
            .map(|&selected| HunkEntry {
                hunk: DiffHunk {
                    text: "@@ -1 +1 @@\n+line\n".to_string(),
                    modified_lines: vec![],
                },
                selected,
                origin: HunkOrigin::Commit,
            })
            .collect(),
        index_status: 'M',
        worktree_status: ' ',
        binary: false,
    }
}

fn fold_pick(rows: &[&Row]) -> Result<PickFor, Refusal> {
    // No repository there: `fold_targets` then leaves ancestry to the command.
    PickFor::fold(&tree(), &PathBuf::from("no-such-repo"), rows, "zz")
}

fn target(row: &Row, effect: FoldEffect) -> FoldTarget {
    FoldTarget {
        key: row.key.clone(),
        arg: row.target.clone().unwrap(),
        effect,
    }
}

#[test]
fn a_commit_pick_needs_local_changes() {
    let dirty = RepoInfo {
        working_changes: vec![FileChange {
            path: "a.rs".to_string(),
            index: ' ',
            worktree: 'M',
        }],
        ..base_info()
    };

    assert!(matches!(PickFor::commit(&dirty), Ok(PickFor::Commit)));
    assert_eq!(
        PickFor::commit(&base_info()).err(),
        Some("no local changes")
    );
}

#[test]
fn a_fold_pick_on_a_file_reads_every_local_change() {
    let tree = tree();

    let Ok(PickFor::Fold {
        sources,
        targets,
        commit,
    }) = fold_pick(&[&tree[1]])
    else {
        panic!("expected a fold pick");
    };

    assert_eq!(sources, ["zz"], "not the one file the cursor is on");
    assert_eq!(commit, None);
    assert_eq!(
        targets,
        [
            target(&tree[3], FoldEffect::Amend),
            target(&tree[4], FoldEffect::Amend)
        ]
    );
}

#[test]
fn a_fold_pick_on_a_commit_reads_that_commit() {
    let tree = tree();

    let Ok(pick) = fold_pick(&[&tree[3]]) else {
        panic!("expected a fold pick");
    };

    assert_eq!(pick.read("zz"), Read::Commit(oid(B)));
    let PickFor::Fold {
        sources,
        targets,
        commit,
    } = pick
    else {
        panic!("expected a fold pick");
    };
    assert_eq!(sources, [oid(B).to_string()]);
    assert_eq!(commit, Some(oid(B)));
    assert_eq!(
        targets,
        [
            target(&tree[0], FoldEffect::Uncommit),
            target(&tree[3], FoldEffect::Noop),
            target(&tree[4], FoldEffect::MoveFile),
        ],
        "its hunks move out of it rather than the commit folding"
    );
}

#[test]
fn a_fold_pick_refuses_rows_with_no_hunks_of_their_own() {
    let tree = tree();

    assert_eq!(
        fold_pick(&[&tree[2]]).err(),
        Some("move to a file, local changes, or a commit")
    );
    assert_eq!(
        fold_pick(&[&tree[3], &tree[4]]).err(),
        Some("one commit at a time")
    );
    assert_eq!(fold_pick(&[]).err(), Some("select source rows first"));
}

#[test]
fn a_pick_is_logged_as_the_command_reading_its_hunks() {
    let sid = |target: &str| format!("<{}>", &target[..2]);
    let fold = PickFor::Fold {
        sources: vec!["zz".to_string()],
        targets: Vec::new(),
        commit: None,
    };
    let split = PickFor::Split { commit: oid(A) };

    assert_eq!(PickFor::Commit.command_line("zz", sid), "loom add -p zz");
    assert_eq!(fold.command_line("zz", sid), "loom fold -p <zz>");
    assert_eq!(split.command_line("zz", sid), "loom split -p <01>");
}

#[test]
fn only_a_fold_of_local_changes_is_read_stamped() {
    let fold = |commit| PickFor::Fold {
        sources: Vec::new(),
        targets: Vec::new(),
        commit,
    };

    assert_eq!(
        PickFor::Commit.read("zz"),
        Read::Staging(vec!["zz".to_string()])
    );
    assert_eq!(fold(None).read("zz"), Read::Worktree);
    assert_eq!(fold(Some(oid(A))).read("zz"), Read::Commit(oid(A)));
    assert_eq!(
        PickFor::Split { commit: oid(A) }.read("zz"),
        Read::Commit(oid(A))
    );
}

#[test]
fn a_split_needs_two_hunks_to_pick_from() {
    let split = PickFor::Split { commit: oid(A) };
    let fold = PickFor::Fold {
        sources: Vec::new(),
        targets: Vec::new(),
        commit: None,
    };
    let none = Some(("No changes to pick", "no hunks to pick"));
    let one = [file("a.rs", &[false])];
    let two = [file("a.rs", &[false]), file("b.rs", &[false])];

    assert_eq!(split.nothing_to_pick(&[]), none);
    assert_eq!(
        split.nothing_to_pick(&one),
        Some(("Only one hunk", "one hunk only, nothing to split"))
    );
    assert_eq!(split.nothing_to_pick(&two), None);
    assert_eq!(fold.nothing_to_pick(&[]), none);
    assert_eq!(fold.nothing_to_pick(&one), None);
    assert_eq!(PickFor::Commit.nothing_to_pick(&[]), None);
}

#[test]
fn a_pick_keeping_no_hunk_keeps_nothing() {
    assert!(any_picked(&[file("a.rs", &[false, true])]));
    assert!(!any_picked(&[file("a.rs", &[false, false])]));
    assert!(!any_picked(&[]));
}

#[test]
fn a_fold_without_hunks_folds_its_sources_whole() {
    let tree = tree();

    assert_eq!(
        fold(
            vec!["a.rs".to_string()],
            target(&tree[4], FoldEffect::Amend),
            None
        ),
        Action::Fold {
            sources: vec!["a.rs".to_string()],
            target: oid(A).to_string(),
            hunks: None,
        }
    );
}

#[test]
fn a_fold_of_working_tree_hunks_carries_the_pick_itself() {
    let tree = tree();
    let hunks = FoldHunks::Worktree {
        picked: vec![file("a.rs", &[true])],
        stamp: vec![None],
    };

    assert_eq!(
        fold(
            vec!["zz".to_string()],
            target(&tree[4], FoldEffect::Amend),
            Some(hunks)
        ),
        Action::FoldHunks {
            sources: vec!["zz".to_string()],
            picked: vec![file("a.rs", &[true])],
            stamp: vec![None],
            target: oid(A).to_string(),
        }
    );
}

/// `fold -p` re-reads the hunks and refuses ids numbered against anything
/// else, so the fingerprint names where they land: nowhere for `zz`.
#[test]
fn a_fold_of_commit_hunks_names_them_by_id() {
    let tree = tree();
    let source = oid(B).to_string();
    let picked = || vec![file("a.rs", &[false, true])];
    let folded = |target: FoldTarget| {
        fold(
            vec![source.clone()],
            target,
            Some(FoldHunks::Commit(picked())),
        )
    };
    let expected = |target: &str, into: Option<&str>| Action::Fold {
        sources: vec![source.clone()],
        target: target.to_string(),
        hunks: Some(HunkArgs::new(
            vec!["a.rs:2".to_string()],
            Some(hunk_select::fingerprint(&source, into, &picked())),
        )),
    };
    let older = oid(A).to_string();

    assert_eq!(
        folded(target(&tree[4], FoldEffect::MoveFile)),
        expected(&older, Some(&older))
    );
    assert_eq!(
        folded(target(&tree[0], FoldEffect::Uncommit)),
        expected("zz", None)
    );
}

#[test]
fn a_split_keeps_something_on_each_side() {
    let commit = oid(A).to_string();
    let picked = [file("a.rs", &[true, false])];

    assert_eq!(
        split(oid(A), &picked),
        Ok(Action::Split {
            commit: commit.clone(),
            files: Vec::new(),
            hunks: Some(HunkArgs::new(
                vec!["a.rs:1".to_string()],
                Some(hunk_select::fingerprint(&commit, None, &picked)),
            )),
        })
    );
    assert_eq!(
        split(oid(A), &[file("a.rs", &[true, true])]),
        Err("leave at least one hunk for the second commit")
    );
}
