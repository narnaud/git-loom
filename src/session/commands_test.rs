use std::path::PathBuf;

use super::*;
use crate::core::repo::{FileChange, RemoteStatus};
use crate::core::shortid::IdAllocator;
use crate::core::test_helpers::{base_info, branch, commit, oid};
use crate::session::tree::test_support::{
    branch_row, commit_file, commit_row, local_changes, row, upstream_row, working_file,
};

const A: u8 = 0x01;
const B: u8 = 0x02;

fn change(path: &str) -> FileChange {
    FileChange {
        path: path.to_string(),
        index: 'M',
        worktree: ' ',
    }
}

/// Commit `A` changing `a.rs` and `b.rs`, `B` changing `c.rs`, seen from
/// `src/`.
fn snapshot() -> Snapshot {
    let mut first = commit(A, "A", Some(0xAA));
    first.files = vec![change("src/a.rs"), change("src/b.rs")];
    let mut second = commit(B, "B", Some(A));
    second.files = vec![change("src/c.rs")];
    let info = RepoInfo {
        commits: vec![second, first],
        ..base_info()
    };
    Snapshot {
        workdir: PathBuf::new(),
        git_dir: PathBuf::new(),
        cwd_prefix: "src".to_string(),
        ids: IdAllocator::new(info.collect_entities()),
        info,
    }
}

#[test]
fn a_commit_takes_working_files_or_every_local_change() {
    let (zz, a, b) = (local_changes(2), working_file("a.rs"), working_file("b.rs"));

    assert_eq!(commit_files(&[&a, &b]).unwrap(), ["a.rs", "b.rs"]);
    assert_eq!(commit_files(&[&zz]).unwrap(), ["zz"]);
    assert_eq!(
        commit_files(&[&a, &commit_row(A)]),
        Err("move to local changes or select files")
    );
    assert_eq!(
        commit_files(&[]),
        Err("move to local changes or select files")
    );
    assert_eq!(commit_files(&[&local_changes(0)]), Err("no local changes"));
}

/// An argument-less `loom commit` commits the index.
#[test]
fn a_commit_never_goes_without_a_file_argument() {
    let unnamed = row(RowKind::LocalChanges { count: 1 }, None);
    let empty = row(RowKind::LocalChanges { count: 1 }, Some(""));

    assert_eq!(
        commit_files(&[&unnamed]),
        Err("this row has nothing to commit")
    );
    assert_eq!(
        commit_files(&[&empty]),
        Err("this row has nothing to commit")
    );
}

#[test]
fn a_fold_takes_rows_that_all_name_something() {
    let a = working_file("a.rs");

    assert_eq!(fold_sources(&[&a]).unwrap(), ["a.rs"]);
    assert_eq!(
        fold_sources(&[&a, &upstream_row()]),
        Err("select source rows first")
    );
    assert_eq!(fold_sources(&[]), Err("select source rows first"));
}

#[test]
fn a_move_takes_one_commit() {
    assert_eq!(move_source(&[&commit_row(A)]), Ok(oid(A)));
    assert_eq!(
        move_source(&[&working_file("a.rs")]),
        Err("move to a commit")
    );
    assert_eq!(move_source(&[]), Err("move to a commit"));
    assert_eq!(
        move_source(&[&commit_row(A), &commit_row(B)]),
        Err("one commit at a time")
    );
}

#[test]
fn a_branch_starts_on_the_row_or_at_the_base() {
    let info = RepoInfo {
        commits: vec![commit(A, "A", Some(0xAA))],
        branches: vec![branch("top", A, None)],
        ..base_info()
    };
    let on_commit = (Some(oid(A).to_string()), oid(A));
    let on_branch = (Some("top".to_string()), oid(A));

    assert_eq!(branch_base(&info, &commit_row(A)), on_commit);
    assert_eq!(branch_base(&info, &branch_row("top", None)), on_branch);
    assert_eq!(
        branch_base(&info, &working_file("a.rs")),
        (None, oid(0xAA)),
        "anywhere else is the base, with no `--on`"
    );
}

#[test]
fn a_split_by_file_leaves_something_for_the_second_commit() {
    let snapshot = snapshot();
    let (a, b) = (commit_file(A, "src/a.rs"), commit_file(A, "src/b.rs"));

    assert_eq!(
        split(&snapshot, &[&a]),
        Ok(Split::Files(Action::Split {
            commit: oid(A).to_string(),
            files: vec!["a.rs".to_string()],
            hunks: None,
        })),
        "the file as the CLI takes it, from the cwd"
    );
    assert_eq!(
        split(&snapshot, &[&a, &b]),
        Err("leave at least one file for the second commit")
    );
    assert_eq!(
        split(&snapshot, &[&commit_file(B, "src/c.rs")]),
        Err("one file only, split its hunks with S")
    );
    assert_eq!(
        split(&snapshot, &[&a, &commit_file(B, "src/c.rs")]),
        Err("one commit at a time")
    );
}

#[test]
fn a_split_of_a_commit_row_is_by_hunk() {
    assert_eq!(
        split(&snapshot(), &[&commit_row(A)]),
        Ok(Split::Hunks(oid(A)))
    );
}

#[test]
fn a_drop_takes_one_row_or_files_together() {
    let (a, b) = (working_file("a.rs"), working_file("b.rs"));
    let dropping = |targets: &[&str]| {
        Ok(Action::Drop {
            targets: targets.iter().map(|t| t.to_string()).collect(),
        })
    };

    assert_eq!(drop(&[&a, &b]), dropping(&["a.rs", "b.rs"]));
    assert_eq!(drop(&[&local_changes(2)]), dropping(&["zz"]));
    assert_eq!(drop(&[&branch_row("top", None)]), dropping(&["top"]));
    assert_eq!(
        drop(&[&commit_row(A), &commit_row(B)]),
        Err("only files can be dropped together")
    );
    assert_eq!(
        drop(&[&upstream_row()]),
        Err("move to a commit, branch, file, or local changes")
    );
    assert_eq!(
        drop(&[]),
        Err("move to a commit, branch, file, or local changes")
    );
}

#[test]
fn an_absorb_takes_files_else_every_local_change() {
    let info = RepoInfo {
        working_changes: vec![change("a.rs"), change("b.rs")],
        ..base_info()
    };
    let a = working_file("a.rs");
    let absorbing = |files: &[&str]| {
        Ok(Action::Absorb {
            files: files.iter().map(|f| f.to_string()).collect(),
        })
    };

    assert_eq!(absorb(&info, &[&a], true), absorbing(&["a.rs"]));
    assert_eq!(absorb(&info, &[&local_changes(2)], true), absorbing(&[]));
    assert_eq!(
        absorb(&info, &[&commit_row(A)], false),
        absorbing(&[]),
        "under the cursor, any other row stands for every change"
    );
    assert_eq!(
        absorb(&info, &[&commit_row(A)], true),
        Err("select files or local changes")
    );
    assert_eq!(absorb(&base_info(), &[&a], true), Err("no local changes"));
}

/// Both would stand for every change.
#[test]
fn an_absorb_refuses_an_empty_selection_or_an_unnamed_file() {
    let info = RepoInfo {
        working_changes: vec![change("a.rs"), change("b.rs")],
        ..base_info()
    };
    let unnamed = Row {
        target: None,
        ..working_file("a.rs")
    };
    let empty = Row {
        target: Some(String::new()),
        ..working_file("a.rs")
    };

    assert_eq!(
        absorb(&info, &[], true),
        Err("select files or local changes")
    );
    for rows in [
        vec![&unnamed],
        vec![&empty],
        vec![&working_file("b.rs"), &unnamed],
    ] {
        assert_eq!(
            absorb(&info, &rows, rows.len() > 1),
            Err("this row has nothing to absorb")
        );
    }
}

#[test]
fn a_command_on_a_branch_takes_one_or_asks() {
    let top = branch_row("top", None);

    assert_eq!(one_branch(&[&top], true), Ok(Some("top".to_string())));
    assert_eq!(one_branch(&[&top], false), Ok(Some("top".to_string())));
    assert_eq!(one_branch(&[&commit_row(A)], false), Ok(None));
    assert_eq!(
        one_branch(&[&commit_row(A)], true),
        Err("select one branch")
    );
    assert_eq!(
        one_branch(&[&top, &branch_row("other", None)], true),
        Err("select one branch")
    );
}

#[test]
fn a_force_push_names_its_branch() {
    let top = || Some("top".to_string());

    assert_eq!(push(None, false), Ok(Action::Push { branch: None }));
    assert_eq!(push(top(), false), Ok(Action::Push { branch: top() }));
    assert_eq!(
        push(top(), true),
        Ok(Action::ForcePush {
            branch: "top".to_string()
        })
    );
    assert_eq!(push(None, true), Err("move to a branch"));
}

#[test]
fn only_a_branch_with_an_upstream_has_a_remote_version() {
    let pushed = branch_row("top", Some(RemoteStatus::Different));
    let local = branch_row("local", None);

    assert_eq!(remote_branch(&[&pushed]), Some("top".to_string()));
    assert_eq!(remote_branch(&[&local]), None);
    assert_eq!(remote_branch(&[&pushed, &local]), None);
    assert_eq!(remote_branch(&[&commit_row(A)]), None);
}

#[test]
fn a_reword_asks_a_branch_for_its_name_first() {
    assert_eq!(
        reword(&commit_row(A)),
        Ok(Reword::Commit(Action::Reword {
            target: oid(A).to_string(),
            name: None,
        }))
    );
    assert_eq!(
        reword(&branch_row("top", None)),
        Ok(Reword::Branch {
            branch: "top".to_string(),
            name: "top".to_string(),
        })
    );
    assert_eq!(
        reword(&working_file("a.rs")),
        Err("move to a commit or branch")
    );
}
