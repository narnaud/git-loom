use std::collections::HashSet;

use super::*;
use crate::core::graph;
use crate::core::repo::RepoInfo;
use crate::core::shortid::IdAllocator;
use crate::core::test_helpers::{base_info, branch, commit, oid};
use crate::gui::state::tests::info;
use crate::session::tree::build_rows;
use crate::session::tree::test_support::{branch_row, commit_row, upstream_row};

/// A pane wide enough for frames between their two bounds.
const ROOM: f32 = 420.0;

fn rows_of(info: RepoInfo) -> Vec<Row> {
    rows_with_open(info, &[])
}

/// The rows of `info`, the commits `open` listing their files.
fn rows_with_open(info: RepoInfo, open: &[u8]) -> Vec<Row> {
    let ids = IdAllocator::new(info.collect_entities());
    let expanded: HashSet<String> = open.iter().map(|byte| oid(*byte).to_string()).collect();
    build_rows(&graph::build_sections(info), &ids, &expanded)
}

/// What a piece reads: a commit's message, a branch's name, a file's path;
/// the tip and the base read as what they are.
fn text(rows: &[Row], piece: &Piece) -> String {
    match (piece.kind, piece.row.map(|row| &rows[row].kind)) {
        (PieceKind::Tip, None) => "tip".to_string(),
        (PieceKind::Base, Some(RowKind::Upstream { .. })) => "base".to_string(),
        (PieceKind::Card, Some(RowKind::Commit { message, .. })) => message.clone(),
        (PieceKind::Tag, Some(RowKind::BranchName { name, .. })) => name.clone(),
        (PieceKind::Chip, Some(RowKind::CommitFile { path, .. })) => path.clone(),
        other => panic!("a piece standing for the wrong row: {:?}", other.0),
    }
}

/// The pieces, top down.
fn reading(rows: &[Row], board: &Board) -> Vec<String> {
    let mut pieces: Vec<&Piece> = board.pieces.iter().collect();
    pieces.sort_by(|a, b| a.rect.y.total_cmp(&b.rect.y));
    pieces.into_iter().map(|p| text(rows, p)).collect()
}

fn rect_of(rows: &[Row], board: &Board, wanted: &str) -> Rect {
    let found = board.pieces.iter().find(|p| text(rows, p) == wanted);
    found
        .unwrap_or_else(|| panic!("no piece reads {wanted}"))
        .rect
}

fn lines(board: &Board, wanted: fn(&LineKind) -> bool) -> Vec<&Line> {
    board.lines.iter().filter(|l| wanted(&l.kind)).collect()
}

fn frames(board: &Board) -> Vec<&Line> {
    lines(board, |kind| matches!(kind, LineKind::Frame { .. }))
}

fn center(rect: Rect) -> (f32, f32) {
    (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

/// How far down the nodes on the line running down `x` are: the trunk's, or
/// those of the thread in a frame.
fn nodes_on(board: &Board, x: f32) -> Vec<(LineKind, f32)> {
    let nodes = lines(board, |kind| {
        matches!(kind, LineKind::Bead | LineKind::Ring)
    });
    let on_it = nodes.into_iter().filter(|n| center(n.rect).0 == x);
    on_it.map(|n| (n.kind, center(n.rect).1)).collect()
}

fn trunk_x(board: &Board) -> f32 {
    center(board.lines[0].rect).0
}

/// Where the line a piece is on meets it: the middle of its own line.
fn level(rows: &[Row], board: &Board, wanted: &str) -> f32 {
    let rect = rect_of(rows, board, wanted);
    rect.y + rect.h.min(CARD_H) / 2.0
}

fn holds(frame: Rect, piece: Rect) -> bool {
    frame.x <= piece.x
        && frame.y <= piece.y
        && piece.x + piece.w <= frame.x + frame.w
        && piece.y + piece.h <= frame.y + frame.h
}

#[test]
fn the_board_reads_top_down_as_the_tree_does() {
    let rows = rows_of(info());

    let board = Board::layout(&rows, ROOM);

    assert_eq!(
        reading(&rows, &board),
        [
            "tip",
            "Loose",
            "feature-b",
            "Second on b",
            "First on b",
            "feature-a",
            "On a",
            "base",
        ]
    );
}

#[test]
fn the_trunk_runs_from_a_ring_at_the_tip_to_a_dot_at_the_base() {
    let rows = rows_of(info());

    let board = Board::layout(&rows, ROOM);

    let trunk = board.lines[0].rect;
    assert_eq!(board.lines[0].kind, LineKind::Thread);
    let (tip, base) = (level(&rows, &board, "tip"), level(&rows, &board, "base"));
    assert_eq!((trunk.y, trunk.y + trunk.h), (tip, base));
    let nodes = nodes_on(&board, trunk_x(&board));
    assert_eq!(nodes.first(), Some(&(LineKind::Ring, tip)));
    assert_eq!(nodes.last(), Some(&(LineKind::Bead, base)));
}

#[test]
fn the_two_ends_of_the_trunk_are_named_where_the_frames_start() {
    let rows = rows_of(info());

    let board = Board::layout(&rows, ROOM);

    let frame = frames(&board)[0].rect;
    for end in ["tip", "base"] {
        let named = rect_of(&rows, &board, end);
        assert_eq!((named.x, named.w), (frame.x, frame.w), "{end}");
    }
    assert!(board.height > level(&rows, &board, "base"));
}

#[test]
fn a_commit_no_branch_owns_is_a_card_with_its_dot_on_the_trunk() {
    let rows = rows_of(info());

    let board = Board::layout(&rows, ROOM);

    let nodes = nodes_on(&board, trunk_x(&board));
    assert_eq!(nodes[1], (LineKind::Bead, level(&rows, &board, "Loose")));
    let (card, frame) = (rect_of(&rows, &board, "Loose"), frames(&board)[0].rect);
    assert_eq!((card.x, card.w), (frame.x, frame.w), "as wide as a frame");
}

#[test]
fn every_branch_is_a_frame_around_its_name_and_commits() {
    let rows = rows_of(info());

    let board = Board::layout(&rows, ROOM);

    let frames = frames(&board);
    assert_eq!(frames.len(), 2, "feature-b, feature-a");
    let own = |frame: &Line| {
        LineKind::Frame {
            first: true,
            last: true,
        } == frame.kind
    };
    assert!(frames.iter().all(|f| own(f)), "neither is stacked");
    for text in ["feature-b", "Second on b", "First on b"] {
        assert!(
            holds(frames[0].rect, rect_of(&rows, &board, text)),
            "{text}"
        );
    }
    assert!(!holds(frames[0].rect, rect_of(&rows, &board, "On a")));
}

/// The merge weaving a branch in is a commit of the integration line, under
/// those no branch owns.
#[test]
fn the_merge_of_a_branch_is_a_dot_on_the_trunk_beside_its_name() {
    let rows = rows_of(info());

    let board = Board::layout(&rows, ROOM);

    let nodes = nodes_on(&board, trunk_x(&board));
    let at = |text: &str| (LineKind::Bead, level(&rows, &board, text));
    assert_eq!(nodes.len(), 5, "tip, Loose, two merges, base");
    assert_eq!(nodes[1..4], [at("Loose"), at("feature-b"), at("feature-a")]);
}

#[test]
fn the_commits_of_a_branch_are_dots_on_a_thread_from_the_ring_at_its_name() {
    let rows = rows_of(info());

    let board = Board::layout(&rows, ROOM);

    let frame = frames(&board)[0];
    let ring = board
        .lines
        .iter()
        .find(|l| l.kind == LineKind::Ring && l.color == frame.color)
        .unwrap();
    assert!(holds(frame.rect, ring.rect));
    let x = center(ring.rect).0;
    let at = |text: &str| level(&rows, &board, text);
    assert_eq!(
        nodes_on(&board, x)[..3],
        [
            (LineKind::Ring, at("feature-b")),
            (LineKind::Bead, at("Second on b")),
            (LineKind::Bead, at("First on b")),
        ]
    );
    let thread: Vec<&Line> = board
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Thread && l.color == frame.color)
        .collect();
    let (top, bottom) = (thread[0].rect, thread[thread.len() - 1].rect);
    assert!(thread.iter().all(|l| center(l.rect).0 == x));
    assert_eq!(
        (top.y, bottom.y + bottom.h),
        (at("feature-b"), at("First on b")),
        "it stops at the oldest commit"
    );
}

/// The tree colors the dots of a branch by its rank among the branches, and
/// the board its thread: both must read as one branch.
#[test]
fn a_branch_has_the_color_the_tree_gives_its_commits() {
    let rows = rows_of(info());

    let board = Board::layout(&rows, ROOM);

    let dot_color = |text: &str| {
        let row = rows.iter().find_map(|row| match &row.kind {
            RowKind::Commit {
                message, dot_color, ..
            } if message == text => Some(*dot_color),
            _ => None,
        });
        row.unwrap()
    };
    let frames = frames(&board);
    assert_eq!(frames[0].color, dot_color("First on b"));
    assert_eq!(frames[1].color, dot_color("On a"));
    assert_eq!(board.lines[0].color, dot_color("Loose"), "the trunk's own");
}

/// ```text
/// │╭─ [top]      ● T
/// ││
/// │├─ [bottom]   ● B2, ● B1
/// ├╯
/// ```
#[test]
fn the_frames_of_a_stack_are_joined_on_one_thread_and_woven_in_once() {
    let stacked = RepoInfo {
        commits: vec![
            commit(0x03, "T", Some(0x02)),
            commit(0x02, "B2", Some(0x01)),
            commit(0x01, "B1", Some(0xAA)),
        ],
        branches: vec![branch("top", 0x03, None), branch("bottom", 0x02, None)],
        ..base_info()
    };
    let rows = rows_of(stacked);

    let board = Board::layout(&rows, ROOM);

    assert_eq!(
        reading(&rows, &board),
        ["tip", "top", "T", "bottom", "B2", "B1", "base"]
    );
    let frames = frames(&board);
    let kinds: Vec<LineKind> = frames.iter().map(|f| f.kind).collect();
    assert_eq!(
        kinds,
        [
            LineKind::Frame {
                first: true,
                last: false
            },
            LineKind::Frame {
                first: false,
                last: true
            },
        ]
    );
    assert_eq!(
        frames[1].rect.y,
        frames[0].rect.y + frames[0].rect.h,
        "no gap between them"
    );
    assert_eq!((frames[0].color, frames[1].color), (Some(0), Some(1)));
    assert!(holds(frames[0].rect, rect_of(&rows, &board, "T")));
    assert!(holds(frames[1].rect, rect_of(&rows, &board, "B1")));

    let at = |text: &str| level(&rows, &board, text);
    let merges = nodes_on(&board, trunk_x(&board));
    assert_eq!(merges.len(), 3, "tip, one merge, base");
    assert_eq!(merges[1], (LineKind::Bead, at("top")));
    // Each stretch has the color of the node over it, so the upper branch
    // reaches down to the name of the one it is stacked on.
    let stretches: Vec<(f32, f32, Option<usize>)> = lines(&board, |kind| *kind == LineKind::Thread)
        [1..]
        .iter()
        .map(|l| (l.rect.y, l.rect.y + l.rect.h, l.color))
        .collect();
    assert_eq!(
        stretches,
        [
            (at("top"), at("T"), Some(0)),
            (at("T"), at("bottom"), Some(0)),
            (at("bottom"), at("B2"), Some(1)),
            (at("B2"), at("B1"), Some(1)),
        ]
    );
}

#[test]
fn branches_at_the_same_commit_share_a_frame() {
    let together = RepoInfo {
        commits: vec![commit(0x01, "Shared", Some(0xAA))],
        branches: vec![branch("one", 0x01, None), branch("two", 0x01, None)],
        ..base_info()
    };
    let rows = rows_of(together);

    let board = Board::layout(&rows, ROOM);

    let mut names = reading(&rows, &board);
    assert_eq!(names.split_off(3), ["Shared", "base"]);
    names.sort();
    assert_eq!(
        names,
        ["one", "tip", "two"],
        "both names, over their commit"
    );
    let frames = frames(&board);
    assert_eq!(frames.len(), 1);
    for text in ["one", "two", "Shared"] {
        assert!(
            holds(frames[0].rect, rect_of(&rows, &board, text)),
            "{text}"
        );
    }
}

#[test]
fn the_names_of_a_frame_start_and_end_where_its_cards_do() {
    let rows = rows_of(info());

    let board = Board::layout(&rows, ROOM);

    let (tag, card) = (
        rect_of(&rows, &board, "feature-b"),
        rect_of(&rows, &board, "First on b"),
    );
    assert_eq!((tag.x, tag.w), (card.x, card.w));
}

/// What `State` leaves of a branch closed onto its name: none of its commit
/// rows.
#[test]
fn a_closed_branch_is_a_frame_around_its_name_and_keeps_its_merge_dot() {
    let mut rows = rows_of(info());
    rows.retain(
        |r| !matches!(&r.kind, RowKind::Commit { message, .. } if message.ends_with("on b")),
    );

    let board = Board::layout(&rows, ROOM);

    let frame = frames(&board)[0].rect;
    assert_eq!(frame.h, HEAD_H + 2.0 * HEAD_PAD);
    assert!(holds(frame, rect_of(&rows, &board, "feature-b")));
    let merge = (LineKind::Bead, level(&rows, &board, "feature-b"));
    assert!(
        nodes_on(&board, trunk_x(&board)).contains(&merge),
        "still woven in"
    );
}

/// Nothing is merged for a branch still at the base.
#[test]
fn a_branch_owning_no_commit_has_no_dot_on_the_trunk() {
    let stale = RepoInfo {
        branches: vec![branch("stale", 0xAA, None)],
        ..base_info()
    };
    let rows = rows_of(stale);

    let board = Board::layout(&rows, ROOM);

    assert_eq!(reading(&rows, &board), ["tip", "stale", "base"]);
    let frame = frames(&board)[0].rect;
    assert!(holds(frame, rect_of(&rows, &board, "stale")));
    assert_eq!(
        nodes_on(&board, trunk_x(&board)).len(),
        2,
        "the tip and the base"
    );
}

#[test]
fn an_open_card_grows_to_list_its_files_and_pushes_down_what_is_under_it() {
    let closed_rows = rows_of(info());
    let closed = Board::layout(&closed_rows, ROOM);
    let rows = rows_with_open(info(), &[0xB2]);
    let open = Board::layout(&rows, ROOM);

    let chips: Vec<&Piece> = open
        .pieces
        .iter()
        .filter(|p| p.kind == PieceKind::Chip)
        .collect();

    let files: Vec<String> = chips.iter().map(|chip| text(&rows, chip)).collect();
    assert_eq!(files, ["x.rs", "y.rs"]);
    let grown = rect_of(&rows, &open, "Second on b");
    let by = 2.0 * CHIP_H + CARD_FOOT;
    assert_eq!(grown.h, CARD_H + by);
    for chip in &chips {
        assert!(chip.rect.y >= grown.y + CARD_H, "under the card's own line");
        assert!(holds(grown, chip.rect));
        assert_eq!((chip.rect.x, chip.rect.w), (grown.x, grown.w), "across it");
    }
    assert!(
        chips[0].rect.y < chips[1].rect.y,
        "in the order of the tree"
    );
    let moved = |text: &str| rect_of(&rows, &open, text).y - rect_of(&closed_rows, &closed, text).y;
    assert_eq!(moved("Loose"), 0.0, "over it");
    assert_eq!(moved("First on b"), by);
    assert_eq!(moved("feature-a"), by);
    assert_eq!(moved("base"), by);
    assert_eq!(frames(&open)[0].rect.h - frames(&closed)[0].rect.h, by);
    assert_eq!(
        level(&rows, &open, "Second on b"),
        level(&closed_rows, &closed, "Second on b"),
        "its dot stays on its own line"
    );
}

/// A file is placed after the card it is in: the window draws it over the
/// card, where it takes the pointer.
#[test]
fn the_files_of_a_card_follow_it_among_the_pieces() {
    let rows = rows_with_open(info(), &[0x01, 0xA1]);

    let board = Board::layout(&rows, ROOM);

    let order: Vec<String> = board.pieces.iter().map(|p| text(&rows, p)).collect();
    let at = |wanted: &str| order.iter().position(|text| text == wanted).unwrap();
    assert_eq!(at("w.rs"), at("Loose") + 1, "on the trunk");
    assert_eq!(at("z.rs"), at("On a") + 1, "in a frame");
}

#[test]
fn frames_take_the_room_there_is_between_what_they_need_and_what_they_read_well_at() {
    let rows = rows_of(info());
    let width = |room: f32| frames(&Board::layout(&rows, room))[0].rect.w;

    assert_eq!(width(0.0), FRAME_MIN);
    assert_eq!(width(5000.0), FRAME_MAX);
    assert_eq!(Board::layout(&rows, ROOM).width, ROOM, "it fills the pane");
}

/// The tree always puts a spacer there; the board does not count on it.
#[test]
fn a_name_right_under_a_commit_no_branch_owns_starts_a_stack_of_its_own() {
    let rows = [
        commit_row(0x01),
        branch_row("b", None),
        commit_row(0x02),
        upstream_row(),
    ];

    let board = Board::layout(&rows, ROOM);

    let kinds: Vec<PieceKind> = board.pieces.iter().map(|p| p.kind).collect();
    assert_eq!(
        kinds,
        [
            PieceKind::Tip,
            PieceKind::Card,
            PieceKind::Tag,
            PieceKind::Card,
            PieceKind::Base
        ]
    );
    let frames = frames(&board);
    assert_eq!(frames.len(), 1);
    assert!(!holds(frames[0].rect, board.pieces[1].rect), "on the trunk");
    assert!(holds(frames[0].rect, board.pieces[3].rect), "the branch's");
}

#[test]
fn an_empty_board_keeps_its_trunk_and_the_names_at_its_ends() {
    let rows = rows_of(base_info());

    let board = Board::layout(&rows, ROOM);

    assert_eq!(reading(&rows, &board), ["tip", "base"]);
    assert_eq!(board.lines.len(), 3, "the trunk and the node at each end");
    assert!(level(&rows, &board, "tip") < level(&rows, &board, "base"));
}
