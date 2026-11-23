use super::*;
use crate::core::ui::Level;
use crate::gui::state::tests::{row, select, selected, state};

fn press(state: &mut State, key: Key) -> bool {
    key_pressed(state, &SharedString::from(key))
}

#[test]
fn the_arrow_keys_and_escape_act_on_the_selection() {
    let mut state = state();

    assert!(press(&mut state, Key::DownArrow));
    assert_eq!(selected(&state), ["local changes"]);
    select(&mut state, "Loose", &[]);
    assert!(press(&mut state, Key::DownArrow));
    assert_eq!(selected(&state), ["feature-b"]);

    assert!(press(&mut state, Key::LeftArrow));
    assert!(press(&mut state, Key::DownArrow));
    assert_eq!(selected(&state), ["feature-a"], "feature-b is closed");

    assert!(press(&mut state, Key::UpArrow));
    assert!(press(&mut state, Key::RightArrow));
    assert!(press(&mut state, Key::DownArrow));
    assert_eq!(selected(&state), ["Second on b"], "and open again");

    assert!(press(&mut state, Key::Escape));
    assert!(selected(&state).is_empty());
}

#[test]
fn a_key_that_is_not_the_selections_is_left_to_the_window() {
    let mut state = state();

    assert!(!press(&mut state, Key::Return));
    assert!(!key_pressed(&mut state, "a"));

    assert!(selected(&state).is_empty());
}

#[test]
fn escape_ends_a_pick_by_click() {
    let mut state = state();
    select(&mut state, "Loose", &[]);
    state.prepare(Cmd::Move);

    assert!(press(&mut state, Key::Escape));

    assert!(state.drag().is_none());
    assert_eq!(state.status(), (Level::Plain, "move cancelled".to_string()));
}

#[test]
fn escape_ends_a_drag_which_holds_the_other_keys() {
    let mut state = state();
    select(&mut state, "b.rs", &[]);
    assert!(state.drag_started(row(&state, "b.rs")));

    assert!(press(&mut state, Key::DownArrow));
    assert_eq!(selected(&state), ["b.rs"]);
    assert!(press(&mut state, Key::Escape));

    assert!(state.drag().is_none());
    assert_eq!(selected(&state), ["b.rs"], "only the drag ends");
}
