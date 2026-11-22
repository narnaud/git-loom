use super::*;
use crate::gui::state::tests::{info, row, select, selected, state, state_of};
use crate::session::paused::test_support::repo_with_a_paused_drop;

/// The command argument of the row reading `text`.
fn target(state: &State, text: &str) -> String {
    let index = row(state, text);
    state.rows[index].target.clone().unwrap()
}

/// The titles of the menu of the selection, `-` between its groups, and
/// `(…)` around what the selection does not take.
fn menu_of(state: &State) -> Vec<String> {
    let lines = state.menu().into_iter().map(|entry| match entry {
        None => "-".to_string(),
        Some(entry) if entry.enabled => entry.title.to_string(),
        Some(entry) => format!("({})", entry.title),
    });
    lines.collect()
}

#[test]
fn each_class_of_piece_has_a_menu_of_its_own() {
    let mut state = state();
    let menu = |state: &mut State, text: &str| {
        select(state, text, &[]);
        menu_of(state)
    };

    assert_eq!(
        menu(&mut state, "local changes"),
        ["Absorb", "-", "Drop all local changes"]
    );
    assert_eq!(menu(&mut state, "b.rs"), ["Absorb", "-", "Drop"]);
    assert_eq!(menu(&mut state, "On a"), ["Uncommit", "-", "Drop"]);
    assert_eq!(
        menu(&mut state, "feature-a"),
        [
            "Push",
            "Force push",
            "Take the remote version",
            "-",
            "Unmerge",
            "-",
            "Drop"
        ]
    );
    assert_eq!(menu(&mut state, "upstream"), ["Update"]);
    state.clear_selection();
    assert!(state.menu().is_empty(), "nothing selected, nothing to run");
}

#[test]
fn a_menu_greys_out_what_the_selection_does_not_take() {
    let mut state = state();

    select(&mut state, "feature-b", &[]);
    assert_eq!(
        menu_of(&state)[..3],
        ["Push", "Force push", "(Take the remote version)"],
        "a branch never pushed has no remote version"
    );
    select(&mut state, "On a", &["Loose"]);
    assert_eq!(
        menu_of(&state),
        ["(Uncommit)", "-", "(Drop)"],
        "one at a time"
    );
    let mut none = info();
    none.working_changes.clear();
    let mut state = state_of(none);
    select(&mut state, "local changes", &[]);
    assert_eq!(
        menu_of(&state),
        ["(Absorb)", "-", "(Drop all local changes)"]
    );
}

#[test]
fn a_file_of_a_commit_can_be_uncommitted() {
    let mut state = state();
    state.row_toggled(row(&state, "On a"));

    select(&mut state, "z.rs", &[]);

    assert_eq!(menu_of(&state), ["Uncommit"]);
}

#[test]
fn the_inspector_offers_what_the_menu_holds_that_the_selection_takes() {
    let mut state = state();
    select(&mut state, "feature-b", &[]);

    let actions: Vec<&str> = state.actions().iter().map(|entry| entry.title).collect();

    assert_eq!(actions, ["Push", "Force push", "Unmerge", "Drop"]);
    state.clear_selection();
    assert!(state.actions().is_empty());
}

#[test]
fn each_command_makes_the_action_the_tui_makes_of_the_same_rows() {
    let mut state = state();
    let zz = target(&state, "local changes");
    let (a, b) = (target(&state, "a.rs"), target(&state, "b.rs"));
    let on_a = target(&state, "On a");

    select(&mut state, "a.rs", &["b.rs"]);
    assert_eq!(
        state.make(Cmd::Absorb),
        Ok(Action::Absorb {
            files: vec![a.clone(), b.clone()]
        })
    );
    assert_eq!(
        state.make(Cmd::Drop),
        Ok(Action::Drop {
            targets: vec![a, b]
        })
    );
    select(&mut state, "local changes", &[]);
    assert_eq!(
        state.make(Cmd::Absorb),
        Ok(Action::Absorb { files: vec![] })
    );
    assert_eq!(
        state.make(Cmd::Drop),
        Ok(Action::Drop {
            targets: vec![zz.clone()]
        })
    );
    select(&mut state, "On a", &[]);
    assert_eq!(
        state.make(Cmd::Uncommit),
        Ok(Action::Fold {
            sources: vec![on_a],
            target: zz,
            hunks: None,
        })
    );
    select(&mut state, "feature-a", &[]);
    let branch = Some("feature-a".to_string());
    assert_eq!(
        state.make(Cmd::Push),
        Ok(Action::Push {
            branch: branch.clone()
        })
    );
    assert_eq!(
        state.make(Cmd::ForcePush),
        Ok(Action::ForcePush {
            branch: "feature-a".to_string()
        })
    );
    assert_eq!(
        state.make(Cmd::TakeRemote),
        Ok(Action::UpdateBranch {
            branch: "feature-a".to_string()
        })
    );
    assert_eq!(state.make(Cmd::Unmerge), Ok(Action::Unmerge { branch }));
    select(&mut state, "upstream", &[]);
    assert_eq!(state.make(Cmd::Update), Ok(Action::Update));
}

#[test]
fn the_bar_merges_a_branch_whatever_is_selected() {
    let state = state();

    let bar = state.bar();

    assert_eq!(bar.len(), 1);
    assert_eq!((bar[0].title, bar[0].enabled), ("Merge a branch…", true));
    assert_eq!(state.make(Cmd::Merge), Ok(Action::Merge));
}

#[test]
fn nothing_runs_while_an_operation_is_paused() {
    let (repo, _) = repo_with_a_paused_drop();
    let mut state = repo.in_dir(State::load).unwrap();

    assert!(!state.bar()[0].enabled);

    assert_eq!(state.prepare(Cmd::Merge), None);
    assert_eq!(
        state.status(),
        (Level::Warn, "merge: an operation is paused".to_string())
    );
}

#[test]
fn a_command_the_selection_does_not_take_says_why_and_runs_nothing() {
    let mut state = state();
    select(&mut state, "On a", &["Loose"]);

    assert_eq!(state.prepare(Cmd::Drop), None);
    assert_eq!(
        state.status(),
        (
            Level::Warn,
            "drop: only files can be dropped together".to_string()
        )
    );
}

#[test]
fn a_right_click_opens_the_menu_of_the_selection_it_is_part_of() {
    let mut state = state();
    select(&mut state, "a.rs", &["b.rs"]);

    assert!(state.row_menu(row(&state, "b.rs")));
    assert_eq!(selected(&state), ["a.rs", "b.rs"]);
    assert_eq!(state.inspected().unwrap().title, "b.rs", "the cursor moves");

    assert!(state.row_menu(row(&state, "feature-a")));
    assert_eq!(selected(&state), ["feature-a"], "else the piece alone");
}

#[test]
fn a_right_click_on_what_is_not_shown_opens_nothing() {
    let mut state = state();
    select(&mut state, "a.rs", &[]);

    let spacer = state.rows.iter().position(|r| r.key.is_empty()).unwrap();

    assert!(!state.row_menu(spacer));
    assert_eq!(selected(&state), ["a.rs"]);
}

#[test]
fn a_command_is_named_to_the_window_by_its_id() {
    for cmd in Cmd::ALL {
        assert_eq!(Cmd::from_id(cmd.id()), Some(cmd));
    }
    assert_eq!(Cmd::from_id(-1), None);
    assert_eq!(Cmd::from_id(Cmd::ALL.len() as i32), None);
}

#[test]
fn no_menu_opens_while_a_command_runs() {
    let mut state = state();
    let (go, wait) = std::sync::mpsc::channel::<()>();
    state.worker.run("loom update".to_string(), move || {
        let _ = wait.recv();
        Err(crate::core::ui::Cancelled.into())
    });

    assert!(!state.row_menu(row(&state, "feature-a")));
    assert!(selected(&state).is_empty());
    assert_eq!(state.prepare(Cmd::Update), None);

    go.send(()).unwrap();
    while state.busy() {
        state.tick();
    }
    assert!(state.row_menu(row(&state, "feature-a")));
}
