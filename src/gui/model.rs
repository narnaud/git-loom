//! The local changes, the board and diff text as the window's models hold
//! them.

use std::rc::Rc;

use slint::{Model, ModelRc, VecModel};

use crate::core::graph::{self, FileGroup};
use crate::core::repo::{self, RemoteStatus};
use crate::gui::board::{self, Board, LineKind, PieceKind};
use crate::gui::state::State;
use crate::gui::ui::{
    BoardLine, ChangeKind, ChangeRow, LocalChanges, Piece, PieceKind as Kind, Remote, Strand,
    TextKind, TextRow,
};
use crate::session::diff_text::{self, DiffLine, expand_tabs};
use crate::session::tree::RowKind;

/// The header of the local changes pane, standing for them all.
pub(crate) fn local_changes(state: &State) -> LocalChanges {
    let header = state
        .local_changes()
        .and_then(|index| match state.rows()[index].kind {
            RowKind::LocalChanges { count } => Some((index, count)),
            _ => None,
        });
    let Some((index, count)) = header else {
        return LocalChanges {
            row: -1,
            ..Default::default()
        };
    };
    LocalChanges {
        row: index as i32,
        hint: match count {
            0 => "no changes".into(),
            count => counted(count, "file").into(),
        },
        selected: state.is_selected(&state.rows()[index]),
    }
}

/// The files of the local changes pane, each with what its line reads.
pub(crate) fn changes(state: &State) -> Vec<ChangeRow> {
    let rows = state
        .local_files()
        .map(|index| (index, &state.rows()[index]));
    let files = rows.filter_map(|(index, row)| match &row.kind {
        RowKind::WorkingFile {
            path,
            index: staged,
            worktree,
        } => {
            let mut out = ChangeRow {
                row: index as i32,
                path: repo::cwd_relative_path(path, state.cwd_prefix()).into(),
                selected: state.is_selected(row),
                ..Default::default()
            };
            match graph::file_group(*staged, *worktree) {
                FileGroup::Conflicted => out.kind = ChangeKind::Conflicted,
                FileGroup::Untracked => out.kind = ChangeKind::Untracked,
                FileGroup::Tracked => {
                    out.staged = staged.to_string().into();
                    out.unstaged = worktree.to_string().into();
                }
            }
            Some(out)
        }
        _ => None,
    });
    files.collect()
}

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
            PieceKind::Chip => Kind::Chip,
            PieceKind::Base => Kind::Base,
        },
        row: placed.row.map_or(-1, |index| index as i32),
        x: placed.rect.x,
        y: placed.rect.y,
        w: placed.rect.w,
        h: placed.rect.h,
        line: match placed.kind {
            PieceKind::Card => board::CARD_H,
            _ => placed.rect.h,
        },
        ..Default::default()
    };
    let Some(row) = placed.row.map(|index| &state.rows()[index]) else {
        out.title = state.integration_branch().into();
        return out;
    };
    out.expandable = row.expandable;
    out.expanded = row.expanded;
    out.selected = state.is_selected(row);
    match &row.kind {
        RowKind::BranchName { name, remote, .. } => {
            out.title = name.as_str().into();
            out.hint = match state.owned(row) {
                0 => "no commits".into(),
                owned => counted(owned, "commit").into(),
            };
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
        RowKind::CommitFile { path, index, .. } => {
            out.status = index.to_string().into();
            out.text = repo::cwd_relative_path(path, state.cwd_prefix()).into();
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

/// Diff text as the lines the window colors, each tab shown as `tab_width`
/// spaces.
pub(crate) fn diff_rows(text: &str, tab_width: usize) -> Vec<TextRow> {
    let lines = diff_text::classify(text);
    let rows = lines.map(|(kind, raw)| TextRow {
        text: expand_tabs(raw, tab_width).into(),
        kind: match kind {
            DiffLine::FileHeader => TextKind::FileHeader,
            DiffLine::Index => TextKind::Index,
            DiffLine::Added => TextKind::Added,
            DiffLine::Removed => TextKind::Removed,
            DiffLine::HunkHeader => TextKind::HunkHeader,
            DiffLine::Context => TextKind::Context,
            DiffLine::CommitLine => TextKind::CommitLine,
            DiffLine::CommitHeader => TextKind::CommitHeader,
        },
    });
    rows.collect()
}

/// How many characters the longest of `rows` has.
pub(crate) fn columns(rows: &[TextRow]) -> i32 {
    let longest = rows.iter().map(|row| row.text.chars().count()).max();
    i32::try_from(longest.unwrap_or(0)).unwrap_or(i32::MAX)
}

pub(crate) fn model_of<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(items)).into()
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
        return replace(model_of(items));
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
