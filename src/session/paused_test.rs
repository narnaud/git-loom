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
