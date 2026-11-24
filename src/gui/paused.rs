//! The paused view: while an operation is paused (Spec 024), the files it
//! left to resolve, the one selected, and staging it.

use crate::core::repo::{self, FileChange};
use crate::core::ui::Level;
use crate::gui::forms::Form;
use crate::gui::state::{Inspected, State};
use crate::session::diff_text;
use crate::session::paused::{self, PausedOp};

impl State {
    pub(crate) fn paused(&self) -> Option<&PausedOp> {
        self.paused.as_ref()
    }

    /// The working files the paused operation leaves, as `loom status` lists
    /// them.
    pub(crate) fn paused_files(&self) -> &[FileChange] {
        self.paused.as_ref().map_or(&[], |op| &op.files)
    }

    /// Which of [`State::paused_files`] is selected.
    pub(crate) fn paused_file(&self) -> Option<usize> {
        let path = self.paused_path.as_deref()?;
        self.paused_files()
            .iter()
            .position(|file| file.path == path)
    }

    pub(super) fn select_paused(&mut self, index: usize) {
        if let Some(file) = self.paused_files().get(index) {
            self.paused_path = Some(file.path.clone());
        }
    }

    /// The pause read, as it opens or after a command: its first file is
    /// selected, a conflict when there is one.
    pub(super) fn select_first_paused(&mut self) {
        self.paused_path = None;
        self.select_paused(0);
    }

    /// `↓` and `↑` among the files.
    pub(super) fn move_paused(&mut self, down: bool) {
        let Some(last) = self.paused_files().len().checked_sub(1) else {
            return;
        };
        let next = match (self.paused_file(), down) {
            (Some(at), true) => (at + 1).min(last),
            (Some(at), false) => at.saturating_sub(1),
            (None, true) => 0,
            (None, false) => last,
        };
        self.select_paused(next);
    }

    /// A double click on the file at `index`: it is staged.
    pub(crate) fn paused_file_activated(&mut self, index: usize) {
        if self.paused.is_some() && !self.held() {
            self.select_paused(index);
            self.stage_paused();
        }
    }

    /// `Stage`: `git add` the file selected, a deletion too, then select the
    /// next one, as the TUI's `Space` does (Spec 020).
    pub(super) fn stage_paused(&mut self) {
        let (Some(op), Some(index)) = (&self.paused, self.paused_file()) else {
            return;
        };
        let before = op.files.clone();
        let path = before[index].path.clone();
        let (workdir, git_dir) = (&self.snapshot.workdir, &self.snapshot.git_dir);
        // Under the sink, for the window's words of a refusal.
        let staged = self
            .worker
            .inline(|| paused::stage(op, workdir, git_dir, &path));
        if let Err(e) = staged {
            // A pause ended since it was shown is shown no longer.
            self.reload();
            let text = e.to_string();
            self.say(Level::Error, text.lines().next().unwrap_or_default());
            self.open_form(Form::notice(&format!("git add {path}"), &text));
            return;
        }
        self.reload();
        let next = paused::next_after_staging(&before, index, self.paused_files());
        self.select_paused(next);
        self.say(Level::Plain, format!("staged {path}"));
    }

    /// What the inspector shows while paused: the file selected against HEAD,
    /// conflict markers and all.
    pub(super) fn inspected_paused(&mut self) -> Option<Inspected<'_>> {
        let file = self.paused_files().get(self.paused_file()?)?.clone();
        // Not a row's key: the rows of a pause are a placeholder's.
        let key = format!("paused:{}", file.path);
        if !self.diffs.contains_key(&key) {
            let diff = diff_text::working_file_diff(&self.snapshot.workdir, &file);
            self.diffs.insert(key.clone(), diff);
        }
        let title = repo::cwd_relative_path(&file.path, &self.snapshot.cwd_prefix);
        let (key, diff) = self.diffs.get_key_value(&key)?;
        Some(Inspected { key, title, diff })
    }
}

#[cfg(test)]
#[path = "paused_test.rs"]
mod tests;
