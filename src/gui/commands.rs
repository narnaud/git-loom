//! What the context menu, the inspector and the bar offer: the commands of
//! the selection, each made of it by the session's catalogue
//! (`session::commands`), which the TUI's keys go through too, and what each
//! asks before it runs.

use crate::core::changeid;
use crate::core::transaction::Resumable;
use crate::core::ui::Level;
use crate::gui::drag::Way;
use crate::gui::forms::{Form, Input};
use crate::gui::state::State;
use crate::session::Refusal;
use crate::session::action::{Action, CommitSource};
use crate::session::commands::{self, Reword, Split};
use crate::session::pick::PickFor;
use crate::session::snapshot::CommitDest;
use crate::session::targets;
use crate::session::tree::{self, Row, RowKind};

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
    Commit,
    /// A commit's message, or a branch's name.
    Reword,
    Branch,
    /// The files of a commit, out into their own.
    Split,
    /// A commit, at the hunks picked.
    SplitHunks,
    /// The hunks picked of the local changes.
    CommitHunks,
    /// Where a click says next.
    FoldInto,
    /// The hunks picked, where a click says next.
    FoldHunksInto,
    /// A commit, to where a click says next.
    Move,
    /// The file of the paused operation selected, to the index.
    Stage,
    Continue,
    Abort,
}

impl Cmd {
    /// Every command, each at the index the window names it by.
    const ALL: [Cmd; 21] = [
        Cmd::Absorb,
        Cmd::Drop,
        Cmd::Uncommit,
        Cmd::Push,
        Cmd::ForcePush,
        Cmd::TakeRemote,
        Cmd::Unmerge,
        Cmd::Update,
        Cmd::Merge,
        Cmd::Commit,
        Cmd::Reword,
        Cmd::Branch,
        Cmd::Split,
        Cmd::SplitHunks,
        Cmd::CommitHunks,
        Cmd::FoldInto,
        Cmd::FoldHunksInto,
        Cmd::Move,
        Cmd::Stage,
        Cmd::Continue,
        Cmd::Abort,
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
            Cmd::Uncommit | Cmd::FoldInto | Cmd::FoldHunksInto => "fold",
            Cmd::Move => "move",
            Cmd::Stage => "add",
            Cmd::Continue => "continue",
            Cmd::Abort => "abort",
            Cmd::Push | Cmd::ForcePush => "push",
            Cmd::TakeRemote | Cmd::Update => "update",
            Cmd::Unmerge => "unmerge",
            Cmd::Merge => "merge",
            Cmd::Commit | Cmd::CommitHunks => "commit",
            Cmd::Reword => "reword",
            Cmd::Branch => "branch",
            Cmd::Split | Cmd::SplitHunks => "split",
        }
    }
}

/// What a command makes of the selection.
enum Made {
    Run(Action),
    /// The hunks to read and pick, which make the action.
    Pick(PickFor),
    /// Where the selection goes, which a click says.
    Aim(Way),
    /// The file of the paused operation selected, staged at once.
    Stage,
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
            &[
                (Cmd::Commit, "Commit…"),
                (Cmd::CommitHunks, "Commit hunks…"),
            ],
            &[
                (Cmd::FoldInto, "Fold into…"),
                (Cmd::FoldHunksInto, "Fold hunks into…"),
                (Cmd::Absorb, "Absorb"),
            ],
            &[(Cmd::Drop, "Drop all local changes")],
        ],
        RowKind::WorkingFile { .. } => &[
            &[
                (Cmd::Commit, "Commit…"),
                (Cmd::CommitHunks, "Commit hunks…"),
            ],
            &[
                (Cmd::FoldInto, "Fold into…"),
                (Cmd::FoldHunksInto, "Fold hunks into…"),
                (Cmd::Absorb, "Absorb"),
            ],
            &[(Cmd::Drop, "Drop")],
        ],
        RowKind::Commit { .. } => &[
            &[(Cmd::Reword, "Reword…"), (Cmd::SplitHunks, "Split…")],
            &[
                (Cmd::FoldInto, "Fold into…"),
                (Cmd::FoldHunksInto, "Fold hunks into…"),
                (Cmd::Move, "Move…"),
                (Cmd::Uncommit, "Uncommit"),
            ],
            &[(Cmd::Branch, "New branch here…")],
            &[(Cmd::Drop, "Drop")],
        ],
        RowKind::CommitFile { .. } => &[&[
            (Cmd::Split, "Split into its own commit…"),
            (Cmd::FoldInto, "Fold into…"),
            (Cmd::Uncommit, "Uncommit"),
        ]],
        RowKind::BranchName { .. } => &[
            &[
                (Cmd::Push, "Push"),
                (Cmd::ForcePush, "Force push"),
                (Cmd::TakeRemote, "Take the remote version…"),
            ],
            &[
                (Cmd::Reword, "Rename…"),
                (Cmd::Branch, "New branch here…"),
                (Cmd::Unmerge, "Unmerge"),
            ],
            &[(Cmd::Drop, "Drop")],
        ],
        RowKind::Upstream { .. } => &[&[(Cmd::Update, "Update")]],
        RowKind::Context { .. } | RowKind::Spacer(_) => &[],
    }
}

/// The buttons under the local changes, left to right.
const COMMIT_BUTTONS: [(Cmd, &str); 3] = [
    (Cmd::Absorb, "Absorb"),
    (Cmd::CommitHunks, "Hunks…"),
    (Cmd::Commit, "Commit…"),
];

/// The menu of a file of the paused operation.
const PAUSED: &[&[(Cmd, &str)]] = &[
    &[(Cmd::Stage, "Stage")],
    &[(Cmd::Continue, "Continue"), (Cmd::Abort, "Abort…")],
];

/// The buttons under the files of the paused operation, left to right.
const PAUSED_BUTTONS: [(Cmd, &str); 3] = [
    (Cmd::Stage, "Stage"),
    (Cmd::Abort, "Abort…"),
    (Cmd::Continue, "Continue"),
];

/// What the bar offers, whatever is selected.
const BAR: [(Cmd, &str); 2] = [
    (Cmd::Branch, "New branch…"),
    (Cmd::Merge, "Merge a branch…"),
];

impl State {
    /// The context menu of the selection, `None` between its groups.
    pub(crate) fn menu(&self) -> Vec<Option<Entry>> {
        let selection = self.selection();
        let groups = match (self.paused_file(), selection.first()) {
            (Some(_), _) => PAUSED,
            (None, Some(first)) => groups(first),
            (None, None) => return Vec::new(),
        };
        let mut menu = Vec::new();
        for group in groups {
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

    /// The buttons under the local changes: the commit buttons, which take
    /// the ones selected, or the paused operation's.
    pub(crate) fn pane_buttons(&self) -> Vec<Entry> {
        let buttons = match self.paused {
            Some(_) => &PAUSED_BUTTONS,
            None => &COMMIT_BUTTONS,
        };
        buttons
            .iter()
            .map(|&(cmd, title)| self.entry(cmd, title))
            .collect()
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
            enabled: self.enabled(cmd),
        }
    }

    /// Whether the selection takes `cmd`, as [`State::make`] says, but for a
    /// fold, which each menu asks of short of walking the graph.
    fn enabled(&self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::FoldInto if self.paused.is_none() => {
                targets::can_fold(&self.rows, &self.selection()).is_ok()
            }
            Cmd::FoldHunksInto if self.paused.is_none() => {
                PickFor::can_fold(&self.rows, &self.selection()).is_ok()
            }
            _ => self.make(cmd).is_ok(),
        }
    }

    /// A right click on what stands for the row at `index`: its menu is the
    /// selection's when the row is part of it, else the row is selected
    /// alone first. False when it opens no menu, as while a command runs:
    /// the reload that ends it would move the selection from under it.
    pub(crate) fn row_menu(&mut self, index: usize) -> bool {
        if self.held() {
            return false;
        }
        if self.paused.is_some() {
            self.select_paused(index);
            return self.paused_file().is_some();
        }
        if !self.selectable().any(|row| row == index) {
            return false;
        }
        if self.is_selected(&self.rows[index]) {
            self.cursor = Some(self.rows[index].key.clone());
        } else {
            self.select_only(index);
        }
        !self.menu().is_empty()
    }

    /// Run `cmd` on the selection, at once or once a form asked what it
    /// needs, or say why it takes nothing from it.
    pub(crate) fn command(&mut self, cmd: Cmd) {
        if let Some(action) = self.prepare(cmd) {
            self.start(action);
        }
    }

    /// [`State::command`] short of running anything: the action to start at
    /// once, if any. Tests go through this.
    pub(super) fn prepare(&mut self, cmd: Cmd) -> Option<Action> {
        if self.held() {
            return None;
        }
        match self.make(cmd) {
            Ok(Made::Run(action)) => {
                // The continue of a commit that paused gets it through.
                let of_commit = self.paused.as_ref().and_then(|op| op.command);
                let commit = matches!(action, Action::Continue { .. })
                    && of_commit == Some(Resumable::Commit);
                if commit && self.draft.paused {
                    self.draft.running = true;
                }
                match self.ask(action) {
                    Ok(form) => {
                        self.open_form(form);
                        None
                    }
                    Err(action) => Some(action),
                }
            }
            Ok(Made::Pick(purpose)) => {
                self.start_pick(purpose);
                None
            }
            Ok(Made::Aim(way)) => {
                self.aim(way);
                None
            }
            Ok(Made::Stage) => {
                self.stage_paused();
                None
            }
            Err(reason) => {
                self.say(Level::Warn, format!("{}: {}", cmd.name(), reason));
                None
            }
        }
    }

    /// What `cmd` makes of the selection: an action with no message or name
    /// yet, which the window asks for, or the hunks to pick to make one.
    fn make(&self, cmd: Cmd) -> Result<Made, Refusal> {
        if let Some(op) = &self.paused {
            return match cmd {
                Cmd::Stage if self.paused_file().is_some() => Ok(Made::Stage),
                Cmd::Stage => Err("select a file"),
                Cmd::Continue => Ok(Made::Run(commands::resume(op))),
                Cmd::Abort => Ok(Made::Run(commands::abort(op))),
                _ => Err("an operation is paused"),
            };
        }
        let rows = self.selection();
        let action = match cmd {
            Cmd::Stage | Cmd::Continue | Cmd::Abort => return Err("nothing is paused"),
            Cmd::CommitHunks => {
                // Of every local change, the ones selected kept at first.
                commands::commit_files(&rows)?;
                return PickFor::commit_hunks(&self.snapshot.info).map(Made::Pick);
            }
            Cmd::FoldInto => {
                let workdir = &self.snapshot.workdir;
                targets::fold_targets(&self.rows, workdir, &rows, false)?;
                return Ok(Made::Aim(Way::Fold));
            }
            Cmd::Move => {
                commands::move_source(&rows)?;
                return Ok(Made::Aim(Way::Move));
            }
            Cmd::FoldHunksInto => {
                let (workdir, all) = (&self.snapshot.workdir, self.snapshot.ids.get_unstaged());
                return PickFor::fold(&self.rows, workdir, &rows, all).map(Made::Pick);
            }
            Cmd::SplitHunks => {
                return match commands::split(&self.snapshot, &rows)? {
                    Split::Hunks(commit) => Ok(Made::Pick(PickFor::Split { commit })),
                    Split::Files(_) => Err("select one commit"),
                };
            }
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
            Cmd::Commit => commands::commit_files(&rows).map(|files| Action::Commit {
                source: CommitSource::Files(files),
                dest: CommitDest::Integration,
                message: None,
            }),
            Cmd::Reword => match rows.as_slice() {
                [row] => match commands::reword(row)? {
                    Reword::Commit(action) => Ok(action),
                    Reword::Branch { branch, .. } => Ok(Action::Reword {
                        target: branch,
                        message: None,
                    }),
                },
                _ => Err("one commit or branch at a time"),
            },
            // On the commit or branch selected, else at the base.
            Cmd::Branch => match rows.as_slice() {
                [row] => Ok(commands::branch_base(&self.snapshot.info, row).0),
                [] => Ok(None),
                _ => Err("one commit or branch at a time"),
            }
            .map(|target| Action::Branch {
                name: String::new(),
                target,
            }),
            Cmd::Split => match commands::split(&self.snapshot, &rows)? {
                Split::Files(action) => Ok(action),
                Split::Hunks(_) => Err("select the files to split out"),
            },
        };
        action.map(Made::Run)
    }

    /// The form that asks what `action` needs before it runs, or `action`
    /// itself when it needs nothing: the window opens no editor.
    pub(super) fn ask(&self, action: Action) -> Result<Form, Action> {
        let sid = |target: &str| tree::short_id(&self.rows, target);
        let mut form = match &action {
            Action::Commit {
                source: CommitSource::Files(files),
                ..
            } => {
                let what = match files.as_slice() {
                    [zz] if *zz == self.snapshot.ids.get_unstaged() => {
                        "every local change".to_string()
                    }
                    [_] => "1 file".to_string(),
                    files => format!("{} files", files.len()),
                };
                self.commit_form(what)
            }
            Action::CommitHunks { hunks, .. } => {
                let what = match hunks.ids.len() {
                    1 => "1 hunk".to_string(),
                    n => format!("{n} hunks"),
                };
                self.commit_form(what)
            }
            Action::Reword { target, .. } => match self.selection().as_slice() {
                [row] if matches!(row.kind, RowKind::BranchName { .. }) => {
                    let mut form =
                        Form::complete(format!("Rename {target}"), Input::Line, "Rename");
                    form.initial = target.clone();
                    form
                }
                _ => {
                    let body = format!("Reword {}", sid(target));
                    let mut form = Form::complete(body, Input::Text, "Reword");
                    form.initial = self.message_of(target);
                    form
                }
            },
            Action::Branch { target, .. } => {
                // A branch is named, as Rename names it; a commit by its ID.
                let selection = self.selection();
                let on_branch = matches!(
                    selection.as_slice(),
                    [row] if matches!(row.kind, RowKind::BranchName { .. })
                );
                let on = match target {
                    Some(name) if on_branch => format!("on {name}"),
                    Some(target) => format!("on {}", sid(target)),
                    None => "at the base".to_string(),
                };
                let mut form = Form::complete(format!("New branch {on}"), Input::Line, "Create");
                form.placeholder = "Name".to_string();
                form
            }
            Action::Split { commit, hunks, .. } => {
                let body = format!("Split {}", sid(commit));
                let mut form = Form::complete(body, Input::Text, "Split");
                let taken = match hunks {
                    Some(_) => "The hunks kept",
                    None => "The files selected",
                };
                form.detail = format!(
                    "{taken} make the first commit, under this message; the rest of the commit \
                     stays in the second, under its own."
                );
                form.placeholder = "Message of the first commit".to_string();
                form
            }
            Action::UpdateBranch { branch } => {
                let body = format!("Take the remote version of {branch}?");
                let mut form = Form::complete(body, Input::None, "Take it");
                form.detail =
                    "Its commits are replaced by its remote's: what only the local branch \
                               has goes, and the report names its old tip."
                        .to_string();
                form
            }
            _ => return Err(action),
        };
        form.complete_with(action);
        Ok(form)
    }

    /// The dialog of a commit of `what`: its message, from the last that did
    /// not go through, and where it goes, from where the last went.
    fn commit_form(&self, what: String) -> Form {
        let mut form = Form::complete(format!("Commit {what}"), Input::Text, "Commit");
        form.placeholder = "Message".to_string();
        form.initial = self.draft.message.clone();
        form.dests = self.commit_dests();
        form.dest = self.commit_dest().unwrap_or(0);
        form
    }

    /// The message of the commit `target` names, to start a reword from;
    /// empty when it cannot be read. Its `Change-Id` is left out: the reword
    /// keeps it (Spec 003), and one typed over would be another commit's.
    fn message_of(&self, target: &str) -> String {
        let repo = git2::Repository::open(&self.snapshot.workdir);
        let commit = repo.and_then(|repo| {
            let oid = git2::Oid::from_str(target)?;
            let message = repo.find_commit(oid)?.message().map(str::to_string);
            Ok(message.unwrap_or_default())
        });
        let message = commit.unwrap_or_default();
        changeid::strip(&message).trim_end().to_string()
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
