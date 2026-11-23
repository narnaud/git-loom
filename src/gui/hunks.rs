//! The hunk picker: the hunks a command takes, read on the worker, then
//! kept or left out file by file over the window.

use crate::core::hunks::FileEntry;
use crate::core::ui::Level;
use crate::gui::state::State;
use crate::session::action::{Action, Stamp};
use crate::session::pick::{self, FoldHunks, PickFor};
use crate::session::tree::{self, RowKind};
use crate::session::worker::{Ended, Job};

/// A pick of hunks: being read, then open.
pub(super) enum Pick {
    Reading {
        job: Job<(Vec<FileEntry>, Stamp)>,
        purpose: PickFor,
        /// The command the read is for, as the log names it.
        command: String,
        keep: Keep,
    },
    Open(HunkPick),
}

/// Which hunks a pick keeps at first, as selected when it started.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Keep {
    /// Those read kept.
    AsRead,
    /// Every one read, the header being selected: a file changed since the
    /// window read the repository too.
    All,
    /// Those of the files at these paths only.
    Paths(Vec<String>),
}

/// The picker over what was read.
pub(crate) struct HunkPick {
    pub(super) files: Vec<FileEntry>,
    /// Index into `files` of the one whose hunks show.
    pub(super) current: usize,
    /// What the files were stamped with as they were read.
    stamp: Stamp,
    purpose: PickFor,
    /// What the pick is for, as the picker is headed.
    pub(super) title: String,
    /// Why the last confirm was refused, until the pick changes.
    pub(super) error: String,
}

impl HunkPick {
    pub(crate) fn confirm(&self) -> &'static str {
        match self.purpose {
            PickFor::Commit | PickFor::CommitHunks => "Commit…",
            PickFor::Fold { .. } => "Fold",
            PickFor::Split { .. } => "Split…",
        }
    }

    /// How many hunks are kept, of how many.
    pub(crate) fn kept(&self) -> (usize, usize) {
        let hunks = self.files.iter().flat_map(|file| &file.hunks);
        hunks.fold((0, 0), |(kept, all), hunk| {
            (kept + usize::from(hunk.selected), all + 1)
        })
    }
}

impl State {
    /// Read the hunks `purpose` picks from, on the worker; the picker opens
    /// once they are read.
    pub(super) fn start_pick(&mut self, purpose: PickFor) {
        if self.held() {
            return;
        }
        let all = self.snapshot.ids.get_unstaged();
        let command = purpose.command_line(all, |target| tree::short_id(&self.rows, target));
        let keep = match purpose {
            PickFor::CommitHunks | PickFor::Fold { commit: None, .. } => self.keep_selected(),
            _ => Keep::AsRead,
        };
        let read = purpose.read();
        let workdir = self.snapshot.workdir.clone();
        self.clear_notice();
        let git_dir = &self.snapshot.git_dir;
        let job = (self.worker).read(command.clone(), git_dir, move || read.run(&workdir));
        self.pick = Some(Pick::Reading {
            job,
            purpose,
            command,
            keep,
        });
    }

    /// What a pick of local changes keeps of those selected.
    fn keep_selected(&self) -> Keep {
        let rows = self.selection();
        if (rows.iter()).any(|row| matches!(row.kind, RowKind::LocalChanges { .. })) {
            return Keep::All;
        }
        let paths = rows.into_iter().filter_map(|row| match &row.kind {
            RowKind::WorkingFile { path, .. } => Some(path.clone()),
            _ => None,
        });
        Keep::Paths(paths.collect())
    }

    /// The command whose hunks are being read, if any.
    pub(crate) fn reading(&self) -> Option<&str> {
        match &self.pick {
            Some(Pick::Reading { command, .. }) => Some(command),
            _ => None,
        }
    }

    pub(crate) fn hunk_pick(&self) -> Option<&HunkPick> {
        match &self.pick {
            Some(Pick::Open(pick)) => Some(pick),
            _ => None,
        }
    }

    fn hunk_pick_mut(&mut self) -> Option<&mut HunkPick> {
        match &mut self.pick {
            Some(Pick::Open(pick)) => Some(pick),
            _ => None,
        }
    }

    /// How many pickers opened, so the window takes each to its start once.
    pub(crate) fn picks_opened(&self) -> u64 {
        self.picks_opened
    }

    /// Open the picker once the read ended, or say why not; whether
    /// anything shown changed.
    pub(super) fn advance_pick(&mut self) -> bool {
        match &self.pick {
            Some(Pick::Reading { job, .. }) if job.is_finished() => {}
            _ => return false,
        }
        let Some(Pick::Reading {
            job, purpose, keep, ..
        }) = self.pick.take()
        else {
            return false;
        };
        let read = job.join("reading the hunks");
        // What it sent last can still be queued: filed before its outcome.
        self.take_requests();
        let (mut files, stamp) = match read {
            Ok(read) => read,
            Err(e) => {
                let text = e.to_string();
                self.worker.log_line(Level::Error, &text);
                self.report(Ended::Failed(text));
                return true;
            }
        };
        if let Some((line, refusal)) = purpose.nothing_to_pick(&files) {
            self.worker.log_line(Level::Warn, line);
            self.say(Level::Warn, format!("{}: {refusal}", purpose.label()));
            return true;
        }
        match keep {
            Keep::AsRead => {}
            Keep::All => {
                for hunk in files.iter_mut().flat_map(|file| &mut file.hunks) {
                    hunk.selected = true;
                }
            }
            Keep::Paths(paths) => keep_only(&mut files, &paths),
        }
        let title = self.pick_title(&purpose);
        // A close refused while the hunks were read no longer holds.
        self.clear_notice();
        self.pick = Some(Pick::Open(HunkPick {
            files,
            current: 0,
            stamp,
            purpose,
            title,
            error: String::new(),
        }));
        self.picks_opened += 1;
        true
    }

    fn pick_title(&self, purpose: &PickFor) -> String {
        match purpose {
            PickFor::Commit | PickFor::CommitHunks => {
                "Commit: keep the hunks to commit".to_string()
            }
            PickFor::Split { commit } => {
                let sid = tree::short_id(&self.rows, &commit.to_string());
                format!("Split {sid}: keep the hunks of the first commit")
            }
            _ => "Fold: keep the hunks to fold".to_string(),
        }
    }

    /// Show the hunks of the file at `index`.
    pub(crate) fn hunk_file_picked(&mut self, index: usize) {
        if let Some(pick) = self.hunk_pick_mut().filter(|pick| index < pick.files.len()) {
            pick.current = index;
        }
    }

    /// The pick, about to change: why a confirm was refused goes with it.
    fn changing_pick(&mut self) -> Option<&mut HunkPick> {
        let pick = self.hunk_pick_mut()?;
        pick.error.clear();
        Some(pick)
    }

    /// Keep every hunk of the file at `index`, or none when all are kept.
    pub(crate) fn hunk_file_toggled(&mut self, index: usize) {
        let pick = self.changing_pick();
        if let Some(file) = pick.and_then(|pick| pick.files.get_mut(index)) {
            let keep = !file.hunks.iter().all(|hunk| hunk.selected);
            for hunk in &mut file.hunks {
                hunk.selected = keep;
            }
        }
    }

    /// Keep or leave out the hunk at `index` of the file shown.
    pub(crate) fn hunk_toggled(&mut self, index: usize) {
        let pick = self.changing_pick();
        let file = pick.and_then(|pick| pick.files.get_mut(pick.current));
        if let Some(hunk) = file.and_then(|file| file.hunks.get_mut(index)) {
            hunk.selected = !hunk.selected;
        }
    }

    /// Keep every hunk, or none.
    pub(crate) fn hunks_kept(&mut self, keep: bool) {
        if let Some(pick) = self.changing_pick() {
            for hunk in pick.files.iter_mut().flat_map(|file| &mut file.hunks) {
                hunk.selected = keep;
            }
        }
    }

    pub(crate) fn hunks_cancelled(&mut self) {
        // Not while the hunks are read: dropped, their job would run on unseen.
        if self.hunk_pick().is_some()
            && let Some(Pick::Open(pick)) = self.pick.take()
        {
            self.worker.log_line(Level::Warn, "Cancelled");
            self.say(Level::Plain, format!("{} cancelled", pick.purpose.label()));
        }
    }

    /// The pick is made: run what it is for, once a dialog asked what that
    /// needs.
    pub(crate) fn hunks_confirmed(&mut self) {
        if let Some(action) = self.confirm_pick() {
            self.start(action);
        }
    }

    /// [`State::hunks_confirmed`] short of running anything: the action to
    /// start at once, if any. A pick refused stays open, saying why. Tests
    /// go through this.
    pub(super) fn confirm_pick(&mut self) -> Option<Action> {
        // The dialog asks where it goes.
        let dest = self.draft.dest.clone();
        let pick = self.hunk_pick_mut()?;
        let made = match &pick.purpose {
            // The TUI's: the window hands a commit over by id.
            PickFor::Commit => return None,
            PickFor::CommitHunks => pick::commit(dest, String::new(), &pick.files, &pick.stamp),
            PickFor::Split { commit } => pick::split(*commit, &pick.files),
            PickFor::Fold { .. } => {
                self.aim_picked();
                return None;
            }
        };
        let action = match made {
            Ok(action) => action,
            Err((why, _)) => {
                pick.error = why.to_string();
                return None;
            }
        };
        self.pick = None;
        match self.ask(action) {
            Ok(form) => {
                self.open_form(form);
                None
            }
            Err(action) => Some(action),
        }
    }

    /// A pick to fold, made: what it keeps goes where a click says next.
    fn aim_picked(&mut self) {
        let Some(pick) = self.hunk_pick_mut() else {
            return;
        };
        if !pick::any_picked(&pick.files) {
            pick.error = "Nothing kept to fold".to_string();
            return;
        }
        let Some(Pick::Open(pick)) = self.pick.take() else {
            return;
        };
        let title = match pick.kept().0 {
            1 => "1 hunk".to_string(),
            n => format!("{n} hunks"),
        };
        let PickFor::Fold {
            sources,
            targets,
            commit,
        } = pick.purpose
        else {
            return;
        };
        let (named, covered) = pick::sources(&self.snapshot.info, commit, &pick.files);
        let from = named.into_iter().chain(covered).collect();
        let hunks = match commit {
            Some(_) => FoldHunks::Commit(pick.files),
            None => FoldHunks::Worktree {
                picked: pick.files,
                stamp: pick.stamp,
            },
        };
        self.aim_hunks(sources, targets, hunks, from, title);
    }
}

/// Keep the hunks of the files at `paths`, and only those: an untracked
/// directory is one line of the local changes, and each of its files an
/// entry of the pick.
fn keep_only(files: &mut [FileEntry], paths: &[String]) {
    for file in files {
        let under = |path: &&String| path.ends_with('/') && file.path.starts_with(path.as_str());
        let keep = paths.iter().any(|path| file.path == *path || under(&path));
        for hunk in &mut file.hunks {
            hunk.selected = keep;
        }
    }
}

#[cfg(test)]
#[path = "hunks_test.rs"]
mod tests;
