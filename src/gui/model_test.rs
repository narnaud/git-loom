use super::*;
use crate::core::repo::RepoInfo;
use crate::core::test_helpers::{base_info, branch};
use crate::gui::state::tests::{info, state, state_of};

fn find<'a>(pieces: &'a [Piece], reading: &str) -> &'a Piece {
    let found = pieces
        .iter()
        .find(|p| p.title == reading || p.text == reading);
    found.unwrap_or_else(|| panic!("no piece reads {reading}"))
}

#[test]
fn a_card_reads_the_short_id_the_subject_and_the_file_count_of_its_commit() {
    let state = state();
    let pieces = pieces(&state);

    let card = find(&pieces, "Second on b");

    assert_eq!(card.kind, Kind::Card);
    let sid = state.rows().iter().find_map(|row| match &row.kind {
        RowKind::Commit { message, .. } if message == "Second on b" => Some(row.sid.clone()),
        _ => None,
    });
    assert_eq!(Some(card.sid.to_string()), sid);
    assert!(!card.sid.is_empty());
    assert_eq!(card.hint, "2 files");
    assert_eq!(find(&pieces, "First on b").hint, "1 file");
}

#[test]
fn a_tag_reads_the_name_of_its_branch_and_how_it_stands_against_its_remote() {
    let pieces = pieces(&state());

    let pushed = find(&pieces, "feature-a");
    let unpushed = find(&pieces, "feature-b");

    assert_eq!((pushed.kind, pushed.remote), (Kind::Tag, Remote::Synced));
    assert_eq!(unpushed.remote, Remote::None);
}

#[test]
fn a_tag_tells_a_branch_ahead_of_its_remote_from_one_whose_remote_is_gone() {
    let mut info: RepoInfo = base_info();
    info.branches = vec![
        branch("ahead", 0xAA, Some(RemoteStatus::Different)),
        branch("orphan", 0xAA, Some(RemoteStatus::Gone)),
    ];
    let pieces = pieces(&state_of(info));

    assert_eq!(find(&pieces, "ahead").remote, Remote::Different);
    assert_eq!(find(&pieces, "orphan").remote, Remote::Gone);
}

#[test]
fn the_tip_reads_the_integration_branch() {
    let pieces = pieces(&state());

    let tip = find(&pieces, "integration");

    assert_eq!(tip.kind, Kind::Tip);
}

#[test]
fn the_base_reads_the_upstream_and_the_commit_the_line_stands_on() {
    let pieces = pieces(&state());

    let base = find(&pieces, "origin/main");

    assert_eq!(base.kind, Kind::Base);
    assert_eq!(
        (base.sid.as_str(), base.text.as_str()),
        ("aaa0000", "Initial commit")
    );
    assert_eq!(base.hint, "", "nothing is new upstream");
}

#[test]
fn the_base_says_how_many_commits_are_new_upstream() {
    let behind = |commits_ahead| {
        let mut info: RepoInfo = base_info();
        info.upstream.commits_ahead = commits_ahead;
        pieces(&state_of(info))
    };

    assert_eq!(find(&behind(1), "origin/main").hint, "1 new commit");
    assert_eq!(find(&behind(3), "origin/main").hint, "3 new commits");
}

#[test]
fn a_piece_is_where_the_board_placed_it() {
    let state = state();

    let pieces = pieces(&state);

    let placed = &state.board().pieces;
    assert_eq!(pieces.len(), placed.len());
    for (piece, placed) in pieces.iter().zip(placed) {
        let rect = placed.rect;
        assert_eq!(
            (piece.x, piece.y, piece.w, piece.h),
            (rect.x, rect.y, rect.w, rect.h)
        );
    }
}

#[test]
fn a_line_carries_its_strand_its_color_and_where_it_is_in_its_stack() {
    let state = state_of(info());

    let lines = lines(state.board());

    let trunk = &lines[0];
    assert_eq!((trunk.strand, trunk.color), (Strand::Thread, -1));
    let frames: Vec<&BoardLine> = lines.iter().filter(|l| l.strand == Strand::Frame).collect();
    assert_eq!(frames.len(), 2);
    assert_eq!((frames[0].color, frames[1].color), (0, 1));
    assert!(
        frames.iter().all(|f| f.first && f.last),
        "neither is stacked"
    );
    let rings = lines.iter().filter(|l| l.strand == Strand::Ring).count();
    assert_eq!(rings, 3, "the tip and the two branch names");
}

#[test]
fn a_model_of_as_many_rows_is_updated_in_place() {
    let model: ModelRc<i32> = Rc::new(VecModel::from(vec![1, 2, 3])).into();
    let mut replaced = false;

    update_model(&model, vec![1, 5, 3], |_| replaced = true);

    assert!(!replaced);
    assert_eq!(model.iter().collect::<Vec<_>>(), [1, 5, 3]);
}

#[test]
fn a_model_of_another_length_is_replaced() {
    let model: ModelRc<i32> = Rc::new(VecModel::from(vec![1, 2, 3])).into();
    let mut installed = None;

    update_model(&model, vec![7, 8], |new| installed = Some(new));

    let installed = installed.expect("a new model");
    assert_eq!(installed.iter().collect::<Vec<_>>(), [7, 8]);
    assert_eq!(
        model.iter().collect::<Vec<_>>(),
        [1, 2, 3],
        "left as it was"
    );
}
