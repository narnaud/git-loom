use super::*;
use crate::core::test_helpers::TestRepo;
use crate::gui::state::tests::{info, row, select, selected, state, state_of};
use crate::session::paused::test_support::repo_with_a_paused_drop;

/// The command argument of the row reading `text`.
fn target(state: &State, text: &str) -> String {
    let index = row(state, text);
    state.rows[index].target.clone().unwrap()
}

/// The action `cmd` makes of the selection; none picks hunks or a place.
fn action(state: &State, cmd: Cmd) -> Result<Action, Refusal> {
    state.make(cmd).map(|made| match made {
        Made::Run(action) => action,
        Made::Pick(_) => panic!("{cmd:?} picks hunks"),
        Made::Aim(_) => panic!("{cmd:?} waits for a click"),
        Made::Stage => panic!("{cmd:?} stages at once"),
    })
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
        [
            "Commit…",
            "Commit hunks…",
            "-",
            "Fold into…",
            "Fold hunks into…",
            "Absorb",
            "-",
            "Drop all local changes"
        ]
    );
    assert_eq!(
        menu(&mut state, "b.rs"),
        [
            "Commit…",
            "Commit hunks…",
            "-",
            "Fold into…",
            "Fold hunks into…",
            "Absorb",
            "-",
            "Drop"
        ]
    );
    assert_eq!(
        menu(&mut state, "On a"),
        [
            "Reword…",
            "Split…",
            "-",
            "Fold into…",
            "Fold hunks into…",
            "Move…",
            "Uncommit",
            "-",
            "New branch here…",
            "-",
            "Drop"
        ]
    );
    assert_eq!(
        menu(&mut state, "feature-a"),
        [
            "Push",
            "Force push",
            "Take the remote version…",
            "-",
            "Rename…",
            "New branch here…",
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
        ["Push", "Force push", "(Take the remote version…)"],
        "a branch never pushed has no remote version"
    );
    select(&mut state, "On a", &["Loose"]);
    assert_eq!(
        menu_of(&state),
        [
            "(Reword…)",
            "(Split…)",
            "-",
            "(Fold into…)",
            "(Fold hunks into…)",
            "(Move…)",
            "(Uncommit)",
            "-",
            "(New branch here…)",
            "-",
            "(Drop)"
        ],
        "one at a time"
    );
    let mut none = info();
    none.working_changes.clear();
    let mut state = state_of(none);
    select(&mut state, "local changes", &[]);
    assert_eq!(
        menu_of(&state),
        [
            "(Commit…)",
            "(Commit hunks…)",
            "-",
            "(Fold into…)",
            "(Fold hunks into…)",
            "(Absorb)",
            "-",
            "(Drop all local changes)"
        ]
    );
}

#[test]
fn a_file_of_a_commit_splits_out_while_the_commit_keeps_another() {
    let mut state = state();
    state.row_toggled(row(&state, "On a"));
    state.row_toggled(row(&state, "Second on b"));

    select(&mut state, "y.rs", &["x.rs"]);
    let refusal = state.make(Cmd::Uncommit).err();
    assert_eq!(refusal, Some("one commit file at a time"));
    select(&mut state, "z.rs", &[]);
    assert_eq!(
        menu_of(&state),
        ["(Split into its own commit…)", "Fold into…", "Uncommit"],
        "its commit's only file"
    );
    select(&mut state, "y.rs", &[]);
    assert_eq!(
        menu_of(&state),
        ["Split into its own commit…", "Fold into…", "Uncommit"]
    );
}

#[test]
fn the_inspector_offers_what_the_menu_holds_that_the_selection_takes() {
    let mut state = state();
    select(&mut state, "feature-b", &[]);

    let actions: Vec<&str> = state.actions().iter().map(|entry| entry.title).collect();

    assert_eq!(
        actions,
        [
            "Push",
            "Force push",
            "Rename…",
            "New branch here…",
            "Unmerge",
            "Drop"
        ]
    );
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
        action(&state, Cmd::Absorb),
        Ok(Action::Absorb {
            files: vec![a.clone(), b.clone()]
        })
    );
    assert_eq!(
        action(&state, Cmd::Drop),
        Ok(Action::Drop {
            targets: vec![a, b]
        })
    );
    select(&mut state, "local changes", &[]);
    assert_eq!(
        action(&state, Cmd::Absorb),
        Ok(Action::Absorb { files: vec![] })
    );
    assert_eq!(
        action(&state, Cmd::Drop),
        Ok(Action::Drop {
            targets: vec![zz.clone()]
        })
    );
    select(&mut state, "On a", &[]);
    assert_eq!(
        action(&state, Cmd::Uncommit),
        Ok(Action::Fold {
            sources: vec![on_a],
            target: zz,
            hunks: None,
        })
    );
    select(&mut state, "feature-a", &[]);
    let branch = Some("feature-a".to_string());
    assert_eq!(
        action(&state, Cmd::Push),
        Ok(Action::Push {
            branch: branch.clone()
        })
    );
    assert_eq!(
        action(&state, Cmd::ForcePush),
        Ok(Action::ForcePush {
            branch: "feature-a".to_string()
        })
    );
    assert_eq!(
        action(&state, Cmd::TakeRemote),
        Ok(Action::UpdateBranch {
            branch: "feature-a".to_string()
        })
    );
    assert_eq!(action(&state, Cmd::Unmerge), Ok(Action::Unmerge { branch }));
    select(&mut state, "upstream", &[]);
    assert_eq!(action(&state, Cmd::Update), Ok(Action::Update));
}

#[test]
fn the_bar_makes_and_merges_a_branch_whatever_is_selected() {
    let state = state();

    let bar: Vec<(&str, bool)> = state.bar().iter().map(|e| (e.title, e.enabled)).collect();

    assert_eq!(bar, [("New branch…", true), ("Merge a branch…", true)]);
    assert_eq!(action(&state, Cmd::Merge), Ok(Action::Merge));
}

#[test]
fn the_bar_runs_nothing_while_an_operation_is_paused() {
    let (repo, _) = repo_with_a_paused_drop();
    let mut state = repo.in_dir(State::load).unwrap();

    assert!(state.bar().iter().all(|entry| !entry.enabled));

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

// ── What a command asks before it runs ──
//
// Never through `command` or `form_accepted`: an action they start would run
// a loom command in the process directory.

/// Open the form `cmd` asks the selection with.
fn ask_for(state: &mut State, cmd: Cmd) -> &Form {
    let made = action(state, cmd).unwrap();
    let Ok(form) = state.ask(made) else {
        panic!("{cmd:?} asks nothing");
    };
    state.open_form(form);
    state.form().unwrap()
}

#[test]
fn a_commit_asks_its_message_and_where_it_goes() {
    let mut state = state();
    select(&mut state, "a.rs", &["b.rs"]);
    let files = vec![target(&state, "a.rs"), target(&state, "b.rs")];

    let form = ask_for(&mut state, Cmd::Commit);
    assert_eq!(form.body, "Commit 2 files");
    assert_eq!(form.input, Input::Text);
    let feature_a = CommitDest::Branch("feature-a".to_string());
    let dests = [
        CommitDest::Integration,
        CommitDest::Branch("feature-b".to_string()),
        feature_a.clone(),
    ];
    assert_eq!((form.dests.as_slice(), form.dest), (&dests[..], 0));

    assert_eq!(
        state.accept(" Add a and b \n", None, 2),
        Some(Action::Commit {
            source: CommitSource::Files(files),
            dest: feature_a,
            message: Some("Add a and b".to_string()),
        })
    );
    select(&mut state, "local changes", &[]);
    let form = ask_for(&mut state, Cmd::Commit);
    assert_eq!(form.body, "Commit every local change");
    assert_eq!(form.dest, 2, "where the last one went");
}

#[test]
fn a_form_of_the_windows_own_refuses_an_empty_answer() {
    let mut state = state();
    select(&mut state, "a.rs", &[]);
    ask_for(&mut state, Cmd::Commit);

    assert_eq!(state.accept("  ", None, 2), None);

    let form = state.form().expect("asked again");
    assert_eq!(form.error, "The message is empty");
    assert_eq!(form.dest, 2, "as it was answered");
}

#[test]
fn a_reword_starts_from_the_message_the_commit_has() {
    let repo = TestRepo::new_with_remote();
    let change_id = "Change-Id: I0123456789abcdef0123456789abcdef01234567";
    let message = format!("Loose on the line\n\nWith a body\n\n{change_id}\n");
    repo.commit(&message, "loose.txt");
    let mut state = repo.in_dir(State::load).unwrap();
    select(&mut state, "Loose on the line", &[]);
    let commit = target(&state, "Loose on the line");

    let form = ask_for(&mut state, Cmd::Reword);
    assert_eq!(form.input, Input::Text);
    assert_eq!(
        form.initial, "Loose on the line\n\nWith a body",
        "the reword keeps its Change-Id"
    );

    assert_eq!(
        state.accept("Loose, reworded", None, 0),
        Some(Action::Reword {
            target: commit,
            message: Some("Loose, reworded".to_string()),
        })
    );
}

#[test]
fn a_rename_is_followed_to_the_branch_and_does_nothing_unchanged() {
    let mut state = state();
    select(&mut state, "feature-a", &[]);

    let form = ask_for(&mut state, Cmd::Reword);
    assert_eq!(form.body, "Rename feature-a");
    assert_eq!(
        (form.input, form.initial.as_str()),
        (Input::Line, "feature-a")
    );
    assert_eq!(state.accept("feature-a ", None, 0), None);
    assert_eq!(
        state.status(),
        (Level::Plain, "Nothing changed".to_string())
    );

    ask_for(&mut state, Cmd::Reword);
    assert_eq!(
        state.accept("feature-z", None, 0),
        Some(Action::Reword {
            target: "feature-a".to_string(),
            message: Some("feature-z".to_string()),
        })
    );
    assert_eq!(state.next_cursor.as_deref(), Some("br:feature-z"));
}

#[test]
fn a_new_branch_is_named_on_the_piece_selected_else_at_the_base() {
    let mut state = state();
    select(&mut state, "On a", &[]);
    let on_a = target(&state, "On a");
    let sid = state.rows[row(&state, "On a")].sid.clone();

    assert_eq!(
        ask_for(&mut state, Cmd::Branch).body,
        format!("New branch on {sid}")
    );
    assert_eq!(
        state.accept("feature-c", None, 0),
        Some(Action::Branch {
            name: "feature-c".to_string(),
            target: Some(on_a),
        })
    );
    assert_eq!(state.next_cursor.as_deref(), Some("br:feature-c"));
    state.clear_selection();
    assert_eq!(
        ask_for(&mut state, Cmd::Branch).body,
        "New branch at the base"
    );
}

#[test]
fn files_of_a_commit_split_out_under_a_message() {
    let mut state = state();
    state.row_toggled(row(&state, "Second on b"));
    select(&mut state, "y.rs", &[]);

    let form = ask_for(&mut state, Cmd::Split);
    assert!(form.body.starts_with("Split "), "{}", form.body);
    assert_eq!(form.input, Input::Text);

    let Some(Action::Split { files, message, .. }) = state.accept("Move y", None, 0) else {
        panic!("a split");
    };
    assert_eq!(files, ["y.rs"]);
    assert_eq!(message.as_deref(), Some("Move y"));
}

#[test]
fn taking_the_remote_version_asks_first() {
    let mut state = state();
    select(&mut state, "feature-a", &[]);

    let form = ask_for(&mut state, Cmd::TakeRemote);
    assert_eq!(form.body, "Take the remote version of feature-a?");
    assert_eq!(form.input, Input::None);

    assert_eq!(
        state.accept("", None, 0),
        Some(Action::UpdateBranch {
            branch: "feature-a".to_string()
        })
    );
}

#[test]
fn what_needs_nothing_runs_without_a_form() {
    let state = state();

    assert!(matches!(state.ask(Action::Update), Err(Action::Update)));
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

#[test]
fn what_is_left_as_it_was_is_told_after_a_refused_answer_too() {
    let mut state = state();
    select(&mut state, "feature-a", &[]);
    ask_for(&mut state, Cmd::Reword);

    assert_eq!(state.accept(" ", None, 0), None);
    assert_eq!(state.form().unwrap().error, "The name is empty");

    assert_eq!(state.accept("feature-a", None, 0), None);
    assert_eq!(
        state.status(),
        (Level::Plain, "Nothing changed".to_string())
    );
}

#[test]
fn a_commit_goes_nowhere_but_to_a_destination_picked() {
    let mut state = state();
    select(&mut state, "a.rs", &[]);
    ask_for(&mut state, Cmd::Commit);

    assert_eq!(state.accept("Add a", None, usize::MAX), None);

    assert_eq!(state.form().unwrap().error, "Pick where the commit goes");
}

#[test]
fn a_new_branch_on_a_branch_names_it() {
    let mut state = state();
    select(&mut state, "feature-a", &[]);

    assert_eq!(
        ask_for(&mut state, Cmd::Branch).body,
        "New branch on feature-a"
    );
}

#[test]
fn the_buttons_under_the_local_changes_take_the_ones_selected() {
    let mut state = state();
    let buttons = |state: &State| {
        let entries = state.pane_buttons().into_iter();
        entries
            .map(|entry| (entry.title, entry.enabled))
            .collect::<Vec<_>>()
    };
    let all = |enabled| {
        [
            ("Absorb", enabled),
            ("Hunks…", enabled),
            ("Commit…", enabled),
        ]
    };

    assert_eq!(buttons(&state), all(false), "nothing selected");
    select(&mut state, "b.rs", &[]);
    assert_eq!(buttons(&state), all(true));
    select(&mut state, "local changes", &[]);
    assert_eq!(buttons(&state), all(true));
    select(&mut state, "Loose", &[]);
    assert_eq!(buttons(&state), all(false));
}

/// `Tab` still reaches the panes under what is over them.
#[test]
fn nothing_starts_under_a_dialog_or_the_log() {
    let mut state = state();
    select(&mut state, "Loose", &[]);
    let drop = |state: &mut State| state.prepare(Cmd::Drop);

    state.open_log();
    assert_eq!(drop(&mut state), None);
    assert!(!state.row_menu(row(&state, "Loose")));
    state.close_log();
    assert_eq!(state.prepare(Cmd::Reword), None, "its dialog opens");
    assert_eq!(drop(&mut state), None);
    state.form_cancelled();

    assert!(matches!(drop(&mut state), Some(Action::Drop { .. })));
}
