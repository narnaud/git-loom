//! The board as the window's models hold it.

use std::rc::Rc;

use slint::{Model, ModelRc, VecModel};

use crate::core::repo::RemoteStatus;
use crate::gui::board::{self, Board, LineKind, PieceKind};
use crate::gui::state::State;
use crate::gui::ui::{BoardLine, Piece, PieceKind as Kind, Remote, Strand};
use crate::session::tree::RowKind;

/// The pieces of the board, each with what it reads.
pub(crate) fn pieces(state: &State) -> Vec<Piece> {
    let pieces = state.board().pieces.iter();
    pieces.map(|placed| piece(placed, state)).collect()
}

fn piece(placed: &board::Piece, state: &State) -> Piece {
    let mut out = Piece {
        kind: match placed.kind {
            PieceKind::Tip => Kind::Tip,
            PieceKind::Tag => Kind::Tag,
            PieceKind::Card => Kind::Card,
            PieceKind::Base => Kind::Base,
        },
        x: placed.rect.x,
        y: placed.rect.y,
        w: placed.rect.w,
        h: placed.rect.h,
        ..Default::default()
    };
    let Some(row) = placed.row.map(|index| &state.rows()[index]) else {
        out.title = state.integration_branch().into();
        return out;
    };
    match &row.kind {
        RowKind::BranchName { name, remote, .. } => {
            out.title = name.as_str().into();
            out.remote = match remote {
                Some(RemoteStatus::Synced) => Remote::Synced,
                Some(RemoteStatus::Different) => Remote::Different,
                Some(RemoteStatus::Gone) => Remote::Gone,
                None => Remote::None,
            };
        }
        RowKind::Commit {
            message,
            file_count,
            ..
        } => {
            out.sid = row.sid.as_str().into();
            out.text = message.as_str().into();
            out.hint = counted(*file_count, "file").into();
        }
        RowKind::Upstream {
            label,
            base_short_id,
            base_message,
            commits_ahead,
        } => {
            out.title = label.as_str().into();
            out.sid = base_short_id.as_str().into();
            out.text = base_message.as_str().into();
            if *commits_ahead > 0 {
                out.hint = counted(*commits_ahead, "new commit").into();
            }
        }
        _ => {}
    }
    out
}

/// `count` of `what`, in the plural unless there is one.
fn counted(count: usize, what: &str) -> String {
    match count {
        1 => format!("1 {what}"),
        n => format!("{n} {what}s"),
    }
}

pub(crate) fn lines(board: &Board) -> Vec<BoardLine> {
    board.lines.iter().map(line).collect()
}

fn line(drawn: &board::Line) -> BoardLine {
    let (first, last) = match drawn.kind {
        LineKind::Frame { first, last } => (first, last),
        _ => (false, false),
    };
    BoardLine {
        strand: match drawn.kind {
            LineKind::Thread => Strand::Thread,
            LineKind::Bead => Strand::Bead,
            LineKind::Ring => Strand::Ring,
            LineKind::Frame { .. } => Strand::Frame,
        },
        x: drawn.rect.x,
        y: drawn.rect.y,
        w: drawn.rect.w,
        h: drawn.rect.h,
        color: drawn.color.map_or(-1, |color| color as i32),
        first,
        last,
    }
}

/// Give `model` the rows `items`: in place when their count is unchanged, so
/// the window keeps what it drew each with; `replace` installs a new model
/// otherwise.
pub(crate) fn update_model<T: Clone + PartialEq + 'static>(
    model: &ModelRc<T>,
    items: Vec<T>,
    replace: impl FnOnce(ModelRc<T>),
) {
    if model.row_count() != items.len() {
        return replace(Rc::new(VecModel::from(items)).into());
    }
    for (i, item) in items.into_iter().enumerate() {
        if model.row_data(i).as_ref() != Some(&item) {
            model.set_row_data(i, item);
        }
    }
}

#[cfg(test)]
#[path = "model_test.rs"]
mod tests;
