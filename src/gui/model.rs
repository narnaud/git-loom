//! The local changes, the board and diff text as the window's models hold
//! them.

use std::rc::Rc;

use slint::{Model, ModelRc, SharedString, VecModel};

use crate::core::graph::{self, FileGroup};
use crate::core::repo::{self, RemoteStatus};
use crate::core::ui::Level;
use crate::gui::board::{self, Board, LineKind, PieceKind};
use crate::gui::commands::Entry;
use crate::gui::forms::{self, Form, Input};
use crate::gui::state::State;
use crate::gui::ui::{
    BoardLine, ChangeKind, ChangeRow, Choice, Dialog, DialogInput, LocalChanges, MenuEntry, Piece,
    PieceKind as Kind, Remote, Strand, TextKind, TextRow, Tone,
};
use crate::session::action::LogEntry;
use crate::session::diff_text::{self, DiffLine, expand_tabs};
use crate::session::snapshot::CommitDest;
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

/// The context menu, a line between its groups.
pub(crate) fn menu(menu: &[Option<Entry>]) -> Vec<MenuEntry> {
    let line = MenuEntry {
        id: -1,
        ..Default::default()
    };
    let lines = menu
        .iter()
        .map(|entry| entry.as_ref().map_or(line.clone(), button));
    lines.collect()
}

/// The entry of `menu` the keys go to from `from` (-1 for none), down or up:
/// the next that can be picked, past lines and greyed entries, else `from`.
pub(crate) fn menu_step(menu: &[MenuEntry], from: i32, down: bool) -> i32 {
    let pickable = |&i: &usize| menu[i].id >= 0 && menu[i].enabled;
    let next = match usize::try_from(from) {
        Ok(from) if down => (from + 1..menu.len()).find(pickable),
        Ok(from) => (0..from.min(menu.len())).rev().find(pickable),
        Err(_) if down => (0..menu.len()).find(pickable),
        Err(_) => (0..menu.len()).rev().find(pickable),
    };
    next.and_then(|i| i32::try_from(i).ok()).unwrap_or(from)
}

/// The buttons of the inspector, or of the bar.
pub(crate) fn buttons(entries: &[Entry]) -> Vec<MenuEntry> {
    entries.iter().map(button).collect()
}

fn button(entry: &Entry) -> MenuEntry {
    MenuEntry {
        id: entry.cmd.id(),
        title: entry.title.into(),
        enabled: entry.enabled,
    }
}

pub(crate) fn tone(level: Level) -> Tone {
    match level {
        Level::Success => Tone::Success,
        Level::Warn => Tone::Warn,
        Level::Error => Tone::Error,
        Level::Plain => Tone::Plain,
    }
}

/// A message as the window words it: the CLI highlights what `msg` puts
/// between backticks, which the window has no highlight for.
pub(crate) fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('`') else {
            break;
        };
        out.push_str(&rest[..start]);
        out.push_str(&after[..end]);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Where a commit can go, as it is picked; `integration` names a loose
/// commit's destination.
fn destinations(dests: &[CommitDest], integration: &str) -> Vec<SharedString> {
    let labels = dests
        .iter()
        .map(|dest| forms::dest_label(dest, integration));
    labels.map(SharedString::from).collect()
}

/// What `form` asks, but for what its fields start with; `integration`
/// names a loose commit's destination.
pub(crate) fn dialog(form: &Form, integration: &str) -> Dialog {
    let destinations = destinations(&form.dests, integration);
    Dialog {
        title: form.title.as_str().into(),
        body: plain(&form.body).into(),
        detail: plain(&form.detail).into(),
        error: plain(&form.error).into(),
        input: match form.input {
            Input::None => DialogInput::None,
            Input::Line => DialogInput::Line,
            Input::Text => DialogInput::Text,
        },
        placeholder: form.placeholder.as_str().into(),
        choices: model_of(choices(form)),
        multi: form.multi,
        destinations: model_of(destinations),
        confirm: form.confirm.into(),
        cancellable: form.cancellable,
    }
}

pub(crate) fn choices(form: &Form) -> Vec<Choice> {
    let choices = form.choices.iter().map(|choice| Choice {
        text: choice.text.as_str().into(),
        heading: choice.heading,
        checked: choice.checked,
    });
    choices.collect()
}

/// The log: each command's line, then what it said, a hint a line under
/// what it hints at.
pub(crate) fn log_rows(log: &[LogEntry]) -> Vec<TextRow> {
    let row = |text: String, kind| TextRow {
        text: text.into(),
        kind,
    };
    let mut rows = Vec::new();
    for entry in log {
        if !rows.is_empty() {
            rows.push(row(String::new(), TextKind::Context));
        }
        rows.push(row(format!("$ {}", entry.command), TextKind::Command));
        for (level, text) in &entry.lines {
            let (mark, kind) = match level {
                Level::Success => ("• ", TextKind::Success),
                Level::Warn => ("! ", TextKind::Warning),
                Level::Error => ("! ", TextKind::Failure),
                Level::Plain => ("", TextKind::Context),
            };
            for (n, line) in text.lines().enumerate() {
                let hint = n > 0 && !mark.is_empty();
                let (lead, kind) = if hint {
                    ("  › ", TextKind::Index)
                } else {
                    (mark, kind)
                };
                rows.push(row(format!("{lead}{}", plain(line)), kind));
            }
        }
    }
    if rows.is_empty() {
        rows.push(row("No command has run yet.".to_string(), TextKind::Index));
    }
    rows
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
