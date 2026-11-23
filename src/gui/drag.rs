//! Pieces dragged onto each other (Spec 024): what the pieces taken do on
//! the one under the pointer, and the command they make let go there.

use std::collections::HashSet;

use crate::core::ui::Level;
use crate::gui::board::{Hit, Zone};
use crate::gui::state::State;
use crate::session::Refusal;
use crate::session::action::{Action, CommitSource};
use crate::session::commands;
use crate::session::snapshot::{CommitDest, MoveSlot};
use crate::session::targets::{self, FoldEffect, FoldTarget};
use crate::session::tree::{Row, RowKind};

/// What pieces dragged do on the piece they are let go on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Effect {
    Fold(FoldTarget),
    Move(MoveSlot),
    /// Local changes become a commit there, once its message is typed.
    CommitTo(CommitDest),
}

impl Effect {
    /// What the piece under the pointer says the drop does.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Effect::Fold(target) => match target.effect {
                FoldEffect::Amend => "AMEND",
                FoldEffect::Uncommit => "UNCOMMIT",
                FoldEffect::MoveFile => "MOVE",
                FoldEffect::Noop => "",
            },
            // Over a card or under the next, the place is the same.
            Effect::Move(_) => "MOVE HERE",
            Effect::CommitTo(_) => "COMMIT HERE",
        }
    }

    /// Where on that piece it lands.
    pub(crate) fn zone(&self) -> Zone {
        match self {
            Effect::Move(MoveSlot::Above(_)) => Zone::Over,
            Effect::Move(MoveSlot::Below(_)) => Zone::Under,
            _ => Zone::On,
        }
    }
}

/// What a piece is to the drag going on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    None,
    /// It is being dragged.
    Source,
    /// The drag can land on it.
    Target,
}

/// Where the pointer is while it drags.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Spot {
    /// At (`x`, `y`) of the board.
    Board {
        x: f32,
        y: f32,
    },
    /// Over the local changes pane, which stands for them all.
    LocalChanges,
    Elsewhere,
}

/// Pieces being dragged.
pub(crate) struct Drag {
    /// Keys of the rows taken, and of the rows they can land on.
    sources: HashSet<String>,
    targets: HashSet<String>,
    /// The command arguments of the rows taken, in tree order.
    args: Vec<String>,
    /// Where a fold of them goes, and what it does there.
    folds: Vec<FoldTarget>,
    /// The one commit taken, which moves too.
    commit: Option<git2::Oid>,
    /// Whether local changes are taken, which a tag takes as a new commit.
    local: bool,
    /// What is taken, as the pointer carries it.
    pub title: String,
    /// The row under the pointer, and what the drop does there.
    pub landing: Option<(usize, Effect)>,
}

impl Drag {
    /// What the rows taken do on `row`, the pointer in `zone` of it, on it
    /// or `inside` it. A commit goes between two cards from the gap between
    /// them and the edges of both; anything folds into a card it is on.
    fn effect_at(&self, state: &State, row: &Row, zone: Zone, inside: bool) -> Option<Effect> {
        let info = &state.snapshot.info;
        let fold = (self.folds.iter())
            .find(|target| target.key == row.key && target.effect != FoldEffect::Noop)
            .filter(|_| inside)
            .cloned()
            .map(Effect::Fold);
        match (&row.kind, self.commit) {
            (RowKind::Commit { oid, .. }, Some(commit)) if row.target.is_some() => {
                let slot = match zone {
                    Zone::Over => Some(MoveSlot::Above(*oid)),
                    Zone::Under => Some(MoveSlot::Below(*oid)),
                    Zone::On => None,
                };
                let slot = slot.filter(|slot| targets::can_move(info, commit, slot));
                slot.map(Effect::Move).or(fold)
            }
            (RowKind::BranchName { name, .. }, Some(commit)) => {
                let slot = MoveSlot::Branch(name.clone());
                targets::can_move(info, commit, &slot).then_some(Effect::Move(slot))
            }
            (RowKind::BranchName { name, .. }, None) if self.local => {
                Some(Effect::CommitTo(CommitDest::Branch(name.clone())))
            }
            _ => fold,
        }
    }
}

impl State {
    /// A drag of `rows`, or why they can go nowhere.
    fn new_drag(&self, rows: &[&Row]) -> Result<Drag, Refusal> {
        let args = commands::fold_sources(rows)?;
        let commit = match rows {
            [row] => match row.kind {
                RowKind::Commit { oid, .. } => Some(oid),
                _ => None,
            },
            _ => None,
        };
        let local = rows.iter().all(|row| {
            matches!(
                row.kind,
                RowKind::WorkingFile { .. } | RowKind::LocalChanges { count: 1.. }
            )
        });
        let workdir = &self.snapshot.workdir;
        let folds = match targets::fold_targets(&self.rows, workdir, rows, false) {
            Ok(folds) => folds,
            // Still somewhere to go: beside a commit, onto a branch.
            Err(_) if commit.is_some() || local => Vec::new(),
            Err(reason) => return Err(reason),
        };
        let mut drag = Drag {
            sources: rows.iter().map(|row| row.key.clone()).collect(),
            targets: HashSet::new(),
            args,
            folds,
            commit,
            local,
            title: self.carried(rows),
            landing: None,
        };
        // Anywhere on a piece counts: a commit goes beside another.
        let lands = |row: &&Row| {
            [Zone::Over, Zone::On, Zone::Under]
                .into_iter()
                .any(|zone| drag.effect_at(self, row, zone, true).is_some())
        };
        drag.targets = (self.rows.iter().filter(lands))
            .map(|row| row.key.clone())
            .collect();
        if drag.targets.is_empty() {
            return Err("nowhere to drop it");
        }
        Ok(drag)
    }

    /// What `rows` are, in a few words.
    fn carried(&self, rows: &[&Row]) -> String {
        let [row] = rows else {
            let class = rows.first().and_then(|row| row.selection_class());
            return format!("{} {}", rows.len(), class.map_or("pieces", |c| c.label()));
        };
        self.title(row)
    }

    /// The pointer left the piece standing for the row at `index` with the
    /// button down: whether a drag starts. It takes the selection when the
    /// piece is in it, else the piece alone.
    pub(crate) fn drag_started(&mut self, index: usize) -> bool {
        self.pressed = None;
        let Some(row) = self.rows.get(index) else {
            return false;
        };
        // A command ending under it would move the pieces it holds.
        if self.held() || self.paused.is_some() {
            return false;
        }
        let rows = match self.is_selected(row) {
            true => self.selection(),
            false => vec![row],
        };
        // The base is nothing to take anywhere.
        if rows.iter().any(|row| row.selection_class().is_none()) {
            return false;
        }
        match self.new_drag(&rows) {
            Ok(drag) => {
                self.drag = Some(drag);
                true
            }
            Err(reason) => {
                self.say(Level::Warn, format!("fold: {reason}"));
                false
            }
        }
    }

    pub(crate) fn drag(&self) -> Option<&Drag> {
        self.drag.as_ref()
    }

    pub(crate) fn drag_role(&self, row: &Row) -> Role {
        match &self.drag {
            Some(drag) if drag.sources.contains(&row.key) => Role::Source,
            Some(drag) if drag.targets.contains(&row.key) => Role::Target,
            _ => Role::None,
        }
    }

    /// The pointer dragging is at `spot`.
    pub(crate) fn dragged_over(&mut self, spot: Spot) {
        let Some(drag) = &self.drag else {
            return;
        };
        let under = match spot {
            Spot::Board { x, y } => self.board().hit(x, y),
            Spot::LocalChanges => self.local_changes().map(|row| Hit {
                row,
                zone: Zone::On,
                inside: true,
            }),
            Spot::Elsewhere => None,
        };
        let landing = under.and_then(|hit| {
            let row = &self.rows[hit.row];
            let effect = drag.effect_at(self, row, hit.zone, hit.inside);
            effect.map(|effect| (hit.row, effect))
        });
        if let Some(drag) = &mut self.drag {
            drag.landing = landing;
        }
    }

    /// The drag was let go: run what it does where it is, once a dialog
    /// asked what that needs.
    pub(crate) fn drag_dropped(&mut self) {
        if let Some(action) = self.land() {
            self.start(action);
        }
    }

    /// [`State::drag_dropped`] short of running anything: the action to
    /// start at once, if any. Tests go through this.
    pub(super) fn land(&mut self) -> Option<Action> {
        let drag = self.drag.take()?;
        let (_, effect) = drag.landing?;
        match effect {
            Effect::Fold(target) => Some(Action::Fold {
                sources: drag.args,
                target: target.arg,
                hunks: None,
            }),
            Effect::Move(slot) => (drag.commit).map(|commit| Action::Move { commit, slot }),
            Effect::CommitTo(dest) => {
                let to = self.commit_dests().iter().position(|d| *d == dest);
                let action = Action::Commit {
                    source: CommitSource::Files(drag.args),
                    dest,
                    message: None,
                };
                match self.ask(action) {
                    Ok(mut form) => {
                        form.dest = to.unwrap_or(form.dest);
                        self.open_form(form);
                        None
                    }
                    Err(action) => Some(action),
                }
            }
        }
    }

    /// The drag ended with nothing run: `Esc`, or the pointer taken away.
    pub(crate) fn drag_cancelled(&mut self) {
        self.drag = None;
    }
}

#[cfg(test)]
#[path = "drag_test.rs"]
mod tests;
