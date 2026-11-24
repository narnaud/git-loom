use super::*;
use test_support::repo_with_a_paused_drop;

fn files(listed: &[(&str, char, char)]) -> Vec<FileChange> {
    let files = listed.iter().map(|&(path, index, worktree)| FileChange {
        path: path.to_string(),
        index,
        worktree,
    });
    files.collect()
}

#[test]
fn a_pause_lists_its_conflicts_first() {
    let (repo, _) = repo_with_a_paused_drop();
    repo.write_file("a.txt", "new\n");

    let op = PausedOp::load(&repo.workdir(), repo.repo.path()).unwrap();

    let paths: Vec<&str> = op.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["f.txt", "a.txt"]);
}

/// Staged, an untracked file joins the tracked files above it: the one now
/// in its place is not the one that followed it.
#[test]
fn the_file_after_one_staged_is_found_wherever_it_now_is() {
    let before = files(&[
        ("x", '!', '!'),
        ("t", ' ', 'M'),
        ("q", '?', '?'),
        ("r", '?', '?'),
    ]);
    let after = files(&[
        ("x", '!', '!'),
        ("q", 'A', ' '),
        ("t", ' ', 'M'),
        ("r", '?', '?'),
    ]);

    assert_eq!(next_after_staging(&before, 2, &after), 3);
}

#[test]
fn the_file_after_one_staged_is_the_one_that_followed_it() {
    let before = files(&[("a", '!', '!'), ("b", '!', '!'), ("c", '?', '?')]);

    let moved = files(&[("b", '!', '!'), ("a", 'M', ' '), ("c", '?', '?')]);
    assert_eq!(next_after_staging(&before, 0, &moved), 0, "b, which a left");
    let gone = files(&[("b", '!', '!'), ("c", '?', '?')]);
    assert_eq!(next_after_staging(&before, 0, &gone), 0, "b");
    let last = files(&[("a", '!', '!'), ("b", '!', '!'), ("c", 'A', ' ')]);
    assert_eq!(next_after_staging(&before, 2, &last), 2, "c, the last");
    assert_eq!(next_after_staging(&before, 2, &[]), 0);
}

#[test]
fn a_file_is_staged_only_while_its_pause_is_the_one_paused() {
    let (repo, _) = repo_with_a_paused_drop();
    let (workdir, git_dir) = (repo.workdir(), repo.repo.path().to_path_buf());
    let op = PausedOp::load(&workdir, &git_dir).unwrap();
    repo.in_dir(crate::resume::abort_run).unwrap();
    repo.write_file("f.txt", "worktree\n");

    let refused = stage(&op, &workdir, &git_dir, "f.txt");

    assert!(refused.is_err());
    let mut index = repo.repo.index().unwrap();
    index.read(true).unwrap();
    let staged = index.get_path(Path::new("f.txt"), 0).unwrap();
    let blob = repo.repo.find_blob(staged.id).unwrap();
    assert_eq!(blob.content(), b"two\n", "the index as the abort left it");
}

/// A continue run elsewhere stops on the next conflict under the same
/// `PausedId`: staging the file then would mark that conflict resolved,
/// markers and all.
#[test]
fn a_file_is_not_staged_once_its_pause_went_on_to_another_step() {
    let repo = crate::core::test_helpers::TestRepo::new_with_remote();
    let mut one = None;
    for subject in ["one", "two", "three"] {
        repo.write_file("f.txt", &format!("{subject}\n"));
        repo.stage_files(&["f.txt"]);
        repo.commit_staged(subject);
        one.get_or_insert(repo.head_oid());
    }
    let one = one.unwrap().to_string();
    repo.in_dir(|| crate::drop::run(vec![one], true)).unwrap();
    let (workdir, git_dir) = (repo.workdir(), repo.repo.path().to_path_buf());
    let op = PausedOp::load(&workdir, &git_dir).unwrap();
    repo.write_file("f.txt", "resolved\n");
    repo.stage_files(&["f.txt"]);
    repo.in_dir(crate::resume::continue_run).unwrap();
    let next = PausedOp::load(&workdir, &git_dir).expect("paused on three");
    assert_eq!(next.state, PausedState::Conflicts);
    assert_eq!(next.id, op.id, "the same operation, at another step");

    let refused = stage(&op, &workdir, &git_dir, "f.txt");

    assert!(refused.is_err());
    let mut index = repo.repo.index().unwrap();
    index.read(true).unwrap();
    assert!(
        index.get_path(Path::new("f.txt"), 0).is_none(),
        "still a conflict"
    );
    assert!(
        stage(&next, &workdir, &git_dir, "f.txt").is_ok(),
        "shown at it"
    );
}
