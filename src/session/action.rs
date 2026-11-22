//! The loom commands a session runs, each on a worker thread whose `msg`
//! output and prompts go to the front-end (`core::ui`).

use anyhow::Result;

use crate::core::graph;
use crate::core::hunk_select::HunkArgs;
use crate::core::hunks::FileEntry;
use crate::core::msg;
use crate::core::trace_log;
use crate::core::transaction;
use crate::core::ui::{self, Hint, Level};
use crate::git;
use crate::session::paused::PausedId;
use crate::session::snapshot::{CommitDest, MoveSlot};
use crate::{absorb, branch, commit, drop, fold, merge, push, reword, split, unmerge, update};

/// One action in a session's log: the equivalent command line and the message
/// lines it produced.
pub(crate) struct LogEntry {
    pub command: String,
    pub lines: Vec<(Level, String)>,
}

/// The one line an action that succeeded is summed up in.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Summary<'a> {
    /// It said nothing worth showing.
    Done,
    /// It paused, or a continue stopped again: its last warning says so.
    Warned(&'a str),
    /// Its last `✓` line and how many more it stands for: a multi-target
    /// command (`drop a b`) prints one per target.
    Succeeded { last: &'a str, more: usize },
}

impl LogEntry {
    pub(crate) fn summary(&self) -> Summary<'_> {
        let successes: Vec<&str> = self
            .lines
            .iter()
            .filter(|(level, _)| *level == Level::Success)
            .filter_map(|(_, text)| text.lines().next())
            .collect();
        if let [.., last] = successes.as_slice() {
            return Summary::Succeeded {
                last,
                more: successes.len() - 1,
            };
        }
        self.lines
            .iter()
            .rev()
            .find(|(level, _)| *level == Level::Warn)
            .and_then(|(_, text)| text.lines().next())
            .map_or(Summary::Done, Summary::Warned)
    }
}

/// What a commit being placed takes with it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CommitSource {
    /// Working files, passed as arguments (`c`): the index decides nothing.
    Files(Vec<String>),
    /// Whatever the hunk selector staged (`C`), committed as it stands.
    Index,
}

/// A loom command to run on the worker thread.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// `loom commit -l|-b <branch> [-m <message>] [files...]`. `c` always
    /// names files, so the index decides nothing; only `C`, which just staged
    /// what it picked, commits the index (no file arguments). Without a
    /// message the editor asks for it.
    Commit {
        source: CommitSource,
        dest: CommitDest,
        message: Option<String>,
    },
    /// `loom commit -l|-b <branch> -m <message> -p --hunks <id>...
    /// --hunks-from <fingerprint>`: the working-tree hunks picked out of
    /// every local change. What the pick leaves out stays as it was, staged
    /// or not (Spec 006).
    // The window's alone: the TUI's `C` stages its pick, then commits the
    // index.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    CommitHunks {
        dest: CommitDest,
        message: String,
        hunks: HunkArgs,
    },
    /// `loom fold [-p] <sources...> <target> [--hunks <id>... --hunks-from
    /// <fingerprint>]`: the hunks, when some, are picked out of the one
    /// source commit.
    Fold {
        sources: Vec<String>,
        target: String,
        hunks: Option<HunkArgs>,
    },
    /// `loom fold -p <files...> <target>` with the working-tree hunks already
    /// picked; `sources` are the files or `zz` the picker was opened on,
    /// `stamp` the pick's `staging::binary_stamp`.
    FoldHunks {
        sources: Vec<String>,
        picked: Vec<FileEntry>,
        stamp: Stamp,
        target: String,
    },
    /// `loom fold <commit> --above|--below <commit>`, or `loom fold <commit>
    /// <branch>`; never `MoveSlot::Stay`.
    Move { commit: git2::Oid, slot: MoveSlot },
    /// `loom split <commit> [files...]`, or `loom split -p <commit> --hunks
    /// <id>... --hunks-from <fingerprint>` with the hunks picked (the TUI's
    /// `S`, the window's `Split…`). `files`
    /// are cwd-relative, as the CLI takes them. Without a message the editor
    /// asks for the first commit's.
    Split {
        commit: String,
        files: Vec<String>,
        hunks: Option<HunkArgs>,
        message: Option<String>,
    },
    /// `loom branch <name> [--on target]`.
    Branch {
        name: String,
        target: Option<String>,
    },
    /// `loom drop <targets...>`: one commit, branch, or `zz`, or several files.
    Drop { targets: Vec<String> },
    /// `loom absorb [files...]`; no files is every local change.
    Absorb { files: Vec<String> },
    /// `loom reword <target> [-m <message>]`: a branch's new name, or a
    /// commit's new message, which the editor asks for without one.
    Reword {
        target: String,
        message: Option<String>,
    },
    /// `loom merge --all`: merge's own picker asks which branch, local ones
    /// first.
    Merge,
    /// `loom unmerge [branch]`; no branch is unmerge's own picker. Unmerge
    /// confirms a branch passed in under the TUI, and a pick that takes
    /// others with it (Spec 020).
    Unmerge { branch: Option<String> },
    /// `loom update`: takes no target.
    Update,
    /// `loom update <branch>`: takes the branch's remote version (Spec 010),
    /// picked in `u`'s menu on that branch.
    UpdateBranch { branch: String },
    /// `loom push [branch]`; no branch is push's own picker.
    Push { branch: Option<String> },
    /// `loom push <branch> -f`; push confirms it under the TUI.
    ForcePush { branch: String },
    /// `loom continue`. Refused when what is paused is no longer `paused`, the
    /// operation the view showed.
    Continue { paused: PausedId },
    /// `loom abort`, once `question` is confirmed: the CLI does not ask, but
    /// here it is one key away from absorb's. Refused when what is paused is no
    /// longer `paused`, the operation the question named.
    Abort { question: String, paused: PausedId },
}

/// `staging::binary_stamp` of a pick's entries, taken as they are read; empty
/// for a pick that stages at once or reads a commit.
pub(crate) type Stamp = Vec<Option<git2::Oid>>;

/// Run the loom command for `action`. Runs on the worker thread: the trace
/// logger is thread-local, so each action gets its own log file, `trace`
/// naming it.
pub(crate) fn execute_action(
    action: Action,
    trace: &str,
    git_dir: &std::path::Path,
    theme: &graph::Theme,
) -> Result<()> {
    // As on the CLI, resuming adds to the trace of the command that paused.
    let resuming = matches!(action, Action::Continue { .. } | Action::Abort { .. });
    trace_log::init(git_dir, trace, resuming);
    // The `cli.rs` guard, which `tui` is exempt from: an operation paused
    // since the tree was shown, or a state file gone unreadable, would
    // otherwise be saved over.
    if !resuming && let Err(e) = transaction::refuse_if_paused(git_dir) {
        trace_log::finalize();
        return Err(e);
    }
    let result = match action {
        Action::Commit {
            source,
            dest,
            message,
        } => {
            let (branch, integration) = commit_to(dest);
            let files = match source {
                CommitSource::Files(files) => files,
                CommitSource::Index => vec![],
            };
            commit::run(branch, integration, message, None, files, vec![], theme)
        }
        Action::CommitHunks {
            dest,
            message,
            hunks,
        } => {
            let (branch, integration) = commit_to(dest);
            let (message, hunks) = (Some(message), Some(hunks));
            commit::run(branch, integration, message, hunks, vec![], vec![], theme)
        }
        Action::Fold {
            sources,
            target,
            hunks,
        } => {
            let mut args = sources;
            args.push(target);
            let patch = hunks.is_some();
            let hunks = hunks.unwrap_or_default();
            fold::run(false, patch, None, hunks, args, vec![], theme)
        }
        Action::FoldHunks {
            picked,
            stamp,
            target,
            ..
        } => fold::run_picked(&picked, &stamp, &target),
        Action::Move { commit, slot } => {
            let (anchor, mut args) = match slot {
                MoveSlot::Above(x) => (Some(fold::Anchor::Above(x.to_string())), vec![]),
                MoveSlot::Below(x) => (Some(fold::Anchor::Below(x.to_string())), vec![]),
                MoveSlot::Branch(name) => (None, vec![name]),
                MoveSlot::Stay => anyhow::bail!("Nothing to move"),
            };
            args.insert(0, commit.to_string());
            fold::run(
                false,
                false,
                anchor,
                HunkArgs::default(),
                args,
                vec![],
                theme,
            )
        }
        Action::Split {
            commit,
            files,
            hunks,
            message,
        } => {
            let patch = hunks.is_some();
            let hunks = hunks.unwrap_or_default();
            split::run(commit, message, patch, hunks, files, vec![], theme)
        }
        Action::Branch { name, target } => branch::run(Some(name), target),
        Action::Drop { targets } => drop::run(targets, false),
        Action::Absorb { files } => absorb::run(false, files, vec![]),
        Action::Reword { target, message } => reword::run(target, message, vec![]),
        Action::Merge => merge::run(None, true),
        Action::Unmerge { branch } => unmerge::run(branch),
        Action::Update => update::run(None, false),
        Action::UpdateBranch { branch } => update::run(Some(branch), false),
        Action::Push { branch } => push::run(branch, false, false, None, false),
        // `push` itself confirms a force under the TUI, naming every branch.
        Action::ForcePush { branch } => push::run(Some(branch), false, true, None, false),
        // The shown view can lag the repo.
        Action::Continue { paused } if PausedId::load(git_dir).as_ref() != Some(&paused) => {
            Err(anyhow::anyhow!(
                "The paused operation changed since it was shown\n{}",
                ui::hint(Hint::Reload).unwrap_or("Look at it again")
            ))
        }
        Action::Continue { .. } => crate::resume::continue_run(),
        Action::Abort { question, paused } => match msg::confirm(&question, "") {
            // Another process may have finished or replaced it while asked.
            Ok(true) if PausedId::load(git_dir).as_ref() != Some(&paused) => Err(anyhow::anyhow!(
                "The paused operation changed while you were asked\nLook at it again before aborting"
            )),
            Ok(true) => crate::resume::abort_run(),
            Ok(false) => Err(msg::cancelled()),
            Err(e) => Err(e),
        },
    };
    trace_log::finalize();
    result
}

/// `commit::run`'s branch and loose flag for `dest`.
fn commit_to(dest: CommitDest) -> (Option<String>, bool) {
    match dest {
        CommitDest::Integration => (None, true),
        CommitDest::Branch(name) => (Some(name), false),
    }
}

/// `-m <message>` as the log shows it: the subject stands for the rest.
fn message_words(message: &str) -> [String; 2] {
    let subject = message.lines().next().unwrap_or_default();
    ["-m".into(), git::shell_quote(subject)]
}

/// `--hunks <id>...` then `--hunks-from <fingerprint>`.
fn hunk_words(hunks: &HunkArgs) -> Vec<String> {
    let mut words = Vec::new();
    for id in &hunks.ids {
        words.extend(["--hunks".into(), git::shell_quote(id)]);
    }
    if let Some(from) = &hunks.from {
        words.extend(["--hunks-from".into(), from.clone()]);
    }
    words
}

/// `-l`, or `-b <branch>` as `sid` names it.
fn dest_words(dest: &CommitDest, sid: impl Fn(&str) -> String) -> Vec<String> {
    match dest {
        CommitDest::Integration => vec!["-l".into()],
        CommitDest::Branch(name) => vec!["-b".into(), sid(name)],
    }
}

/// The CLI line equivalent to `action`, for the log; `sid` maps a command
/// argument to the short ID the tree shows for it.
pub(crate) fn command_line(action: &Action, sid: impl Fn(&str) -> String) -> String {
    let mut words = vec!["loom".to_string()];
    match action {
        Action::Commit {
            source,
            dest,
            message,
        } => {
            words.push("commit".into());
            words.extend(dest_words(dest, &sid));
            words.extend(message.iter().flat_map(|m| message_words(m)));
            // `C` staged what it picked, so this half names no files.
            if let CommitSource::Files(files) = source {
                words.extend(files.iter().map(|f| sid(f)));
            }
        }
        Action::CommitHunks {
            dest,
            message,
            hunks,
        } => {
            words.push("commit".into());
            words.extend(dest_words(dest, &sid));
            words.extend(message_words(message));
            words.push("-p".into());
            words.extend(hunk_words(hunks));
        }
        Action::Fold {
            sources,
            target,
            hunks,
        } => {
            words.push("fold".into());
            if hunks.is_some() {
                words.push("-p".into());
            }
            words.extend(sources.iter().map(|s| sid(s)));
            words.push(sid(target));
            words.extend(hunks.iter().flat_map(hunk_words));
        }
        Action::FoldHunks {
            sources, target, ..
        } => {
            words.extend(["fold".into(), "-p".into()]);
            words.extend(sources.iter().map(|s| sid(s)));
            words.push(sid(target));
        }
        Action::Move { commit, slot } => {
            words.extend(["fold".into(), sid(&commit.to_string())]);
            match slot {
                MoveSlot::Above(x) => words.extend(["--above".into(), sid(&x.to_string())]),
                MoveSlot::Below(x) => words.extend(["--below".into(), sid(&x.to_string())]),
                MoveSlot::Branch(name) => words.push(sid(name)),
                MoveSlot::Stay => {}
            }
        }
        Action::Split {
            commit,
            files,
            hunks,
            message,
        } => {
            words.push("split".into());
            if hunks.is_some() {
                words.push("-p".into());
            }
            words.push(sid(commit));
            words.extend(message.iter().flat_map(|m| message_words(m)));
            words.extend(files.iter().map(|f| git::shell_quote(f)));
            words.extend(hunks.iter().flat_map(hunk_words));
        }
        Action::Branch { name, target } => {
            words.extend(["branch".into(), name.clone()]);
            if let Some(target) = target {
                words.extend(["--on".into(), sid(target)]);
            }
        }
        Action::Drop { targets } => {
            words.push("drop".into());
            words.extend(targets.iter().map(|t| sid(t)));
        }
        Action::Absorb { files } => {
            words.push("absorb".into());
            words.extend(files.iter().map(|f| sid(f)));
        }
        Action::Reword { target, message } => {
            words.extend(["reword".into(), sid(target)]);
            words.extend(message.iter().flat_map(|m| message_words(m)));
        }
        Action::Merge => words.extend(["merge".into(), "--all".into()]),
        Action::Unmerge { branch } => {
            words.push("unmerge".into());
            words.extend(branch.iter().map(|b| sid(b)));
        }
        Action::Update => words.push("update".into()),
        Action::UpdateBranch { branch } => {
            words.extend(["update".into(), sid(branch)]);
        }
        Action::Push { branch } => {
            words.push("push".into());
            words.extend(branch.iter().map(|b| sid(b)));
        }
        Action::ForcePush { branch } => {
            words.extend(["push".into(), sid(branch), "-f".into()]);
        }
        Action::Continue { .. } => words.push("continue".into()),
        Action::Abort { .. } => words.push("abort".into()),
    }
    words.join(" ")
}

#[cfg(test)]
#[path = "action_test.rs"]
mod tests;
