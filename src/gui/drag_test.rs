use super::*;
use crate::core::test_helpers::oid;
use crate::core::ui::Cancelled;
use crate::gui::board::{CARD_H, PieceKind};
use crate::gui::commands::Cmd;
use crate::gui::state::tests::{row, select, selected, state, tick_until};

/// Press the piece reading `text` and drag it away.
fn drag(state: &mut State, text: &str) -> bool {
    let index = row(state, text);
    state.row_pressed(index, false, false);
    state.drag_started(index)
}

/// Where the piece reading `text` is, the pointer `down` its own line.
fn spot(state: &State, text: &str, down: f32) -> Spot {
    let index = row(state, text);
    if matches!(state.rows[index].kind, RowKind::LocalChanges { .. }) {
        return Spot::LocalChanges;
    }
    let pieces = state.board().pieces.iter();
    let piece = pieces.clone().find(|p| p.row == Some(index));
    let piece = piece.unwrap_or_else(|| panic!("no piece reads {text}"));
    let line = match piece.kind {
        PieceKind::Card => CARD_H,
        _ => piece.rect.h,
    };
    let rect = piece.rect;
    Spot::Board {
        x: rect.x + 1.0,
        y: rect.y + down * line,
    }
}

/// What the drop would do, the pointer `down` the piece reading `text`.
fn over(state: &mut State, text: &str, down: f32) -> &'static str {
    state.dragged_over(spot(state, text, down));
    let landing = state.drag().and_then(|drag| drag.landing.as_ref());
    landing.map_or("", |(_, effect)| effect.label())
}

fn notice(state: &State) -> String {
    state.status().1
}

#[test]
fn dragged_files_amend_a_commit_and_are_committed_on_a_branch() {
    let mut state = state();
    select(&mut state, "a.rs", &["b.rs"]);
    assert!(drag(&mut state, "a.rs"));
    assert_eq!(state.drag().unwrap().title, "2 files");

    assert_eq!(over(&mut state, "Loose", 0.5), "AMEND");
    assert_eq!(over(&mut state, "feature-a", 0.5), "COMMIT HERE");
    assert_eq!(over(&mut state, "upstream", 0.5), "");
    assert_eq!(over(&mut state, "local changes", 0.5), "", "where they are");
}

/// The lines of an open card are drawn over it, tinted with it.
#[test]
fn a_drop_on_a_file_of_a_card_lands_on_the_card() {
    let mut state = state();
    state.row_toggled(row(&state, "On a"));
    assert!(drag(&mut state, "b.rs"));

    assert_eq!(over(&mut state, "z.rs", 0.5), "AMEND");
    let landing = state.drag().unwrap().landing.as_ref().map(|(row, _)| *row);
    assert_eq!(landing, Some(row(&state, "On a")));
}

#[test]
fn the_pieces_a_drag_can_land_on_are_marked_and_what_it_takes_too() {
    let mut state = state();
    assert!(drag(&mut state, "b.rs"));
    let role = |text: &str| state.drag_role(&state.rows[row(&state, text)]);

    assert_eq!(role("b.rs"), Role::Source);
    assert_eq!(role("Loose"), Role::Target);
    assert_eq!(role("feature-b"), Role::Target);
    assert_eq!(role("a.rs"), Role::None);
    assert_eq!(role("upstream"), Role::None);
    state.drag_cancelled();
    assert_eq!(
        state.drag_role(&state.rows[row(&state, "b.rs")]),
        Role::None
    );
}

#[test]
fn files_let_go_on_a_commit_are_folded_into_it() {
    let mut state = state();
    select(&mut state, "a.rs", &["b.rs"]);
    drag(&mut state, "b.rs");
    over(&mut state, "On a", 0.5);

    assert_eq!(
        state.land(),
        Some(Action::Fold {
            sources: vec!["./a.rs".to_string(), "./b.rs".to_string()],
            target: oid(0xA1).to_string(),
            hunks: None,
        }),
        "by their paths"
    );
    assert!(state.drag().is_none());
}

/// A commit needs a message: the drop only says where it goes.
#[test]
fn local_changes_let_go_on_a_branch_ask_for_the_message_of_a_commit_there() {
    let mut state = state();
    state.draft.dest = state.commit_dests()[1].clone();
    drag(&mut state, "local changes");
    over(&mut state, "feature-a", 0.5);

    assert_eq!(state.land(), None);
    let form = state.form().expect("the commit form");
    let feature_a = CommitDest::Branch("feature-a".to_string());
    assert_eq!(form.dests[form.dest], feature_a);
    let dest = form.dest;
    assert_eq!(
        state.accept("Add it", None, dest),
        Some(Action::Commit {
            source: CommitSource::Files(vec!["zz".to_string()]),
            dest: feature_a,
            message: Some("Add it".to_string()),
        })
    );
}

#[test]
fn a_dragged_commit_moves_by_the_edges_of_a_card_and_folds_in_its_middle() {
    let mut state = state();
    assert!(drag(&mut state, "Loose"));

    assert_eq!(over(&mut state, "On a", 0.05), "MOVE HERE");
    assert_eq!(over(&mut state, "On a", 0.5), "AMEND");
    assert_eq!(over(&mut state, "On a", 0.95), "MOVE HERE");
    assert_eq!(over(&mut state, "feature-a", 0.5), "MOVE HERE");
    assert_eq!(over(&mut state, "local changes", 0.5), "UNCOMMIT");
    assert_eq!(over(&mut state, "Loose", 0.5), "", "not onto itself");
}

#[test]
fn a_commit_goes_between_two_cards_from_the_gap_between_them() {
    let mut state = state();
    drag(&mut state, "Loose");
    let rect = |state: &State, text: &str| {
        let index = row(state, text);
        let pieces = state.board().pieces.iter();
        pieces.clone().find(|p| p.row == Some(index)).unwrap().rect
    };
    let commit = |state: &State, text: &str| match state.rows[row(state, text)].kind {
        RowKind::Commit { oid, .. } => oid,
        _ => unreachable!(),
    };
    let (second, first) = (rect(&state, "Second on b"), rect(&state, "First on b"));
    let gap = (second.y + second.h + first.y) / 2.0;
    let landing = |state: &mut State, y: f32| {
        let x = first.x + 1.0;
        state.dragged_over(Spot::Board { x, y });
        state
            .drag()
            .unwrap()
            .landing
            .clone()
            .map(|(_, effect)| effect)
    };
    let under_second = MoveSlot::Below(commit(&state, "Second on b"));
    let over_first = MoveSlot::Above(commit(&state, "First on b"));

    assert_eq!(
        landing(&mut state, gap - 1.0),
        Some(Effect::Move(under_second))
    );
    assert_eq!(
        landing(&mut state, gap + 1.0),
        Some(Effect::Move(over_first))
    );
    drag_cancelled_then(&mut state, "b.rs");
    assert_eq!(
        landing(&mut state, gap),
        None,
        "a file folds into a card it is on"
    );
}

/// Another button let go mid-drag takes Slint's grab: the drag's own
/// release never comes, and a later click must not land it.
#[test]
fn a_press_ends_a_drag_whose_release_was_lost() {
    let mut state = state();
    assert!(drag(&mut state, "Loose"));
    assert_eq!(over(&mut state, "On a", 0.5), "AMEND");

    state.row_pressed(row(&state, "On a"), false, false);

    assert!(state.drag().is_none());
    assert_eq!(selected(&state), ["On a"]);
}

fn drag_cancelled_then(state: &mut State, text: &str) {
    state.drag_cancelled();
    assert!(drag(state, text));
}

#[test]
fn a_commit_let_go_on_an_edge_moves_there() {
    let mut state = state();
    drag(&mut state, "Loose");
    over(&mut state, "First on b", 0.95);

    assert_eq!(
        state.land(),
        Some(Action::Move {
            commit: oid(0x01),
            slot: MoveSlot::Below(oid(0xB1)),
        })
    );
}

/// `fold --above/--below` refuses the place the commit already has.
#[test]
fn a_commit_is_not_offered_the_place_it_already_has() {
    let mut state = state();
    assert!(drag(&mut state, "Second on b"));

    assert_eq!(
        over(&mut state, "First on b", 0.05),
        "AMEND",
        "between it and its parent, it folds"
    );
    assert_eq!(over(&mut state, "First on b", 0.95), "MOVE HERE");
    assert_eq!(over(&mut state, "feature-b", 0.5), "", "the branch it ends");
    assert_eq!(over(&mut state, "feature-a", 0.5), "MOVE HERE");
}

#[test]
fn a_file_of_a_commit_goes_to_another_commit_or_back_to_the_local_changes() {
    let mut state = state();
    state.row_toggled(row(&state, "Second on b"));
    assert!(drag(&mut state, "y.rs"));

    assert_eq!(over(&mut state, "Loose", 0.5), "MOVE");
    assert_eq!(over(&mut state, "local changes", 0.5), "UNCOMMIT");
    assert_eq!(over(&mut state, "Second on b", 0.5), "", "its own commit");
    assert_eq!(over(&mut state, "feature-a", 0.5), "");
}

#[test]
fn several_commits_a_branch_or_the_base_are_not_dragged() {
    let mut state = state();

    select(&mut state, "Loose", &["On a"]);
    assert!(!drag(&mut state, "On a"));
    assert_eq!(notice(&state), "fold: one commit at a time");
    assert!(!drag(&mut state, "feature-a"));
    assert_eq!(notice(&state), "fold: a branch cannot be folded");
    assert!(!drag(&mut state, "upstream"));

    assert!(state.drag().is_none());
}

#[test]
fn a_drag_let_go_over_nothing_runs_nothing() {
    let mut state = state();
    drag(&mut state, "b.rs");
    over(&mut state, "upstream", 0.5);

    assert_eq!(state.land(), None);
    assert!(state.drag().is_none());
}

#[test]
fn a_pointer_off_the_pieces_lands_nowhere() {
    let mut state = state();
    drag(&mut state, "b.rs");
    over(&mut state, "Loose", 0.5);

    state.dragged_over(Spot::Board { x: -3.0, y: -3.0 });
    assert!(state.drag().unwrap().landing.is_none());
    over(&mut state, "Loose", 0.5);
    state.dragged_over(Spot::Elsewhere);
    assert!(state.drag().unwrap().landing.is_none());
}

/// A drag takes the whole selection, so pressing a piece of it waits.
#[test]
fn a_press_in_a_selection_selects_that_piece_alone_once_let_go_undragged() {
    let mut state = state();
    select(&mut state, "a.rs", &["b.rs"]);
    let a = row(&state, "a.rs");

    state.row_pressed(a, false, false);
    assert_eq!(selected(&state), ["a.rs", "b.rs"]);
    state.row_released(a);
    assert_eq!(selected(&state), ["a.rs"]);

    select(&mut state, "a.rs", &["b.rs"]);
    state.row_pressed(a, false, false);
    assert!(state.drag_started(a));
    state.drag_cancelled();
    state.row_released(a);
    assert_eq!(selected(&state), ["a.rs", "b.rs"]);
}

#[test]
fn nothing_is_dragged_while_a_command_runs() {
    let mut state = state();
    // Cancelled, so the end reads no repository.
    state
        .worker
        .run("loom".to_string(), || Err(Cancelled.into()));

    assert!(!drag(&mut state, "b.rs"));

    tick_until(&mut state, |state| !state.busy());
}

#[test]
fn no_menu_opens_while_dragging() {
    let mut state = state();
    assert!(drag(&mut state, "b.rs"));

    assert!(!state.row_menu(row(&state, "Loose")));
    assert_eq!(selected(&state), ["b.rs"]);
}

// ── Fold into…, Move… ──

#[test]
fn fold_into_waits_for_a_click_on_where_it_goes() {
    let mut state = state();
    select(&mut state, "b.rs", &[]);

    assert_eq!(state.prepare(Cmd::FoldInto), None);
    assert!(!state.drag().unwrap().held());
    assert_eq!(
        notice(&state),
        "Fold b.rs into… — click where it goes, Esc to cancel"
    );
    over(&mut state, "upstream", 0.5);
    assert_eq!(state.click(), None);
    assert_eq!(
        notice(&state),
        "Click where it goes, or press Esc to cancel"
    );
    assert!(state.drag().is_some(), "it waits");

    assert_eq!(over(&mut state, "Loose", 0.5), "AMEND");
    assert_eq!(
        state.click(),
        Some(Action::Fold {
            sources: vec!["./b.rs".to_string()],
            target: oid(0x01).to_string(),
            hunks: None,
        })
    );
    assert!(state.drag().is_none());
}

#[test]
fn a_pick_by_click_offers_only_what_it_was_asked_for() {
    let mut state = state();
    select(&mut state, "Loose", &[]);

    state.prepare(Cmd::FoldInto);
    assert_eq!(over(&mut state, "On a", 0.05), "AMEND");
    assert_eq!(over(&mut state, "feature-a", 0.5), "");
    state.drag_cancelled();

    state.prepare(Cmd::Move);
    assert_eq!(over(&mut state, "On a", 0.05), "MOVE HERE");
    assert_eq!(
        over(&mut state, "On a", 0.5),
        "MOVE HERE",
        "a move folds into nothing"
    );
    assert_eq!(over(&mut state, "On a", 0.95), "MOVE HERE");
    assert_eq!(over(&mut state, "feature-a", 0.5), "MOVE HERE");
    assert_eq!(over(&mut state, "local changes", 0.5), "");
}

#[test]
fn a_move_by_click_goes_over_or_under_a_card_from_anywhere_on_it() {
    let mut state = state();
    state.row_toggled(row(&state, "On a"));
    select(&mut state, "Loose", &[]);
    state.prepare(Cmd::Move);
    let landing = |state: &mut State, spot| {
        state.dragged_over(spot);
        let landing = state.drag().unwrap().landing.clone();
        landing.map(|(_, effect)| effect)
    };

    let upper = spot(&state, "On a", 0.4);
    let over = Some(Effect::Move(MoveSlot::Above(oid(0xA1))));
    assert_eq!(landing(&mut state, upper), over);
    let file = spot(&state, "z.rs", 0.5);
    let under = Some(Effect::Move(MoveSlot::Below(oid(0xA1))));
    assert_eq!(landing(&mut state, file), under, "a file is its card");
}

#[test]
fn local_changes_are_folded_from_the_menu_not_committed() {
    let mut state = state();
    select(&mut state, "b.rs", &[]);
    state.prepare(Cmd::FoldInto);

    assert_eq!(over(&mut state, "feature-a", 0.5), "");
    assert_eq!(over(&mut state, "On a", 0.5), "AMEND");
}

#[test]
fn move_takes_one_commit() {
    let mut state = state();
    select(&mut state, "Loose", &["On a"]);

    assert_eq!(state.prepare(Cmd::Move), None);

    assert!(state.drag().is_none());
    assert_eq!(notice(&state), "move: one commit at a time");
}
