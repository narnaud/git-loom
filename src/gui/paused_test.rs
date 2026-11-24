use super::*;
use crate::core::test_helpers::TestRepo;
use crate::core::transaction::Resumable;
use crate::gui::commands::Cmd;
use crate::gui::model;
use crate::gui::state::tests::tick_until;
use crate::gui::ui::ChangeKind;
use crate::session::action::Action;
use crate::session::paused::is_conflicted;
use crate::session::paused::test_support::repo_with_a_paused_drop;

/// A window open on a `loom drop` paused on a conflict in `f.txt`.
fn paused() -> (TestRepo, State) {
    let (repo, _) = repo_with_a_paused_drop();
    let state = repo.in_dir(State::load).unwrap();
    (repo, state)
}

/// A window open on a `loom drop` paused on conflicts in `a.txt` to `d.txt`.
fn four_conflicts() -> (TestRepo, State) {
    let repo = TestRepo::new_with_remote();
    let files = ["a.txt", "b.txt", "c.txt", "d.txt"];
    let commit = |subject: &str| {
        for file in files {
            repo.write_file(file, &format!("{subject}\n"));
        }
        repo.stage_files(&files);
        repo.commit_staged(subject);
        repo.head_oid()
    };
    commit("base");
    let one = commit("one");
    commit("two");
    repo.in_dir(|| crate::drop::run(vec![one.to_string()], true))
        .unwrap();
    let state = repo.in_dir(State::load).unwrap();
    (repo, state)
}

fn selected_path(state: &State) -> Option<&str> {
    let index = state.paused_file()?;
    Some(state.paused_files()[index].path.as_str())
}

/// As the command ended; started, it would run where the tests do.
fn ended(state: &mut State, repo: &TestRepo, command: &str) {
    state.worker.run(command.to_string(), || Ok(()));
    repo.in_dir(|| tick_until(state, |state| !state.busy()));
}

#[test]
fn a_pause_lists_the_files_it_leaves_and_shows_the_one_selected() {
    let (_repo, mut state) = paused();

    assert_eq!(state.paused_file(), Some(0), "the first, at first");
    let header = model::local_changes(&state);
    assert_eq!(
        (header.row, header.hint.as_str(), header.headline.as_str()),
        (
            -1,
            "1 file",
            "Conflicts — resolve them, stage each file, then continue."
        )
    );
    let read: Vec<_> = (model::changes(&state).iter())
        .map(|change| (change.path.to_string(), change.kind, change.selected))
        .collect();
    assert_eq!(read, [("f.txt".to_string(), ChangeKind::Conflicted, true)]);
    let shown = state.inspected().unwrap();
    assert_eq!(shown.title, "f.txt");
    assert!(
        shown.diff.contains("+two"),
        "against a HEAD with no f.txt: {}",
        shown.diff
    );
}

#[test]
fn only_the_commands_of_the_pause_apply() {
    let (_repo, mut state) = paused();

    let buttons: Vec<_> = (state.pane_buttons().into_iter())
        .map(|entry| (entry.title, entry.enabled))
        .collect();
    assert_eq!(
        buttons,
        [("Stage", true), ("Abort…", true), ("Continue", true)]
    );
    assert!(state.row_menu(0));
    let menu: Vec<_> = (state.menu().into_iter())
        .map(|entry| entry.map_or("-", |entry| entry.title))
        .collect();
    assert_eq!(menu, ["Stage", "-", "Continue", "Abort…"]);
    let inspector: Vec<_> = state.actions().iter().map(|entry| entry.title).collect();
    assert_eq!(inspector, ["Stage", "Continue", "Abort…"]);
    assert!(matches!(
        state.prepare(Cmd::Continue),
        Some(Action::Continue { .. })
    ));
    assert!(matches!(
        state.prepare(Cmd::Abort),
        Some(Action::Abort { .. })
    ));
    assert_eq!(state.prepare(Cmd::Absorb), None);
    assert_eq!(
        state.status(),
        (Level::Warn, "absorb: an operation is paused".to_string())
    );
}

#[test]
fn a_click_or_an_arrow_selects_a_file_and_the_view_follows_it() {
    let (_repo, mut state) = four_conflicts();

    state.row_pressed(2, false, false);
    assert_eq!(selected_path(&state), Some("c.txt"));
    state.move_cursor(true);
    assert_eq!(selected_path(&state), Some("d.txt"));
    state.move_cursor(true);
    assert_eq!(selected_path(&state), Some("d.txt"), "the last, still");
    state.move_cursor(false);
    assert_eq!(state.cursor_change(), Some(2));
}

#[test]
fn a_file_staged_is_added_and_the_one_that_followed_it_selected() {
    let (repo, mut state) = four_conflicts();
    repo.write_file("a.txt", "resolved\n");

    repo.in_dir(|| state.prepare(Cmd::Stage));

    let a = state
        .paused_files()
        .iter()
        .find(|file| file.path == "a.txt");
    assert!(a.is_some_and(|a| !is_conflicted(a)), "listed, resolved");
    assert_eq!(selected_path(&state), Some("b.txt"));
    assert_eq!(state.status(), (Level::Plain, "staged a.txt".to_string()));
    repo.write_file("b.txt", "resolved\n");
    repo.in_dir(|| state.paused_file_activated(0));
    assert_eq!(selected_path(&state), Some("c.txt"), "by a double click");
}

#[test]
fn nothing_is_staged_under_the_log() {
    let (repo, mut state) = four_conflicts();
    state.open_log();

    repo.in_dir(|| state.paused_file_activated(0));

    assert!(state.paused_files().iter().all(is_conflicted));
}

/// The window reads the repository only after its own commands: a pause
/// ended in a terminal is still shown.
#[test]
fn a_pause_ended_elsewhere_stages_nothing_and_is_shown_no_longer() {
    let (repo, mut state) = paused();
    repo.in_dir(crate::resume::abort_run).unwrap();

    repo.in_dir(|| state.prepare(Cmd::Stage));

    assert_eq!(
        state.status().1,
        "The paused operation changed since it was shown"
    );
    let detail = state.form().map(|form| form.detail.as_str());
    assert_eq!(detail, Some("The window shows it as it is now"));
    assert!(state.paused().is_none());
    assert_eq!(state.paused_file(), None);
}

#[test]
fn a_commit_that_paused_keeps_its_message_until_it_is_continued_through() {
    let continued = |cmd: Cmd, the_commits: bool| {
        let (repo, mut state) = paused();
        state.draft.message = "Add it".to_string();
        state.draft.running = true;
        ended(&mut state, &repo, "loom commit");
        assert!(state.draft.paused);
        if the_commits {
            // The drop's pause stands for the commit's.
            state.paused.as_mut().unwrap().command = Some(Resumable::Commit);
        }
        assert!(state.prepare(cmd).is_some());
        repo.in_dir(crate::resume::abort_run).unwrap();
        ended(&mut state, &repo, "loom continue");
        state.draft.message.clone()
    };

    assert_eq!(continued(Cmd::Continue, true), "");
    assert_eq!(continued(Cmd::Abort, true), "Add it", "aborted");
    assert_eq!(continued(Cmd::Continue, false), "Add it", "another pause");
}
