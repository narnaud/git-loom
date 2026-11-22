//! The hunk picker: the hunks a command takes, read on the worker, then
//! kept or left out file by file over the window.

use crate::core::hunks::FileEntry;
use crate::core::ui::Level;
use crate::gui::state::State;
use crate::session::action::{Action, Stamp};
use crate::session::pick::{self, PickFor};
use crate::session::tree;
use crate::session::worker::{Ended, Job};

/// A pick of hunks: being read, then open.
pub(super) enum Pick {
    Reading {
        job: Job<(Vec<FileEntry>, Stamp)>,
        purpose: PickFor,
        /// The command the read is for, as the log names it.
        command: String,
    },
    Open(HunkPick),
}

/// The picker over what was read.
pub(crate) struct HunkPick {
    pub files: Vec<FileEntry>,
    /// Index into `files` of the one whose hunks show.
    pub current: usize,
    purpose: PickFor,
    /// What the pick is for, as the picker is headed.
    pub title: String,
    /// Why the last confirm was refused.
    pub error: String,
}

impl HunkPick {
    pub(crate) fn confirm(&self) -> &'static str {
        match self.purpose {
            PickFor::Commit => "Commit",
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
        let read = purpose.read(all);
        let workdir = self.snapshot.workdir.clone();
        self.clear_notice();
        let git_dir = &self.snapshot.git_dir;
        let job = (self.worker).read(command.clone(), git_dir, move || read.run(&workdir));
        self.pick = Some(Pick::Reading {
            job,
            purpose,
            command,
        });
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
        let Some(Pick::Reading { job, purpose, .. }) = self.pick.take() else {
            return false;
        };
        let read = job.join("reading the hunks");
        // What it sent last can still be queued: filed before its outcome.
        self.take_requests();
        let (files, _) = match read {
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
        let title = self.pick_title(&purpose);
        // A close refused while the hunks were read no longer holds.
        self.clear_notice();
        self.pick = Some(Pick::Open(HunkPick {
            files,
            current: 0,
            purpose,
            title,
            error: String::new(),
        }));
        self.picks_opened += 1;
        true
    }

    fn pick_title(&self, purpose: &PickFor) -> String {
        match purpose {
            PickFor::Split { commit } => {
                let sid = tree::short_id(&self.rows, &commit.to_string());
                format!("Split {sid}: keep the hunks of the first commit")
            }
            PickFor::Commit => "Commit: keep the hunks to commit".to_string(),
            PickFor::Fold { .. } => "Fold: keep the hunks to fold".to_string(),
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
        let pick = self.hunk_pick_mut()?;
        let made = match pick.purpose {
            PickFor::Split { commit } => pick::split(commit, &pick.files),
            // The window opens no pick of these.
            PickFor::Commit | PickFor::Fold { .. } => return None,
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
}

#[cfg(test)]
#[path = "hunks_test.rs"]
mod tests;
