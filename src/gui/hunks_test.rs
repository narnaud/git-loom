use super::*;
use crate::core::diff::DiffHunk;
use crate::core::hunk_select::{self, HunkArgs};
use crate::core::hunks::{HunkEntry, HunkOrigin};
use crate::core::test_helpers::TestRepo;
use crate::gui::commands::Cmd;
use crate::gui::model;
use crate::gui::state::tests::{row, select, state, tick_until};
use crate::gui::ui::TextKind;
use crate::session::action::LogEntry;
use crate::session::tree::RowKind;

/// A file of a commit changed in a hunk per text, none kept.
fn file(path: &str, hunks: &[&str]) -> FileEntry {
    let hunks = hunks.iter().map(|text| HunkEntry {
        hunk: DiffHunk {
            text: format!("@@ -1 +1 @@\n-{text}\n+{text}, changed\n"),
            modified_lines: vec![1],
        },
        selected: false,
        origin: HunkOrigin::Commit,
    });
    FileEntry {
        path: path.to_string(),
        hunks: hunks.collect(),
        index_status: 'M',
        worktree_status: ' ',
        binary: false,
    }
}

/// The picker open on `files`, to split the commit "On a".
fn picking(files: Vec<FileEntry>) -> (State, git2::Oid) {
    let mut state = state();
    let commit = match state.rows[row(&state, "On a")].kind {
        RowKind::Commit { oid, .. } => oid,
        _ => unreachable!(),
    };
    state.pick = Some(Pick::Open(HunkPick {
        files,
        current: 0,
        purpose: PickFor::Split { commit },
        title: String::new(),
        error: String::new(),
    }));
    (state, commit)
}

fn kept(state: &State) -> Vec<Vec<bool>> {
    let files = &state.hunk_pick().unwrap().files;
    let hunks = files
        .iter()
        .map(|f| f.hunks.iter().map(|h| h.selected).collect());
    hunks.collect()
}

#[test]
fn a_hunk_is_kept_alone_or_with_its_whole_file() {
    let (mut state, _) = picking(vec![
        file("a.rs", &["one", "two"]),
        file("b.rs", &["three"]),
    ]);

    state.hunk_toggled(1);
    assert_eq!(kept(&state), [vec![false, true], vec![false]]);
    state.hunk_file_toggled(0);
    assert_eq!(kept(&state), [vec![true, true], vec![false]], "some: all");
    state.hunk_file_toggled(0);
    assert_eq!(kept(&state), [vec![false, false], vec![false]], "all: none");

    state.hunk_file_picked(1);
    state.hunk_toggled(0);
    assert_eq!(kept(&state), [vec![false, false], vec![true]]);
    state.hunks_kept(true);
    assert_eq!(state.hunk_pick().unwrap().kept(), (3, 3));
}

#[test]
fn the_picker_shows_each_hunk_under_its_first_line() {
    let mut deleted = file("gone.rs", &["gone"]);
    deleted.hunks[0].hunk.text = "(file deleted)".to_string();
    let (mut state, _) = picking(vec![file("src/a.rs", &["one", "two"]), deleted]);
    state.hunk_toggled(1);

    let pick = state.hunk_pick().unwrap();
    let lines = model::hunk_lines(pick, 4);
    let read: Vec<_> = (lines.iter())
        .map(|l| (l.hunk, l.first, l.kind, l.kept, l.text.as_str()))
        .collect();
    assert_eq!(
        read,
        [
            (0, true, TextKind::HunkHeader, false, "@@ -1 +1 @@"),
            (0, false, TextKind::Removed, false, "-one"),
            (0, false, TextKind::Added, false, "+one, changed"),
            (1, true, TextKind::HunkHeader, true, "@@ -1 +1 @@"),
            (1, false, TextKind::Removed, true, "-two"),
            (1, false, TextKind::Added, true, "+two, changed"),
        ]
    );
    let files = model::hunk_files(pick, "src");
    let read: Vec<_> = (files.iter())
        .map(|f| (f.path.as_str(), f.staged.as_str(), f.checked, f.mixed))
        .collect();
    assert_eq!(
        read,
        [
            ("a.rs", "M", false, true),
            ("../gone.rs", "M", false, false)
        ]
    );
    assert_eq!(model::picking(pick).summary, "1 of 3 hunks kept");

    state.hunk_file_picked(1);
    let lines = model::hunk_lines(state.hunk_pick().unwrap(), 4);
    assert_eq!(
        (lines[0].kind, lines[0].text.as_str()),
        (TextKind::Index, "(file deleted)"),
        "a label, not a line of the file"
    );
}

#[test]
fn a_split_keeps_a_hunk_on_each_side_then_asks_its_message() {
    let (mut state, commit) = picking(vec![file("a.rs", &["one", "two"])]);

    assert_eq!(state.confirm_pick(), None);
    let pick = state.hunk_pick().expect("still open");
    assert_eq!(pick.error, "Nothing kept for the first commit");
    state.hunks_kept(true);
    assert_eq!(state.confirm_pick(), None);
    let pick = state.hunk_pick().expect("still open");
    assert_eq!(pick.error, "Nothing left for the second commit");

    state.hunk_toggled(1);
    assert_eq!(state.confirm_pick(), None);
    assert!(state.hunk_pick().is_none());
    let form = state.form().expect("the message asked");
    assert!(form.detail.starts_with("The hunks kept"), "{}", form.detail);
    let read = [file("a.rs", &["one", "two"])];
    let from = hunk_select::fingerprint(&commit.to_string(), None, &read);
    assert_eq!(
        state.accept("First half", None, 0),
        Some(Action::Split {
            commit: commit.to_string(),
            files: Vec::new(),
            hunks: Some(HunkArgs::new(vec!["a.rs:1".to_string()], Some(from))),
            message: Some("First half".to_string()),
        })
    );
}

/// `Tab` still reaches the panes under it.
#[test]
fn nothing_starts_under_the_hunk_picker() {
    let (mut state, _) = picking(vec![file("a.rs", &["one"])]);
    select(&mut state, "Loose", &[]);

    assert_eq!(state.prepare(Cmd::Drop), None);
    assert!(!state.row_menu(row(&state, "Loose")));
    state.hunks_cancelled();
    assert!(matches!(
        state.prepare(Cmd::Drop),
        Some(Action::Drop { .. })
    ));
}

#[test]
fn a_pick_cancelled_runs_nothing() {
    let (mut state, _) = picking(vec![file("a.rs", &["one", "two"])]);

    state.hunks_cancelled();

    assert!(state.hunk_pick().is_none());
    assert!(state.form().is_none());
    assert_eq!(
        state.status(),
        (Level::Plain, "split cancelled".to_string())
    );
}

#[test]
fn the_hunks_of_a_commit_are_read_before_the_picker_opens() {
    let repo = TestRepo::new_with_remote();
    repo.write_file("a.txt", "a\n");
    repo.write_file("b.txt", "b\n");
    repo.stage_files(&["a.txt", "b.txt"]);
    repo.commit_staged("Two files");
    // Through git, as the commit above: `TestRepo::commit` would write the
    // index libgit2 read before it, which has neither file.
    repo.write_file("c.txt", "c\n");
    repo.stage_files(&["c.txt"]);
    repo.commit_staged("One file");
    let mut state = repo.in_dir(State::load).unwrap();

    select(&mut state, "Two files", &[]);
    assert_eq!(state.prepare(Cmd::SplitHunks), None);
    let sid = state.rows[row(&state, "Two files")].sid.clone();
    let reading = format!("loom split -p {sid}");
    assert_eq!(state.reading(), Some(reading.as_str()));
    assert!(state.busy());
    tick_until(&mut state, |state| state.hunk_pick().is_some());
    let paths: Vec<&str> = (state.hunk_pick().unwrap().files.iter())
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(paths, ["a.txt", "b.txt"]);
    assert!(!state.busy());

    state.hunks_cancelled();
    select(&mut state, "One file", &[]);
    state.prepare(Cmd::SplitHunks);
    tick_until(&mut state, |state| !state.busy());
    assert!(state.hunk_pick().is_none());
    assert_eq!(
        state.status(),
        (
            Level::Warn,
            "split: one hunk only, nothing to split".to_string()
        )
    );
}

#[test]
fn a_cancel_while_the_hunks_are_read_leaves_the_read_to_end() {
    let repo = TestRepo::new_with_remote();
    repo.write_file("a.txt", "a\n");
    repo.write_file("b.txt", "b\n");
    repo.stage_files(&["a.txt", "b.txt"]);
    repo.commit_staged("Two files");
    let mut state = repo.in_dir(State::load).unwrap();
    select(&mut state, "Two files", &[]);
    assert_eq!(state.prepare(Cmd::SplitHunks), None);

    state.hunks_cancelled();

    assert!(state.reading().is_some());
    assert!(state.busy());
    tick_until(&mut state, |state| state.hunk_pick().is_some());
}

#[test]
fn a_close_refused_while_the_hunks_are_read_is_not_said_once_they_are() {
    let repo = TestRepo::new_with_remote();
    repo.write_file("a.txt", "a\n");
    repo.write_file("b.txt", "b\n");
    repo.stage_files(&["a.txt", "b.txt"]);
    repo.commit_staged("Two files");
    let mut state = repo.in_dir(State::load).unwrap();
    select(&mut state, "Two files", &[]);
    state.prepare(Cmd::SplitHunks);

    assert!(!state.may_close());
    tick_until(&mut state, |state| state.hunk_pick().is_some());

    assert_eq!(state.status(), (Level::Plain, String::new()));
}

#[test]
fn what_a_read_sent_last_is_logged_before_how_it_ended() {
    let mut state = state();
    let command = "loom split -p a1".to_string();
    let job = state
        .worker
        .read(command.clone(), &state.snapshot.git_dir, || {
            crate::core::ui::message(Level::Warn, "Skipped `c.txt`");
            Ok((Vec::new(), Vec::new()))
        });
    while !job.is_finished() {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    state.pick = Some(Pick::Reading {
        job,
        purpose: PickFor::Split {
            commit: git2::Oid::ZERO_SHA1,
        },
        command,
    });

    assert!(state.advance_pick());

    assert_eq!(
        state.log().last().unwrap().lines,
        [
            (Level::Warn, "Skipped `c.txt`".to_string()),
            (Level::Warn, "No changes to pick".to_string()),
        ]
    );
}

#[test]
fn why_a_confirm_was_refused_goes_once_the_pick_changes() {
    let (mut state, _) = picking(vec![file("a.rs", &["one", "two"])]);
    state.confirm_pick();
    assert_eq!(
        state.hunk_pick().unwrap().error,
        "Nothing kept for the first commit"
    );

    state.hunk_toggled(0);

    assert_eq!(state.hunk_pick().unwrap().error, "");
}

#[test]
fn the_message_of_a_split_cancelled_ends_it_as_its_picker_would() {
    let (mut state, _) = picking(vec![file("a.rs", &["one", "two"])]);
    state.worker.log.push(LogEntry {
        command: "loom split -p a".to_string(),
        lines: Vec::new(),
    });
    state.hunk_toggled(0);
    assert_eq!(state.confirm_pick(), None, "its message is asked");

    state.form_cancelled();

    assert_eq!(
        state.status(),
        (Level::Plain, "split cancelled".to_string())
    );
    let lines = &state.log().last().unwrap().lines;
    assert_eq!(lines, &[(Level::Warn, "Cancelled".to_string())]);
}
