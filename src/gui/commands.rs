//! What the context menu, the inspector and the bar offer: the commands of
//! the selection, each made of it by the session's catalogue
//! (`session::commands`), which the TUI's keys go through too.

use crate::core::ui::Level;
use crate::gui::state::State;
use crate::session::Refusal;
use crate::session::action::Action;
use crate::session::commands;
use crate::session::tree::{Row, RowKind};

/// A command the window offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cmd {
    Absorb,
    Drop,
    Uncommit,
    Push,
    ForcePush,
    TakeRemote,
    Unmerge,
    Update,
    Merge,
}

impl Cmd {
    /// Every command, each at the index the window names it by.
    const ALL: [Cmd; 9] = [
        Cmd::Absorb,
        Cmd::Drop,
        Cmd::Uncommit,
        Cmd::Push,
        Cmd::ForcePush,
        Cmd::TakeRemote,
        Cmd::Unmerge,
        Cmd::Update,
        Cmd::Merge,
    ];

    pub(crate) fn id(self) -> i32 {
        self as i32
    }

    pub(crate) fn from_id(id: i32) -> Option<Cmd> {
        let index = usize::try_from(id).ok()?;
        Cmd::ALL.get(index).copied()
    }

    /// The loom command it runs, which says why it refuses.
    fn name(self) -> &'static str {
        match self {
            Cmd::Absorb => "absorb",
            Cmd::Drop => "drop",
            Cmd::Uncommit => "fold",
            Cmd::Push | Cmd::ForcePush => "push",
            Cmd::TakeRemote | Cmd::Update => "update",
            Cmd::Unmerge => "unmerge",
            Cmd::Merge => "merge",
        }
    }
}

/// A command as a menu, a button or the bar shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry {
    pub cmd: Cmd,
    pub title: &'static str,
    /// Whether the selection is something it takes.
    pub enabled: bool,
}

/// The commands the menu of a piece like `row` lists, group by group: every
/// one its class takes, whatever the selection.
fn groups(row: &Row) -> &'static [&'static [(Cmd, &'static str)]] {
    match row.kind {
        RowKind::LocalChanges { .. } => &[
            &[(Cmd::Absorb, "Absorb")],
            &[(Cmd::Drop, "Drop all local changes")],
        ],
        RowKind::WorkingFile { .. } => &[&[(Cmd::Absorb, "Absorb")], &[(Cmd::Drop, "Drop")]],
        RowKind::Commit { .. } => &[&[(Cmd::Uncommit, "Uncommit")], &[(Cmd::Drop, "Drop")]],
        RowKind::CommitFile { .. } => &[&[(Cmd::Uncommit, "Uncommit")]],
        RowKind::BranchName { .. } => &[
            &[
                (Cmd::Push, "Push"),
                (Cmd::ForcePush, "Force push"),
                (Cmd::TakeRemote, "Take the remote version"),
            ],
            &[(Cmd::Unmerge, "Unmerge")],
            &[(Cmd::Drop, "Drop")],
        ],
        RowKind::Upstream { .. } => &[&[(Cmd::Update, "Update")]],
        RowKind::Context { .. } | RowKind::Spacer(_) => &[],
    }
}

/// What the bar offers, whatever is selected.
const BAR: [(Cmd, &str); 1] = [(Cmd::Merge, "Merge a branch…")];

impl State {
    /// The context menu of the selection, `None` between its groups.
    pub(crate) fn menu(&self) -> Vec<Option<Entry>> {
        let selection = self.selection();
        let Some(first) = selection.first() else {
            return Vec::new();
        };
        let mut menu = Vec::new();
        for group in groups(first) {
            if !menu.is_empty() {
                menu.push(None);
            }
            menu.extend(
                group
                    .iter()
                    .map(|&(cmd, title)| Some(self.entry(cmd, title))),
            );
        }
        menu
    }

    /// The buttons of the inspector: what the menu of the selection holds
    /// that the selection takes.
    pub(crate) fn actions(&self) -> Vec<Entry> {
        let entries = self.menu().into_iter().flatten();
        entries.filter(|entry| entry.enabled).collect()
    }

    /// The buttons of the bar.
    pub(crate) fn bar(&self) -> Vec<Entry> {
        BAR.iter()
            .map(|&(cmd, title)| self.entry(cmd, title))
            .collect()
    }

    fn entry(&self, cmd: Cmd, title: &'static str) -> Entry {
        Entry {
            cmd,
            title,
            enabled: self.make(cmd).is_ok(),
        }
    }

    /// A right click on what stands for the row at `index`: its menu is the
    /// selection's when the row is part of it, else the row is selected
    /// alone first. False when it opens no menu, as while a command runs:
    /// the reload that ends it would move the selection from under it.
    pub(crate) fn row_menu(&mut self, index: usize) -> bool {
        if self.busy() || !self.selectable().any(|row| row == index) {
            return false;
        }
        if self.is_selected(&self.rows[index]) {
            self.cursor = Some(self.rows[index].key.clone());
        } else {
            self.select_only(index);
        }
        !self.menu().is_empty()
    }

    /// Run `cmd` on the selection, or say why it takes nothing from it.
    pub(crate) fn command(&mut self, cmd: Cmd) {
        if let Some(action) = self.prepare(cmd) {
            self.start(action);
        }
    }

    /// [`State::command`] short of running anything: the action to start at
    /// once, if any. Tests go through this.
    pub(super) fn prepare(&mut self, cmd: Cmd) -> Option<Action> {
        if self.busy() {
            return None;
        }
        match self.make(cmd) {
            Ok(action) => Some(action),
            Err(reason) => {
                self.say(Level::Warn, format!("{}: {}", cmd.name(), reason));
                None
            }
        }
    }

    /// The action `cmd` makes of the selection.
    fn make(&self, cmd: Cmd) -> Result<Action, Refusal> {
        if self.paused.is_some() {
            return Err("an operation is paused");
        }
        let rows = self.selection();
        match cmd {
            Cmd::Absorb => commands::absorb(&self.snapshot.info, &rows, true),
            Cmd::Drop => commands::drop(&rows),
            Cmd::Uncommit => self.uncommit(&rows),
            Cmd::Push => commands::push(commands::one_branch(&rows, true)?, false),
            Cmd::ForcePush => commands::push(commands::one_branch(&rows, true)?, true),
            Cmd::TakeRemote => commands::remote_branch(&rows)
                .map(|branch| Action::UpdateBranch { branch })
                .ok_or("select one branch with an upstream"),
            Cmd::Unmerge => {
                commands::one_branch(&rows, true).map(|branch| Action::Unmerge { branch })
            }
            Cmd::Update => Ok(Action::Update),
            Cmd::Merge => Ok(Action::Merge),
        }
    }

    /// `loom fold <source> zz`: the one commit or commit file of `rows` back
    /// to the working tree. Each menu asks this, so not through
    /// `targets::fold_targets`, which walks the graph for its other targets.
    fn uncommit(&self, rows: &[&Row]) -> Result<Action, Refusal> {
        let sources = commands::fold_sources(rows)?;
        match (rows.first().map(|row| &row.kind), rows.len()) {
            (Some(RowKind::Commit { .. } | RowKind::CommitFile { .. }), 1) => {}
            (Some(RowKind::Commit { .. }), _) => return Err("one commit at a time"),
            (Some(RowKind::CommitFile { .. }), _) => return Err("one commit file at a time"),
            _ => return Err("select a commit or a file of one"),
        }
        let is_local = |row: &&Row| matches!(row.kind, RowKind::LocalChanges { .. });
        let local = self.rows.iter().find(is_local);
        let target = local.and_then(|row| row.target.clone());
        Ok(Action::Fold {
            sources,
            target: target.ok_or("select a commit or a file of one")?,
            hunks: None,
        })
    }
}

#[cfg(test)]
#[path = "commands_test.rs"]
mod tests;
