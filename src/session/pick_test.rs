use std::path::PathBuf;

use super::*;
use crate::core::diff::DiffHunk;
use crate::core::hunks::{HunkEntry, HunkOrigin};
use crate::core::repo::FileChange;
use crate::core::test_helpers::{TestRepo, base_info, oid};
use crate::session::action::execute_action;
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
    let by_id = PickFor::commit_hunks(&dirty);
    assert!(matches!(by_id, Ok(PickFor::CommitHunks)));
    assert_eq!(
        PickFor::commit_hunks(&base_info()).err(),
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

    assert_eq!(pick.read(), Read::Commit(oid(B)));
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
    let by_id = PickFor::CommitHunks.command_line("zz", sid);
    assert_eq!(by_id, "loom commit -p", "it stages nothing");
    assert_eq!(fold.command_line("zz", sid), "loom fold -p <zz>");
    assert_eq!(split.command_line("zz", sid), "loom split -p <01>");
}

#[test]
fn a_pick_reads_every_local_change_or_the_commit_it_takes_from() {
    let fold = |commit| PickFor::Fold {
        sources: Vec::new(),
        targets: Vec::new(),
        commit,
    };

    assert_eq!(PickFor::Commit.read(), Read::Staging);
    assert_eq!(PickFor::CommitHunks.read(), Read::Worktree);
    assert_eq!(fold(None).read(), Read::Worktree);
    assert_eq!(fold(Some(oid(A))).read(), Read::Commit(oid(A)));
    assert_eq!(
        PickFor::Split { commit: oid(A) }.read(),
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
    assert_eq!(PickFor::CommitHunks.nothing_to_pick(&[]), none);
    assert_eq!(PickFor::CommitHunks.nothing_to_pick(&one), None);
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
fn a_commit_of_hunks_names_them_by_id_against_the_stamp() {
    let picked = [file("a.rs", &[false, true])];
    let stamp = [Some(oid(A)), None];
    let source = staging::stamp_source(&stamp);

    assert_eq!(
        commit(CommitDest::Integration, "Add".to_string(), &picked, &stamp),
        Ok(Action::CommitHunks {
            dest: CommitDest::Integration,
            message: "Add".to_string(),
            hunks: HunkArgs::new(
                vec!["a.rs:2".to_string()],
                Some(hunk_select::fingerprint(&source, None, &picked)),
            ),
        })
    );
    let none = [file("a.rs", &[false, false])];
    assert_eq!(
        commit(CommitDest::Integration, "Add".to_string(), &none, &stamp),
        Err(("Nothing kept for the commit", "keep at least one hunk"))
    );
}

/// `a.txt` of twenty lines committed, then its first and last changed: two
/// hunks in the working tree.
fn repo_with_two_hunks() -> TestRepo {
    let repo = TestRepo::new_with_remote();
    let lines: Vec<String> = (1..=20).map(|n| format!("line {n}")).collect();
    repo.write_file("a.txt", &(lines.join("\n") + "\n"));
    repo.stage_files(&["a.txt"]);
    repo.commit_staged("Twenty lines");
    let mut changed = lines;
    changed[0] = "first, changed".to_string();
    changed[19] = "last, changed".to_string();
    repo.write_file("a.txt", &(changed.join("\n") + "\n"));
    repo
}

/// Read every local change, as a pick to commit does, keeping the first
/// hunk alone, and the commit of it.
fn commit_first_hunk(repo: &TestRepo) -> Action {
    let read = PickFor::CommitHunks.read();
    let (mut picked, stamp) = read.run(&repo.workdir()).unwrap();
    let hunks = &mut picked[0].hunks;
    assert_eq!(hunks.len(), 2);
    (hunks[0].selected, hunks[1].selected) = (true, false);
    for file in &mut picked[1..] {
        file.hunks[0].selected = false;
    }
    commit(
        CommitDest::Integration,
        "First".to_string(),
        &picked,
        &stamp,
    )
    .unwrap()
}

fn run(repo: &TestRepo, action: Action) -> Result<()> {
    let theme = crate::core::graph::Theme::dark();
    repo.in_dir(|| execute_action(action, "commit", repo.repo.path(), &theme))
}

#[test]
fn a_commit_of_hunks_takes_those_picked_and_leaves_the_rest() {
    let repo = repo_with_two_hunks();
    let action = commit_first_hunk(&repo);

    run(&repo, action).unwrap();

    let committed = repo.diff_commit(&repo.head_oid().to_string());
    assert!(committed.contains("+first, changed"), "{committed}");
    assert!(!committed.contains("last, changed"), "{committed}");
    assert!(repo.read_file("a.txt").contains("last, changed"));
}

/// The window stamps a binary file as the command does, else every pick
/// beside one would be refused as moved.
#[test]
fn a_commit_of_hunks_beside_a_binary_file_leaves_it_out_or_takes_it() {
    let repo = repo_with_two_hunks();
    let binary = repo.workdir().join("b.bin");
    std::fs::write(&binary, [0u8, 1, 2]).unwrap();
    repo.stage_files(&["b.bin"]);
    repo.commit_staged("A binary file");
    std::fs::write(&binary, [0u8, 1, 2, 3]).unwrap();

    run(&repo, commit_first_hunk(&repo)).unwrap();
    let committed = repo.diff_commit(&repo.head_oid().to_string());
    assert!(committed.contains("+first, changed"), "{committed}");
    assert!(!committed.contains("b.bin"), "{committed}");

    let (mut picked, stamp) = PickFor::CommitHunks.read().run(&repo.workdir()).unwrap();
    for file in &mut picked {
        file.hunks[0].selected = file.path == "b.bin";
    }
    let dest = CommitDest::Integration;
    let action = commit(dest, "Binary".to_string(), &picked, &stamp).unwrap();
    run(&repo, action).unwrap();
    let committed = repo.diff_commit(&repo.head_oid().to_string());
    assert!(committed.contains("b.bin"), "{committed}");
    assert!(repo.read_file("a.txt").contains("last, changed"));
}

#[test]
fn a_pick_staged_at_once_reads_the_same_files_unstamped() {
    let repo = repo_with_two_hunks();
    std::fs::write(repo.workdir().join("b.bin"), [0u8, 1, 2]).unwrap();

    let (staged, unstamped) = PickFor::Commit.read().run(&repo.workdir()).unwrap();
    let (by_id, stamp) = PickFor::CommitHunks.read().run(&repo.workdir()).unwrap();

    let paths = |files: &[FileEntry]| files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
    assert_eq!(paths(&staged), ["a.txt", "b.bin"]);
    assert_eq!(paths(&by_id), paths(&staged));
    assert!(unstamped.is_empty());
    let blob = git2::Oid::hash_object(git2::ObjectType::Blob, &[0, 1, 2]).unwrap();
    assert_eq!(stamp, [Some(blob)]);
}

#[test]
fn a_commit_of_hunks_from_a_tree_that_moved_since_is_refused() {
    let repo = repo_with_two_hunks();
    let action = commit_first_hunk(&repo);
    let head = repo.head_oid();
    let edited = repo
        .read_file("a.txt")
        .replace("line 10", "line 10, edited");
    repo.write_file("a.txt", &edited);

    let error = run(&repo, action).unwrap_err().to_string();

    assert!(error.contains("The hunks changed"), "{error}");
    assert_eq!(repo.head_oid(), head);
    assert_eq!(repo.read_file("a.txt"), edited);
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
            message: None,
        })
    );
    assert_eq!(
        split(oid(A), &[file("a.rs", &[true, true])]),
        Err((
            "Nothing left for the second commit",
            "leave at least one hunk for the second commit"
        ))
    );
    assert_eq!(
        split(oid(A), &[file("a.rs", &[false, false])]),
        Err((
            "Nothing kept for the first commit",
            "keep at least one hunk for the first commit"
        ))
    );
}
