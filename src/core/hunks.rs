//! The picker model: files and their hunks, each with a selection and an origin.

use crate::core::diff::DiffHunk;

/// Where a hunk came from — determines how to apply/reverse on confirm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HunkOrigin {
    /// From `git diff --cached` (already staged).
    Staged,
    /// From `git diff` (unstaged working-tree change).
    Unstaged,
    /// From a commit diff (`git diff <oid>^..<oid>`).
    Commit,
}

/// A single hunk with a toggle state and origin.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HunkEntry {
    pub hunk: DiffHunk,
    pub selected: bool,
    pub origin: HunkOrigin,
}

/// A file and its parsed hunks, with git status information.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FileEntry {
    pub path: String,
    pub hunks: Vec<HunkEntry>,
    /// Index (staged) status character: ' ', 'A', 'M', 'D', 'R', or '?'.
    pub index_status: char,
    /// Worktree (unstaged) status character: ' ', 'M', 'D', 'R', '?', or '!'.
    pub worktree_status: char,
    /// Whether this file is binary (no hunk-level patching possible).
    pub binary: bool,
}

impl FileEntry {
    /// Whether the file is not in the index at all.
    pub(crate) fn is_untracked(&self) -> bool {
        self.index_status == '?' && self.worktree_status == '?'
    }

    /// Compute the effective status characters based on current hunk selections.
    ///
    /// Returns `(index_char, worktree_char)` reflecting what `git status` would
    /// show if the current selections were applied.
    pub(crate) fn effective_status(&self) -> (char, char) {
        let will_have_staged = self.hunks.iter().any(|h| h.selected);
        let will_have_unstaged = self.hunks.iter().any(|h| !h.selected);

        if self.is_untracked() {
            return if will_have_staged {
                ('A', ' ')
            } else {
                ('?', '?')
            };
        }

        // Staged new file fully deselected → back to untracked.
        if self.index_status == 'A' && !will_have_staged {
            return ('?', '?');
        }

        let eff_index = if will_have_staged {
            match self.index_status {
                'A' | 'M' | 'D' | 'R' => self.index_status,
                _ => match self.worktree_status {
                    'D' => 'D',
                    _ => 'M',
                },
            }
        } else {
            ' '
        };

        let eff_worktree = if will_have_unstaged {
            match self.worktree_status {
                'M' | 'D' => self.worktree_status,
                _ => match self.index_status {
                    'D' => 'D',
                    _ => 'M',
                },
            }
        } else {
            ' '
        };

        (eff_index, eff_worktree)
    }
}

#[cfg(test)]
#[path = "hunks_test.rs"]
mod tests;
