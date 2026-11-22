//! Where everything on the board is (Spec 024): the tree rows of a session
//! placed as pieces and lines, with no window.

use crate::session::tree::{Row, RowKind, STACK_JOINT};

/// A commit's line.
const CARD_H: f32 = 30.0;
/// A name's line: a branch's at the top of its frame, and those at the two
/// ends of the trunk.
const HEAD_H: f32 = 30.0;
// What a frame needs to read, and what it stops gaining from.
const FRAME_MIN: f32 = 280.0;
const FRAME_MAX: f32 = 560.0;
// Inside a frame, from its left edge: where its thread runs, and where its
// names and its cards start.
const THREAD: f32 = 16.0;
const CARD_X: f32 = 32.0;
/// Over and under the names of a frame.
const HEAD_PAD: f32 = 4.0;
/// Right of what a frame holds, and under its cards.
const FRAME_PAD: f32 = 8.0;
// Between two cards: in a frame, and on the trunk.
const GAP: f32 = 4.0;
const LOOSE_GAP: f32 = 6.0;
/// Margin around the board.
const PAD: f32 = 16.0;
// From the margin: where the trunk runs, and where frames and loose cards
// start.
const TRUNK: f32 = 6.0;
const INSET: f32 = 26.0;
/// Between two blocks.
const BLOCK_GAP: f32 = 10.0;
// Where the trunk starts, and where the first piece under its tip goes.
const TIP: f32 = 27.0;
const TOP: f32 = 50.0;
/// Under the base.
const FOOT: f32 = 16.0;
const LINE_W: f32 = 2.0;
// Across a node, the ring of bare ground that sets it off included.
const NODE: f32 = 12.0;
const BASE_NODE: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// What a piece of the board stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PieceKind {
    /// The integration branch, naming the tip of its line.
    Tip,
    /// A branch name, heading the frame of the commits it owns.
    Tag,
    /// A commit.
    Card,
    /// The upstream, naming the base the line ends at.
    Base,
}

/// Something the board names, placed on it. A name starts at the left edge
/// of its rectangle: that is how it stands over the cards under it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Piece {
    pub kind: PieceKind,
    /// Index of its row in the tree, which has none for the tip.
    pub row: Option<usize>,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineKind {
    /// A straight stretch: the trunk, or the thread of a branch.
    Thread,
    /// A commit on its line. On the trunk: one no branch owns, the merge
    /// weaving a stack in, or the base.
    Bead,
    /// Where a line starts: a branch name on its thread, the integration
    /// tip on the trunk.
    Ring,
    /// The card of one branch. In a stack only the first has its top corners
    /// and only the last its bottom ones: the rest is joined.
    Frame { first: bool, last: bool },
}

/// Something the board draws without naming it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub kind: LineKind,
    pub rect: Rect,
    /// Index of its branch among the rotating thread colors; `None` on the
    /// trunk.
    pub color: Option<usize>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Board {
    pub pieces: Vec<Piece>,
    /// In drawing order: the trunk, the frames, the threads in them, then
    /// the nodes on both.
    pub lines: Vec<Line>,
    pub width: f32,
    pub height: f32,
}

/// What the trunk has beside it, as tree rows.
enum Block {
    /// Commits no branch owns, newest first.
    Loose(Vec<usize>),
    /// The branches the tree lists as one stack, the upper one first.
    Stack(Vec<Section>),
}

/// A branch of a stack, as tree rows.
struct Section {
    color: usize,
    /// Whether the branch owns commits, which a merge then weaves in.
    owns: bool,
    /// Its names: several when branches are at the same commit.
    tags: Vec<usize>,
    /// Its commits, newest first.
    cards: Vec<usize>,
}

/// The blocks `rows` list, top down, and the row of the upstream.
fn read(rows: &[Row]) -> (Vec<Block>, Option<usize>) {
    let mut blocks: Vec<Block> = Vec::new();
    // Whether the last block still takes the rows that follow.
    let mut open = false;
    // A branch is colored by its rank, as the tree colors its dots.
    let mut colors = 0;
    let mut upstream = None;
    for (i, row) in rows.iter().enumerate() {
        let last = blocks.last_mut().filter(|_| open);
        match (&row.kind, last) {
            (RowKind::BranchName { range, .. }, last) => {
                // Names at the same commit follow each other.
                let co_located = i > 0 && matches!(rows[i - 1].kind, RowKind::BranchName { .. });
                let section = Section {
                    color: colors,
                    owns: range.is_some(),
                    tags: vec![i],
                    cards: Vec::new(),
                };
                match last {
                    Some(Block::Stack(sections)) if co_located => {
                        let shared = sections.last_mut().expect("a stack has a section");
                        shared.tags.push(i);
                        continue;
                    }
                    Some(Block::Stack(sections)) => sections.push(section),
                    _ => blocks.push(Block::Stack(vec![section])),
                }
                colors += 1;
                open = true;
            }
            (RowKind::Commit { .. }, Some(Block::Stack(sections))) => {
                let section = sections.last_mut().expect("a stack has a section");
                section.cards.push(i);
            }
            (RowKind::Commit { .. }, Some(Block::Loose(cards))) => cards.push(i),
            (RowKind::Commit { .. }, None) => {
                blocks.push(Block::Loose(vec![i]));
                open = true;
            }
            // The stack goes on with the branch this one is stacked on.
            (RowKind::Spacer(STACK_JOINT), _) => {}
            (RowKind::Upstream { .. }, _) => {
                upstream = Some(i);
                open = false;
            }
            _ => open = false,
        }
    }
    (blocks, upstream)
}

/// A stretch of the line running down `x`, from `top` to `bottom`.
fn stretch(x: f32, top: f32, bottom: f32, color: Option<usize>) -> Line {
    Line {
        kind: LineKind::Thread,
        rect: Rect {
            x: x - LINE_W / 2.0,
            y: top,
            w: LINE_W,
            h: bottom - top,
        },
        color,
    }
}

/// A node `size` across, centered on (`x`, `y`).
fn node(kind: LineKind, x: f32, y: f32, size: f32, color: Option<usize>) -> Line {
    Line {
        kind,
        rect: Rect {
            x: x - size / 2.0,
            y: y - size / 2.0,
            w: size,
            h: size,
        },
        color,
    }
}

impl Board {
    /// Lay `rows` out in a pane `room` wide.
    pub(crate) fn layout(rows: &[Row], room: f32) -> Board {
        let (blocks, upstream) = read(rows);
        let frame_w = (room - PAD * 2.0 - INSET)
            .clamp(FRAME_MIN, FRAME_MAX)
            .floor();
        let trunk = PAD + TRUNK;
        let left = PAD + INSET;
        let mut board = Board {
            width: left + frame_w + PAD,
            ..Board::default()
        };
        board.place(
            PieceKind::Tip,
            None,
            left,
            TIP - HEAD_H / 2.0,
            frame_w,
            HEAD_H,
        );
        let mut frames = Vec::new();
        let mut threads = Vec::new();
        let mut nodes = vec![node(LineKind::Ring, trunk, TIP, NODE, None)];
        let mut y = TOP;
        for block in &blocks {
            let sections = match block {
                Block::Loose(cards) => {
                    for card in cards {
                        nodes.push(node(LineKind::Bead, trunk, y + CARD_H / 2.0, NODE, None));
                        board.place(PieceKind::Card, Some(*card), left, y, frame_w, CARD_H);
                        y += CARD_H + LOOSE_GAP;
                    }
                    y += BLOCK_GAP - LOOSE_GAP;
                    continue;
                }
                Block::Stack(sections) => sections,
            };
            if sections.iter().any(|section| section.owns) {
                let level = y + HEAD_PAD + HEAD_H / 2.0;
                nodes.push(node(LineKind::Bead, trunk, level, NODE, None));
            }
            let thread = left + THREAD;
            let (x, w) = (left + CARD_X, frame_w - CARD_X - FRAME_PAD);
            // The node the thread comes down from, and the color it has
            // from there.
            let mut above: Option<(f32, usize)> = None;
            for (k, section) in sections.iter().enumerate() {
                let color = section.color;
                let top = y;
                let mut marks = Vec::new();
                y += HEAD_PAD;
                for tag in &section.tags {
                    board.place(PieceKind::Tag, Some(*tag), x, y, w, HEAD_H);
                    marks.push((LineKind::Ring, y + HEAD_H / 2.0));
                    y += HEAD_H;
                }
                y += HEAD_PAD;
                if !section.cards.is_empty() {
                    for card in &section.cards {
                        marks.push((LineKind::Bead, y + CARD_H / 2.0));
                        board.place(PieceKind::Card, Some(*card), x, y, w, CARD_H);
                        y += CARD_H + GAP;
                    }
                    y += FRAME_PAD - GAP;
                }
                frames.push(Line {
                    kind: LineKind::Frame {
                        first: k == 0,
                        last: k + 1 == sections.len(),
                    },
                    rect: Rect {
                        x: left,
                        y: top,
                        w: frame_w,
                        h: y - top,
                    },
                    color: Some(color),
                });
                for (kind, at) in marks {
                    if let Some((from, ink)) = above {
                        threads.push(stretch(thread, from, at, Some(ink)));
                    }
                    nodes.push(node(kind, thread, at, NODE, Some(color)));
                    above = Some((at, color));
                }
            }
            y += BLOCK_GAP;
        }

        let base = y + HEAD_H / 2.0;
        if upstream.is_some() {
            board.place(PieceKind::Base, upstream, left, y, frame_w, HEAD_H);
        }
        board.height = y + HEAD_H + FOOT;
        nodes.push(node(LineKind::Bead, trunk, base, BASE_NODE, None));
        board.lines.push(stretch(trunk, TIP, base, None));
        board.lines.append(&mut frames);
        board.lines.append(&mut threads);
        board.lines.append(&mut nodes);
        board
    }

    fn place(&mut self, kind: PieceKind, row: Option<usize>, x: f32, y: f32, w: f32, h: f32) {
        let rect = Rect { x, y, w, h };
        self.pieces.push(Piece { kind, row, rect });
    }
}

#[cfg(test)]
#[path = "board_test.rs"]
mod tests;
