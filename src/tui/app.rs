//! Interactive status TUI (`loom tui`): the status tree on the left, the diff
//! of the item under the cursor on the right.
//!
//! Actions (commit, fold, move, split, branch, drop, absorb, reword) run the regular loom
//! command on a worker thread while the TUI stays up: the command's prompts
//! become popups and its messages a log (`core::ui`), and only an editor takes
//! the terminal over. Fold, move and commit pick their target in a second step
//! inside the tree; `C`, `F` and `S` pick their hunks first, in the hunk selector, which
//! runs before any action, as a nested shell on the same terminal.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::ListItem,
};

use crate::core::graph::{self, Section};
use crate::core::hunk_select::{self, HunkArgs};
use crate::core::repo::{self, BranchInfo, CommitInfo, FileChange, RemoteStatus, RepoInfo};
use crate::core::shortid::IdAllocator;
use crate::core::staging;
use crate::core::transaction;
use crate::core::ui::{self, Answer, Cancelled, Level, Request};
use crate::git;
use crate::tui::hunk_selector::{FileEntry, run_hunk_selector_nested};
use crate::tui::shell::{KeyResult, PaneId, Shell, ShellApp, ShellConfig, Tick};
use crate::tui::status_tree::{
    self, LOCAL_CHANGES_KEY, PENDING_COMMIT_OID, Row, RowKind, RowMark, SelectionClass, branch_key,
    commit_file_key, working_file_key,
};
use crate::tui::theme::TuiTheme;
use crate::tui::widgets::common::{colorize_diff, pane_block};
use crate::tui::widgets::diff_pane::DiffPane;
use crate::tui::widgets::list_pane::ListPane;
use crate::tui::widgets::popup::{self, LogEntry, Notice, Prompt, PromptOutcome, TextField};
use crate::{absorb, branch, commit, drop, fold, reword, split};

// ── Data model ───────────────────────────────────────────────────────────

/// Everything gathered from the repo for one TUI round.
struct Snapshot {
    workdir: PathBuf,
    git_dir: PathBuf,
    cwd_prefix: String,
    /// Kept as gathered (hidden branches removed) so the graph can be rebuilt
    /// with a branch that does not exist yet.
    info: RepoInfo,
    ids: IdAllocator,
}

/// Placeholder name of a branch being created. Git rejects a ref component
/// starting with a dot, so it clashes with no real branch; keeping it
/// non-empty keeps it distinct from "no branch here" sentinels in `graph`.
const NEW_BRANCH_NAME: &str = ".new";

/// Where `c` puts its commit: loose on the integration line (`-i`) or at the
/// tip of a woven branch (`-b`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum CommitDest {
    Integration,
    Branch(String),
}

/// A row that exists only on screen, or one drawn where it is not: a branch
/// being named, a commit being placed, or a commit being moved.
enum Preview {
    Branch(BranchInfo),
    Commit { dest: CommitDest, file_count: usize },
    Move { oid: git2::Oid, slot: MoveSlot },
}

/// Where `m` puts its commit, as `loom fold` spells it (Spec 007).
#[derive(Debug, Clone, PartialEq, Eq)]
enum MoveSlot {
    /// Where it already is: the walk starts here, and nothing runs.
    Stay,
    /// `--above <commit>`.
    Above(git2::Oid),
    /// `--below <commit>`.
    Below(git2::Oid),
    /// `fold <commit> <branch>`: the branch's tip, advancing only that name.
    /// Never shown as a branch pick: it is the one way to reach an empty
    /// branch, or to split a co-located group.
    Branch(String),
}

impl Snapshot {
    /// The graph sections with `preview` faked in — a branch at its tip, or a
    /// commit at its destination — so the tree shows where it will land:
    /// split ownership, co-located names, stacking and all.
    ///
    /// A commit being placed or moved keeps every branch where the real tree
    /// draws it, so one it fills or empties does not jump between the empty
    /// and the owning groups under `↑`/`↓`; the tree regroups once it runs.
    fn sections(&self, preview: Option<Preview>) -> Vec<Section> {
        let mut info = self.info.clone();
        let pinned = match preview {
            Some(Preview::Branch(pending)) => {
                info.branches.push(pending);
                false
            }
            Some(Preview::Commit { dest, file_count }) => {
                place_pending_commit(&mut info, &dest, file_count);
                true
            }
            Some(Preview::Move { oid, slot }) => {
                place_moved_commit(&mut info, oid, &slot);
                true
            }
            None => false,
        };
        let sections = graph::build_sections(info);
        if pinned {
            keep_branch_places(&graph::build_sections(self.info.clone()), sections)
        } else {
            sections
        }
    }

    /// Whether `files` (short IDs) is the `zz` that stands for every change.
    fn is_all_changes(&self, files: &[String]) -> bool {
        files.iter().any(|f| f == self.ids.get_unstaged())
    }

    /// How many files the index holds a change for — what a `C` commit takes.
    fn staged_count(&self) -> usize {
        self.info
            .working_changes
            .iter()
            .filter(|c| is_staged(c))
            .count()
    }
}

/// Reorder the branch sections of `preview` into the order `base` draws them
/// in. Each is ranked by the first of its names `base` places, one it lacks
/// by the section before it; the sort is stable, so a group or stack a
/// preview splits keeps its own order. An empty group is split by place.
fn keep_branch_places(base: &[Section], mut preview: Vec<Section>) -> Vec<Section> {
    let place: HashMap<&str, usize> = base
        .iter()
        .enumerate()
        .flat_map(|(i, s)| match s {
            Section::Branch { names, .. } => names.iter().map(|(n, _)| (n.as_str(), i)).collect(),
            _ => Vec::new(),
        })
        .collect();
    let is_branch = |s: &Section| matches!(s, Section::Branch { .. });
    let Some(start) = preview.iter().position(is_branch) else {
        return preview;
    };
    let end = start + preview[start..].iter().take_while(|s| is_branch(s)).count();
    let taken: Vec<Section> = preview.drain(start..end).collect();
    let rank_of = |name: &str| place.get(name).copied();
    let mut rank = 0;
    let mut run: Vec<(usize, Section)> = Vec::new();
    for section in taken {
        let Section::Branch { names, commits } = section else {
            unreachable!("a run of branch sections");
        };
        if !commits.is_empty() {
            rank = names
                .iter()
                .filter_map(|(n, _)| rank_of(n))
                .min()
                .unwrap_or(rank);
            run.push((rank, Section::Branch { names, commits }));
            continue;
        }
        // A branch the preview empties is co-located with those at the base,
        // but keeps its own place: nothing is drawn under the group anyway.
        for (name, remote) in names {
            rank = rank_of(&name).unwrap_or(rank);
            match run.last_mut() {
                Some((r, Section::Branch { names, commits }))
                    if *r == rank && commits.is_empty() =>
                {
                    names.push((name, remote))
                }
                _ => run.push((
                    rank,
                    Section::Branch {
                        names: vec![(name, remote)],
                        commits: Vec::new(),
                    },
                )),
            }
        }
    }
    run.sort_by_key(|(rank, _)| *rank);
    preview.splice(start..start, run.into_iter().map(|(_, s)| s));
    preview
}

/// Insert the pending commit into `info` where `loom commit` would put it:
/// newest on the integration line, or at the branch's tip with the branch
/// advanced onto it and whatever was stacked on that tip re-parented, so the
/// ownership walk in `graph` draws the stack as the relocation will leave it.
fn place_pending_commit(info: &mut RepoInfo, dest: &CommitDest, file_count: usize) {
    let oid = PENDING_COMMIT_OID;
    let mut pending = CommitInfo {
        oid,
        short_id: String::new(),
        message: format!(
            "new commit ({} file{})",
            file_count,
            if file_count == 1 { "" } else { "s" }
        ),
        parent_oid: None,
        change_id: None,
        files: Vec::new(),
    };
    match dest {
        CommitDest::Integration => {
            pending.parent_oid = Some(
                info.commits
                    .first()
                    .map_or(info.upstream.merge_base_oid, |c| c.oid),
            );
            info.commits.insert(0, pending);
        }
        // Every destination was read off a drawn branch row of this same
        // snapshot, so the branch is there.
        CommitDest::Branch(name) => push_onto_branch(info, pending, name),
    }
}

/// Put `commit` on top of branch `name`, advancing that name alone, with
/// whatever was stacked on the old tip re-parented onto it. A no-op when the
/// branch is not there.
fn push_onto_branch(info: &mut RepoInfo, mut commit: CommitInfo, name: &str) {
    let Some(branch) = info.branches.iter_mut().find(|b| b.name == name) else {
        return;
    };
    let oid = commit.oid;
    let tip = branch.tip_oid;
    branch.tip_oid = oid;
    commit.parent_oid = Some(tip);
    // A tip outside the range is the base: the branches forking from it stay
    // parallel, only a stack on an in-range tip follows.
    let at = info.commits.iter().position(|c| c.oid == tip);
    if at.is_some() {
        reparent_children(info, tip, oid);
    }
    info.commits
        .insert(at.unwrap_or(info.commits.len()), commit);
}

fn reparent_children(info: &mut RepoInfo, from: git2::Oid, to: git2::Oid) {
    for commit in &mut info.commits {
        if commit.parent_oid == Some(from) {
            commit.parent_oid = Some(to);
        }
    }
}

/// Move commit `oid` of `info` to `slot`, as `loom fold` would (Spec 007):
/// taken out, its children and the branches ending at it fall back onto its
/// parent; `--above` hands it the target's branches, `--below` leaves them.
fn place_moved_commit(info: &mut RepoInfo, oid: git2::Oid, slot: &MoveSlot) {
    let anchor_known = |x: &git2::Oid| *x != oid && info.commits.iter().any(|c| c.oid == *x);
    let known = match slot {
        MoveSlot::Stay => false,
        MoveSlot::Above(x) | MoveSlot::Below(x) => anchor_known(x),
        MoveSlot::Branch(name) => info.branches.iter().any(|b| b.name == *name),
    };
    let Some(from) = info
        .commits
        .iter()
        .position(|c| c.oid == oid)
        .filter(|_| known)
    else {
        return;
    };
    let mut moved = info.commits.remove(from);
    if let Some(parent) = moved.parent_oid {
        reparent_children(info, oid, parent);
        for branch in &mut info.branches {
            if branch.tip_oid == oid {
                branch.tip_oid = parent;
            }
        }
    }
    let position = |info: &RepoInfo, x: git2::Oid| {
        info.commits
            .iter()
            .position(|c| c.oid == x)
            .expect("checked above")
    };
    match slot {
        MoveSlot::Stay => {}
        MoveSlot::Above(x) => {
            let at = position(info, *x);
            reparent_children(info, *x, oid);
            for branch in &mut info.branches {
                if branch.tip_oid == *x {
                    branch.tip_oid = oid;
                }
            }
            moved.parent_oid = Some(*x);
            info.commits.insert(at, moved);
        }
        MoveSlot::Below(x) => {
            let at = position(info, *x);
            moved.parent_oid = info.commits[at].parent_oid;
            info.commits[at].parent_oid = Some(oid);
            info.commits.insert(at + 1, moved);
        }
        MoveSlot::Branch(name) => push_onto_branch(info, moved, name),
    }
}

/// What a commit being placed takes with it.
#[derive(Debug, PartialEq, Eq)]
enum CommitSource {
    /// Working files, passed as arguments (`c`): the index decides nothing.
    Files(Vec<String>),
    /// Whatever the hunk selector staged (`C`), committed as it stands.
    Index,
}

/// What folding the sources into a target does, tagged on the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FoldEffect {
    /// Working changes or a fixup go into the target commit.
    Amend,
    /// The target is a source, or the commit a source file already sits in.
    Noop,
    /// The source commit or commit file goes back to the working tree.
    Uncommit,
    /// A commit file, or a commit's picked hunks, move into the target commit.
    MoveFile,
}

impl FoldEffect {
    fn tag(self) -> &'static str {
        match self {
            FoldEffect::Amend => "[AMEND]",
            FoldEffect::Noop => "[NOOP]",
            FoldEffect::Uncommit => "[UNCOMMIT]",
            FoldEffect::MoveFile => "[MOVE]",
        }
    }
}

/// A row a pending fold can land on.
#[derive(Debug, Clone)]
struct FoldTarget {
    key: String,
    /// The row's command argument.
    arg: String,
    effect: FoldEffect,
}

/// A loom command to run on the worker thread.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// `loom commit -i|-b <branch> [files...]`. `c` always names files, so
    /// the index decides nothing; only `C`, which just staged what it picked,
    /// commits the index (no file arguments).
    Commit {
        source: CommitSource,
        dest: CommitDest,
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
    /// <id>... --hunks-from <fingerprint>` with the hunks `S` picked. `files`
    /// are cwd-relative, as the CLI takes them.
    Split {
        commit: String,
        files: Vec<String>,
        hunks: Option<HunkArgs>,
    },
    /// `loom branch new <name> [-t target]`.
    NewBranch {
        name: String,
        target: Option<String>,
    },
    /// `loom drop <targets...>`: one commit, branch, or `zz`, or several files.
    Drop { targets: Vec<String> },
    /// `loom absorb [files...]`; no files is every local change.
    Absorb { files: Vec<String> },
    /// `loom reword <target>`; `name` is the new branch name typed in the
    /// tree, `None` for a commit (the editor asks for the message).
    Reword {
        target: String,
        name: Option<String>,
    },
}

/// Why the event loop returned.
enum Outcome {
    Quit,
}

/// Input mode: normal, picking the target of a pending fold, placing a
/// pending commit, or typing a branch name over its row (an existing
/// branch's, or a placeholder row for a branch about to be created).
enum Mode {
    Normal,
    /// `↑`/`↓` move the cursor through `targets`, read off the tree `f` was
    /// pressed on; the tree is rebuilt for `targets[index]`.
    FoldTarget {
        sources: Vec<String>,
        source_rows: Sources,
        targets: Vec<FoldTarget>,
        index: usize,
        /// Key of the row `f` or `F` was pressed on, to go back to on cancel.
        origin: String,
        /// What `F` picked; `None` for `f`, which folds its sources whole.
        hunks: Option<FoldHunks>,
    },
    /// `↑`/`↓` move the placeholder commit through `dests`; the tree is
    /// rebuilt with it at `dests[index]`.
    CommitTarget {
        source: CommitSource,
        source_rows: Sources,
        dests: Vec<CommitDest>,
        index: usize,
        /// Key of the row `c` was pressed on, to go back to on cancel.
        origin: String,
    },
    /// `↑`/`↓` move `commit` through `slots`, each a distinct tree; the tree
    /// is rebuilt with it at `slots[index]`.
    MoveTarget {
        commit: git2::Oid,
        slots: Vec<MoveSlot>,
        index: usize,
    },
    RenameBranch {
        branch: String,
        field: TextField,
    },
    NewBranch {
        target: Option<String>,
        /// Commit the branch will point at; the fake branch is drawn there.
        tip: git2::Oid,
        /// Key of the row `b` was pressed on, to go back to on cancel.
        origin: String,
        field: TextField,
    },
}

/// The rows a pending fold or commit takes its content from, marked in the
/// gutter for as long as its target is being picked. Row keys rather than
/// command arguments: keys survive the rebuilds `↑`/`↓` trigger, and a row
/// subsumed by `zz` or by the index carries no argument of its own.
#[derive(Default)]
struct Sources {
    /// Rows the command names.
    named: HashSet<String>,
    /// Rows a named source subsumes: the files under a `zz` header, or the
    /// staged files `C` commits through the index.
    covered: HashSet<String>,
}

/// Whether the index holds something of `change` for a `C` commit to take.
/// The one spelling of it: the placeholder's file count and the rows marked
/// as covered must not be able to disagree.
fn is_staged(change: &FileChange) -> bool {
    matches!(change.index, 'A' | 'M' | 'D' | 'R')
}

/// Where a pick stands after one poll.
enum PickTick {
    Nothing,
    Redraw,
    /// `first` on the poll that read it, so a held pick redraws once rather
    /// than every tick with nothing animating.
    Ready {
        first: bool,
    },
}

/// What a pick's selection goes to.
enum PickFor {
    /// `C`: staged at once, then committed.
    Commit,
    /// `F`: folded into one of `targets`, read off the tree at the press so a
    /// pick with nowhere to go never opens. `commit` is the source commit when
    /// the hunks are its own rather than the working tree's.
    Fold {
        sources: Vec<String>,
        targets: Vec<FoldTarget>,
        commit: Option<git2::Oid>,
    },
    /// `S`: the hunks of `commit` that make the first of its two halves.
    Split { commit: git2::Oid },
}

impl PickFor {
    /// The command the pick is for, as notices and the status bar name it.
    fn label(&self) -> &'static str {
        match self {
            PickFor::Commit => "commit",
            PickFor::Fold { .. } => "fold",
            PickFor::Split { .. } => "split",
        }
    }
}

/// The two halves of a `C` or `F` pick. Reading the hunks is a git per changed
/// file, so it waits on a worker like any other slow work; only the selector
/// that follows needs the terminal, and only that half runs on the loop.
enum Pick {
    Collecting {
        handle: JoinHandle<Result<(Vec<FileEntry>, Stamp)>>,
        /// Key of the row `C` or `F` was pressed on, to go back to on cancel.
        origin: String,
        purpose: PickFor,
        /// Ticks so far, for the status-bar animation.
        ticks: usize,
        /// `Esc` was pressed: drop the result when it lands. The pick is held
        /// until then rather than abandoned, so no action can start beside a
        /// worker still running `git` over the same repository.
        cancelled: bool,
    },
    /// Read and waiting for the shell to hand the terminal over.
    Ready {
        entries: Vec<FileEntry>,
        stamp: Stamp,
        origin: String,
        purpose: PickFor,
    },
}

/// `staging::binary_stamp` of a pick's entries, taken as they are read; empty
/// for a pick that stages at once or reads a commit.
type Stamp = Vec<Option<git2::Oid>>;

/// The hunks `F` picked, folded instead of its source rows whole.
enum FoldHunks {
    /// Working-tree hunks, staged only once the target is confirmed, so a
    /// cancelled fold leaves the index as it was; `stamp` lets the fold
    /// refuse a pick the tree has moved away from by then.
    Worktree {
        picked: Vec<FileEntry>,
        stamp: Stamp,
    },
    /// Hunks of the source commit, handed to `loom fold -p` by id (Spec 019).
    Commit(Vec<FileEntry>),
}

/// The action currently running on its worker thread.
struct Running {
    handle: JoinHandle<Result<()>>,
    command: String,
    /// Text of the command's active spinner, if any.
    spinner: Option<String>,
    /// Ticks so far, for the status-bar animation.
    ticks: usize,
}

/// What is drawn over the panes and owns the keyboard.
enum Popup {
    /// A prompt from the running command; the answer goes back on `reply`.
    Prompt {
        prompt: Prompt,
        reply: Sender<Option<Answer>>,
    },
    /// A message to dismiss; `then` runs on dismissal.
    Notice {
        notice: Notice,
        then: AfterNotice,
    },
    Log {
        scroll: DiffPane,
    },
}

enum AfterNotice {
    Nothing,
    /// The action failed: reload the tree to show whatever it left.
    Reload,
    /// The action paused on conflicts: nothing else can run, so leave.
    Quit,
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ── Entry point ──────────────────────────────────────────────────────────

/// Run the interactive status TUI.
pub fn run(theme: graph::Theme) -> Result<()> {
    // Backstop for agent mode — the primary guard rejects `tui` at dispatch
    // time, but any future call path must not open a full-screen TUI either.
    if crate::core::agent_mode::enabled() {
        anyhow::bail!(
            "the TUI is interactive and unavailable in agent mode\n\
             Use `loom status` instead"
        );
    }

    let tui_theme = TuiTheme::from_graph_theme(&theme);
    let context = crate::status::resolve_context(&repo::open_repo()?, None);
    let snapshot = load_snapshot(context)?;
    let git_dir = snapshot.git_dir.clone();

    // Local changes start expanded.
    let mut expanded: HashSet<String> = HashSet::new();
    expanded.insert(LOCAL_CHANGES_KEY.to_string());

    let app = App::new(snapshot, &tui_theme, theme, expanded, context);
    let (_, Outcome::Quit) = Shell::new(app).run()?;

    // The TUI leaves on a conflict pause; repeat the popup's guidance where
    // it stays readable.
    if let Ok(Some(state)) = transaction::load(&git_dir) {
        println!("{}", paused_message(&state.command));
    }
    Ok(())
}

fn paused_message(command: &str) -> String {
    format!(
        "A `loom {}` is paused due to conflicts.\n\
         Resolve them, then run `loom continue` to resume, or `loom abort` to cancel.",
        command
    )
}

/// Gather repo info, exactly like `loom status` with files enabled.
fn load_snapshot(context: usize) -> Result<Snapshot> {
    let repo = repo::open_repo()?;
    let workdir = repo::require_workdir(&repo, "display status")?.to_path_buf();
    let git_dir = repo.path().to_path_buf();
    let cwd_prefix = repo::cwd_relative_to_repo(&repo).unwrap_or_default();

    let mut info = repo::gather_repo_info(&repo, true, context)?;
    // Collect entities before filtering so short IDs stay stable.
    let ids = IdAllocator::new(info.collect_entities());
    crate::status::apply_hidden_branches(&repo, &mut info);

    Ok(Snapshot {
        workdir,
        git_dir,
        cwd_prefix,
        info,
        ids,
    })
}

/// Run the loom command for `action`. Runs on the worker thread: the trace
/// logger is thread-local, so each action gets its own log file, named after
/// the command line the log shows.
fn execute_action(
    action: Action,
    command: &str,
    git_dir: &std::path::Path,
    theme: &graph::Theme,
) -> Result<()> {
    crate::trace::init(git_dir, &format!("loom tui: {}", command));
    let result = match action {
        Action::Commit { source, dest } => {
            let (branch, integration) = match dest {
                CommitDest::Integration => (None, true),
                CommitDest::Branch(name) => (Some(name), false),
            };
            let files = match source {
                CommitSource::Files(files) => files,
                CommitSource::Index => vec![],
            };
            commit::run(branch, integration, None, None, files, vec![], theme)
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
        } => {
            let patch = hunks.is_some();
            let hunks = hunks.unwrap_or_default();
            split::run(commit, None, patch, hunks, files, theme)
        }
        Action::NewBranch { name, target } => branch::new::run(Some(name), target),
        Action::Drop { targets } => drop::run(targets, false),
        Action::Absorb { files } => absorb::run(false, files),
        Action::Reword { target, name } => reword::run(target, name),
    };
    crate::trace::finalize();
    result
}

// ── App state ────────────────────────────────────────────────────────────

struct App<'a> {
    snapshot: Snapshot,
    theme: &'a TuiTheme,
    /// Handed to each action's worker (the hunk pickers take it).
    graph_theme: graph::Theme,
    rows: Vec<Row>,
    /// Tree pane cursor and view state.
    tree: ListPane,
    /// Keys of the multi-selected rows.
    selected: HashSet<String>,
    /// Class every selected row belongs to; `None` when nothing is selected.
    selected_class: Option<SelectionClass>,
    expanded: HashSet<String>,
    /// Context depth the tree is loaded with, as `loom status <N>` takes it.
    context: usize,
    mode: Mode,
    /// Right-pane scroll state.
    diff: DiffPane,
    /// Diff lines cached per row key.
    diff_cache: HashMap<String, Vec<Line<'static>>>,
    /// Row key to put the cursor on after the next reload, for an action that
    /// renames the row it acts on.
    next_cursor: Option<String>,
    /// Row key to fall back to when the action fails and the row it ran from
    /// was only a preview (`b`, `c`), so no longer exists after the reload.
    fallback_cursor: Option<String>,
    /// The `C` pick in flight, from the key press to the selector.
    pick: Option<Pick>,
    /// `q`/`Ctrl-C` during a pick: leave once its worker lands.
    quit_after_pick: bool,
    /// Transient message shown in the status bar until the next key.
    notice: Option<String>,
    /// Exit value set by deep handlers, drained after each key.
    outcome: Option<Outcome>,
    /// Requests from the running action (`core::ui`); `request_tx` is what
    /// each worker gets a clone of.
    requests: Receiver<Request>,
    request_tx: Sender<Request>,
    running: Option<Running>,
    popup: Option<Popup>,
    log: Vec<LogEntry>,
}

impl<'a> App<'a> {
    fn new(
        snapshot: Snapshot,
        theme: &'a TuiTheme,
        graph_theme: graph::Theme,
        expanded: HashSet<String>,
        context: usize,
    ) -> Self {
        let (request_tx, requests) = channel();
        let mut app = App {
            snapshot,
            theme,
            graph_theme,
            rows: Vec::new(),
            tree: ListPane::new(0),
            selected: HashSet::new(),
            selected_class: None,
            expanded,
            context,
            mode: Mode::Normal,
            diff: DiffPane::new(),
            diff_cache: HashMap::new(),
            next_cursor: None,
            fallback_cursor: None,
            pick: None,
            quit_after_pick: false,
            notice: None,
            outcome: None,
            requests,
            request_tx,
            running: None,
            popup: None,
            log: Vec::new(),
        };
        app.rows = app.build_rows();
        let cursor = app.rows.iter().position(|r| r.focusable).unwrap_or(0);
        app.tree.set_cursor(cursor);
        // Prime the diff for the initial cursor row so the first render
        // doesn't have to.
        app.ensure_diff_cached();
        app
    }

    /// The tree rows for the current snapshot, expansion state, and — while
    /// a new branch is being named or a commit placed — its placeholder row.
    fn build_rows(&self) -> Vec<Row> {
        let preview = match &self.mode {
            Mode::NewBranch { tip, .. } => Some(Preview::Branch(BranchInfo {
                name: NEW_BRANCH_NAME.to_string(),
                tip_oid: *tip,
                remote: None,
            })),
            Mode::CommitTarget {
                source,
                dests,
                index,
                ..
            } => Some(Preview::Commit {
                dest: dests[*index].clone(),
                file_count: match source {
                    CommitSource::Files(files) => self.commit_file_count(files),
                    CommitSource::Index => self.snapshot.staged_count(),
                },
            }),
            Mode::MoveTarget {
                commit,
                slots,
                index,
            } => Some(Preview::Move {
                oid: *commit,
                slot: slots[*index].clone(),
            }),
            _ => None,
        };
        let sections = self.snapshot.sections(preview);
        status_tree::build_rows(&sections, &self.snapshot.ids, &self.expanded)
    }

    /// How many working files `files` (short IDs, or `zz`) stand for.
    fn commit_file_count(&self, files: &[String]) -> usize {
        if self.snapshot.is_all_changes(files) {
            self.snapshot.info.working_changes.len()
        } else {
            files.len()
        }
    }

    fn current_row(&self) -> Option<&Row> {
        self.rows.get(self.tree.cursor())
    }

    /// Replace the repo state, keeping the cursor on the same row when it
    /// still exists. Selection and the diff cache are tied to the old rows.
    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let key = self
            .next_cursor
            .take()
            .or_else(|| self.current_row().map(|r| r.key.clone()));
        let previous = self.tree.cursor();
        self.snapshot = snapshot;
        self.clear_selection();
        self.diff_cache.clear();
        self.rows = self.build_rows();
        let cursor = key
            .and_then(|key| self.rows.iter().position(|r| r.focusable && r.key == key))
            .or_else(|| nearest_focusable(&self.rows, previous))
            .unwrap_or(0);
        self.tree.set_cursor(cursor);
        self.diff.reset();
        self.ensure_diff_cached();
    }

    /// Reload the tree from the repo; a failure is shown, the old tree kept.
    /// Reports whether the new snapshot was applied.
    fn reload(&mut self) -> bool {
        match load_snapshot(self.context) {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                true
            }
            Err(e) => {
                self.next_cursor = None;
                self.show_error(&e.to_string(), AfterNotice::Nothing);
                false
            }
        }
    }

    /// Show `delta` more (or fewer) context commits before the base and
    /// reload. Depth 1 is the floor: the base alone, as `loom status` starts.
    fn change_context(&mut self, delta: isize) {
        let depth = self.context.saturating_add_signed(delta).max(1);
        if depth == self.context {
            self.notice = Some(format!("context: {}", depth));
            return;
        }
        let previous = self.context;
        self.context = depth;
        if self.reload() {
            // The walk stops at the root commit, so asking for more than
            // history holds must not run the depth away from the tree: `-`
            // would then take as many presses to show anything.
            self.context = depth.min(self.snapshot.info.context_commits.len() + 1);
            self.notice = Some(format!("context: {}", self.context));
        } else {
            self.context = previous;
        }
    }

    fn show_error(&mut self, text: &str, then: AfterNotice) {
        self.popup = Some(Popup::Notice {
            notice: Notice::new("Error", Level::Error, text),
            then,
        });
    }

    // -- running an action ------------------------------------------------------

    /// The short ID the tree shows for `target`, or `target` itself when no
    /// row carries one.
    fn sid_of(&self, target: &str) -> String {
        self.rows
            .iter()
            .find(|r| r.target.as_deref() == Some(target) && !r.sid.is_empty())
            .map(|r| r.sid.clone())
            .unwrap_or_else(|| target.to_string())
    }

    /// `title` built from the sources' short IDs, or from a count when that
    /// would not fit the pane: the title is what still names the sources
    /// once they scroll off the tree.
    fn title_sources(
        &self,
        targets: &[String],
        width: u16,
        title: impl Fn(&str) -> String,
    ) -> String {
        let joined = targets
            .iter()
            .map(|t| self.sid_of(t))
            .collect::<Vec<_>>()
            .join(" ");
        let full = title(&joined);
        // Two columns for the corners the title sits between.
        if full.chars().count() + 2 <= width as usize {
            full
        } else {
            title(&format!("{} item(s)", targets.len()))
        }
    }

    /// The CLI line equivalent to `action`, with the short IDs the tree
    /// shows, for the log.
    fn command_line(&self, action: &Action) -> String {
        let sid = |target: &str| self.sid_of(target);
        let mut words = vec!["loom".to_string()];
        match action {
            Action::Commit { source, dest } => {
                words.push("commit".into());
                match dest {
                    CommitDest::Integration => words.push("-i".into()),
                    CommitDest::Branch(name) => words.extend(["-b".into(), sid(name)]),
                }
                // `C` staged what it picked, so this half names no files.
                if let CommitSource::Files(files) = source {
                    words.extend(files.iter().map(|f| sid(f)));
                }
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
                if let Some(hunks) = hunks {
                    for id in &hunks.ids {
                        words.extend(["--hunks".into(), hunk_select::quoted(id)]);
                    }
                    if let Some(from) = &hunks.from {
                        words.extend(["--hunks-from".into(), from.clone()]);
                    }
                }
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
            } => {
                words.push("split".into());
                if hunks.is_some() {
                    words.push("-p".into());
                }
                words.push(sid(commit));
                words.extend(files.iter().map(|f| hunk_select::quoted(f)));
                if let Some(hunks) = hunks {
                    for id in &hunks.ids {
                        words.extend(["--hunks".into(), hunk_select::quoted(id)]);
                    }
                    if let Some(from) = &hunks.from {
                        words.extend(["--hunks-from".into(), from.clone()]);
                    }
                }
            }
            Action::NewBranch { name, target } => {
                words.extend(["branch".into(), "new".into(), name.clone()]);
                if let Some(target) = target {
                    words.extend(["-t".into(), sid(target)]);
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
            Action::Reword { target, name } => {
                words.extend(["reword".into(), sid(target)]);
                if let Some(name) = name {
                    words.extend(["-m".into(), name.clone()]);
                }
            }
        }
        words.join(" ")
    }

    /// Run `action` on a worker thread; the TUI keeps going and answers its
    /// prompts.
    fn start_action(&mut self, action: Action) {
        let command = self.command_line(&action);
        self.log.push(LogEntry {
            command: command.clone(),
            lines: Vec::new(),
        });
        self.clear_selection();
        self.mode = Mode::Normal;
        self.notice = None;

        let tx = self.request_tx.clone();
        let git_dir = self.snapshot.git_dir.clone();
        let theme = self.graph_theme.clone();
        let trace_name = command.clone();
        let handle = std::thread::spawn(move || {
            ui::install(tx);
            let result = execute_action(action, &trace_name, &git_dir, &theme);
            ui::uninstall();
            result
        });
        self.running = Some(Running {
            handle,
            command,
            spinner: None,
            ticks: 0,
        });
    }

    /// One request from the running action. `Suspend` is the shell's to
    /// handle and never reaches here.
    fn handle_request(&mut self, request: Request) {
        match request {
            Request::Prompt {
                kind,
                prompt,
                error,
                reply,
            } => {
                let command = self.running.as_ref().map_or("", |r| r.command.as_str());
                self.popup = Some(Popup::Prompt {
                    prompt: Prompt::new(kind, prompt, error, command),
                    reply,
                });
            }
            Request::Message { level, text } => {
                if let Some(entry) = self.log.last_mut() {
                    entry.lines.push((level, text));
                }
            }
            Request::Spinner(text) => {
                if let Some(running) = &mut self.running {
                    running.spinner = text;
                }
            }
            Request::Suspend(_) | Request::Resume => {}
        }
    }

    /// The worker is done: report in the status bar (success), a popup
    /// (failure), or nothing (cancelled prompt). Returns whether the repo may
    /// have changed, so the caller runs [`App::after_action`].
    fn finish_action(&mut self, result: Result<()>) -> bool {
        if result.is_err() {
            // Nothing was renamed or created: aim at the preview's origin row,
            // or at nothing, since the old key is still the live one.
            self.next_cursor = self.fallback_cursor.take();
        }
        self.fallback_cursor = None;
        match result {
            Ok(()) => {
                // A multi-target command (`drop a b`) prints one ✓ per target;
                // the bar shows the last and says how many it stands for.
                let successes: Vec<&str> = self.log.last().map_or_else(Vec::new, |entry| {
                    entry
                        .lines
                        .iter()
                        .filter(|(level, _)| *level == Level::Success)
                        .filter_map(|(_, text)| text.lines().next())
                        .collect()
                });
                self.notice = Some(match successes.as_slice() {
                    [] => "✓ done".to_string(),
                    [one] => format!("✓ {}", one),
                    [.., last] => {
                        format!("✓ {} (+{} more, L: log)", last, successes.len() - 1)
                    }
                });
                true
            }
            Err(e) if e.downcast_ref::<Cancelled>().is_some() => {
                self.log_line(Level::Warn, "Cancelled");
                self.notice = Some("cancelled".to_string());
                false
            }
            Err(e) => {
                let text = e.to_string();
                self.log_line(Level::Error, &text);
                // An open log already shows the error line, so reload behind
                // it instead of replacing what the reader is looking at.
                if matches!(self.popup, Some(Popup::Log { .. })) {
                    return true;
                }
                self.show_error(&text, AfterNotice::Reload);
                false
            }
        }
    }

    fn log_line(&mut self, level: Level, text: &str) {
        if let Some(entry) = self.log.last_mut() {
            entry.lines.push((level, text.to_string()));
        }
    }

    /// After an action changed the repo: a conflict pause ends the session
    /// (every other command is blocked until it's resolved), otherwise the
    /// tree reloads.
    fn after_action(&mut self) {
        match transaction::load(&self.snapshot.git_dir) {
            Ok(Some(state)) => {
                self.popup = Some(Popup::Notice {
                    notice: Notice::new("Paused", Level::Warn, &paused_message(&state.command)),
                    then: AfterNotice::Quit,
                });
            }
            _ => {
                self.reload();
            }
        }
    }

    fn dismiss_notice(&mut self, then: AfterNotice) {
        match then {
            AfterNotice::Nothing => {}
            AfterNotice::Reload => self.after_action(),
            AfterNotice::Quit => self.outcome = Some(Outcome::Quit),
        }
    }

    fn handle_popup_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        match popup {
            Popup::Prompt { mut prompt, reply } => match prompt.handle_key(code, modifiers) {
                PromptOutcome::Pending => self.popup = Some(Popup::Prompt { prompt, reply }),
                PromptOutcome::Answer(answer) => {
                    let _ = reply.send(Some(answer));
                }
                PromptOutcome::Cancel => {
                    let _ = reply.send(None);
                }
            },
            Popup::Notice { notice, then } => {
                if Notice::dismisses(code) {
                    self.dismiss_notice(then);
                } else {
                    self.popup = Some(Popup::Notice { notice, then });
                }
            }
            Popup::Log { mut scroll } => match code {
                KeyCode::Esc | KeyCode::Char('q' | 'L') => {}
                _ => {
                    match code {
                        KeyCode::Up | KeyCode::Char('k') => scroll.scroll_by(-1),
                        KeyCode::Down | KeyCode::Char('j') => scroll.scroll_by(1),
                        KeyCode::PageUp => scroll.scroll_page(-1),
                        KeyCode::PageDown => scroll.scroll_page(1),
                        _ => {}
                    }
                    self.popup = Some(Popup::Log { scroll });
                }
            },
        }
    }

    fn open_log(&mut self) {
        let mut scroll = DiffPane::new();
        // Start at the newest entry; the render clamps to the content.
        scroll.set_scroll(u16::MAX);
        self.popup = Some(Popup::Log { scroll });
    }

    /// Rebuild rows after an expansion change, keeping the cursor on `key`.
    /// Rows that went away take their selection with them: an off-screen row
    /// would keep vetoing every other class with no marker in sight.
    fn rebuild_rows(&mut self, key: &str) {
        self.rows = self.build_rows();
        let visible: HashSet<&str> = self.rows.iter().map(|r| r.key.as_str()).collect();
        self.selected.retain(|k| visible.contains(k.as_str()));
        if self.selected.is_empty() {
            self.selected_class = None;
        }
        if let Some(pos) = self.rows.iter().position(|r| r.key == key) {
            self.tree.set_cursor(pos);
        } else if self.tree.cursor() >= self.rows.len() {
            self.tree.set_cursor(self.rows.len().saturating_sub(1));
        }
    }

    // -- keyboard handling ----------------------------------------------------

    /// A key with no popup open and no action running.
    fn handle_tree_key(&mut self, focused: PaneId, code: KeyCode) {
        let action = match code {
            KeyCode::Esc => {
                self.handle_escape();
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                match focused {
                    PaneId::Left => self.move_cursor(-1),
                    PaneId::Right => self.diff.scroll_by(-1),
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match focused {
                    PaneId::Left => self.move_cursor(1),
                    PaneId::Right => self.diff.scroll_by(1),
                }
                None
            }
            KeyCode::PageUp => {
                self.diff.scroll_page(-1);
                None
            }
            KeyCode::PageDown => {
                self.diff.scroll_page(1);
                None
            }
            // Folding is how the cursor leaves a row: `←` on a child walks up
            // to its parent. The cursor must stay on the placeholder or the
            // fold target, and a closed commit would hide a commit-file source.
            KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l')
                if self.placing().is_some() =>
            {
                let what = self.placing().unwrap_or_default();
                self.notice = Some(format!("{}: Enter to confirm, Esc to cancel", what));
                None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.expand_current();
                None
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.collapse_current();
                None
            }
            KeyCode::Enter => match &self.mode {
                Mode::FoldTarget { .. } => self.confirm_fold_target(),
                Mode::CommitTarget { .. } => self.confirm_commit_target(),
                Mode::MoveTarget { .. } => self.confirm_move_target(),
                // Rename mode never gets here: it is handled above.
                _ => {
                    self.toggle_current();
                    None
                }
            },
            KeyCode::Char('L') => {
                self.open_log();
                None
            }
            // While picking a fold target or placing a commit only navigation,
            // Enter, and Esc apply — action keys must not fire and discard
            // the pending operation.
            KeyCode::Char(
                ' ' | 'c' | 'C' | 'f' | 'F' | 'm' | 's' | 'S' | 'b' | 'd' | 'a' | 'r' | 'R' | '+'
                | '=' | '-',
            )
            | KeyCode::F(5)
                if self.placing().is_some() =>
            {
                let what = self.placing().unwrap_or_default();
                self.notice = Some(format!("{}: Enter to confirm, Esc to cancel", what));
                None
            }
            KeyCode::Char(' ') => {
                self.toggle_selection();
                None
            }
            KeyCode::Char('c') => {
                self.action_commit_start();
                None
            }
            KeyCode::Char('C') => {
                self.action_commit_patch_start();
                None
            }
            KeyCode::Char('f') => {
                self.action_fold_start();
                None
            }
            KeyCode::Char('F') => {
                self.action_fold_patch_start();
                None
            }
            KeyCode::Char('m') => {
                self.action_move_start();
                None
            }
            KeyCode::Char('s') => self.action_split(),
            KeyCode::Char('S') => {
                self.action_split_patch_start();
                None
            }
            KeyCode::Char('b') => {
                self.action_new_branch();
                None
            }
            KeyCode::Char('d') => self.action_drop(),
            KeyCode::Char('a') => self.action_absorb(),
            KeyCode::Char('r') => self.action_reword(),
            KeyCode::Char('R') | KeyCode::F(5) => {
                self.reload();
                None
            }
            // `=` is `+` without Shift on most layouts, next to `-`.
            KeyCode::Char('+' | '=') => {
                self.change_context(1);
                None
            }
            KeyCode::Char('-') => {
                self.change_context(-1);
                None
            }
            _ => None,
        };
        if let Some(action) = action {
            self.start_action(action);
        }
    }

    /// The command whose target the tree is walking, if any: only navigation,
    /// Enter and Esc apply then.
    fn placing(&self) -> Option<&'static str> {
        match self.mode {
            Mode::CommitTarget { .. } => Some("commit"),
            Mode::FoldTarget { .. } => Some("fold"),
            Mode::MoveTarget { .. } => Some("move"),
            _ => None,
        }
    }

    /// Esc: cancel fold-target, commit or move mode, else clear the selection,
    /// else quit.
    fn handle_escape(&mut self) {
        if matches!(self.mode, Mode::FoldTarget { .. }) {
            self.cancel_fold_target();
        } else if matches!(self.mode, Mode::CommitTarget { .. }) {
            self.cancel_commit_target();
        } else if matches!(self.mode, Mode::MoveTarget { .. }) {
            self.cancel_move_target();
        } else if !self.selected.is_empty() {
            self.clear_selection();
        } else {
            self.outcome = Some(Outcome::Quit);
        }
    }

    fn move_cursor(&mut self, dir: isize) {
        match self.mode {
            Mode::CommitTarget { .. } => return self.move_commit_dest(dir),
            Mode::FoldTarget { .. } => return self.move_fold_target(dir),
            Mode::MoveTarget { .. } => return self.move_move_target(dir),
            _ => {}
        }
        if self
            .tree
            .move_cursor(dir, self.rows.len(), |i| self.rows[i].focusable)
        {
            self.diff.reset();
        }
    }

    fn expand_current(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        if row.expandable && !row.expanded {
            let key = row.key.clone();
            self.expanded.insert(key.clone());
            self.rebuild_rows(&key);
        }
    }

    /// Collapse the current row, or jump to (and collapse) its parent when the
    /// cursor is on a child file row.
    fn collapse_current(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        if row.expandable && row.expanded {
            let key = row.key.clone();
            self.expanded.remove(&key);
            self.rebuild_rows(&key);
            return;
        }
        if matches!(
            row.kind,
            RowKind::WorkingFile { .. } | RowKind::CommitFile { .. }
        ) {
            // Walk up to the expandable parent.
            let mut pos = self.tree.cursor();
            while pos > 0 {
                pos -= 1;
                if self.rows[pos].expandable {
                    let key = self.rows[pos].key.clone();
                    self.expanded.remove(&key);
                    self.rebuild_rows(&key);
                    return;
                }
            }
        }
    }

    fn toggle_current(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        if !row.expandable {
            return;
        }
        if row.expanded {
            self.collapse_current();
        } else {
            self.expand_current();
        }
    }

    /// Toggle the cursor row in or out of the selection. A selection holds one
    /// class of row: toggling in a different one is refused rather than
    /// replacing what is already selected, since no action could use the mix.
    fn toggle_selection(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        let Some(class) = row.selection_class() else {
            return;
        };
        let key = row.key.clone();
        if self.selected.remove(&key) {
            if self.selected.is_empty() {
                self.selected_class = None;
            }
        } else {
            if let Some(current) = self.selected_class
                && current != class
            {
                self.notice = Some(format!(
                    "selection holds {}; Esc clears it",
                    current.label()
                ));
                return;
            }
            self.selected.insert(key);
            self.selected_class = Some(class);
        }
        self.move_cursor(1);
    }

    fn clear_selection(&mut self) {
        self.selected.clear();
        self.selected_class = None;
    }

    // -- actions ----------------------------------------------------------------

    /// The selected rows, in tree order, else the cursor row.
    fn picked_rows(&self) -> Vec<&Row> {
        if self.selected.is_empty() {
            self.current_row().into_iter().collect()
        } else {
            self.rows
                .iter()
                .filter(|r| self.selected.contains(&r.key))
                .collect()
        }
    }

    /// The gutter marks for `rows` as the sources of a pending command: the
    /// rows themselves, plus the working files a `zz` header among them
    /// subsumes.
    fn mark_sources(&self, rows: &[&Row]) -> Sources {
        let covers_all = rows
            .iter()
            .any(|r| matches!(r.kind, RowKind::LocalChanges { .. }));
        Sources {
            named: rows.iter().map(|r| r.key.clone()).collect(),
            covered: if covers_all {
                self.working_file_keys(|_| true)
            } else {
                HashSet::new()
            },
        }
    }

    /// Keys of the working-file rows matching `keep`, read off the snapshot
    /// rather than the drawn rows: `[local changes]` may be closed, and the
    /// files it hides are covered all the same.
    fn working_file_keys(&self, keep: impl Fn(&FileChange) -> bool) -> HashSet<String> {
        self.snapshot
            .info
            .working_changes
            .iter()
            .filter(|c| keep(c))
            .map(|c| working_file_key(&c.path))
            .collect()
    }

    /// The gutter marks for a `C` commit: the index is the source, so the
    /// staged files are all there is to point at — no row names it.
    fn index_sources(&self) -> Sources {
        let mut covered = self.working_file_keys(is_staged);
        // The one row that is drawn whether `[local changes]` is open or
        // closed, so a `C` placement is never left with no mark at all.
        covered.insert(LOCAL_CHANGES_KEY.to_string());
        Sources {
            named: HashSet::new(),
            covered,
        }
    }

    /// The gutter mark for `row`. A pending command's sources outrank the
    /// selection, which `Space` cannot change while a target is being picked.
    fn row_mark(&self, row: &Row) -> RowMark {
        let source_rows = match &self.mode {
            Mode::CommitTarget { source_rows, .. } | Mode::FoldTarget { source_rows, .. } => {
                Some(source_rows)
            }
            _ => None,
        };
        if let Some(sources) = source_rows {
            if sources.named.contains(&row.key) {
                return RowMark::Source;
            }
            if sources.covered.contains(&row.key) {
                return RowMark::Covered;
            }
        }
        if self.selected.contains(&row.key) {
            return RowMark::Selected;
        }
        RowMark::None
    }

    /// `c`: the selected working files or `[local changes]` header (else the
    /// cursor's) are the commit; the index plays no part. The tree is then
    /// redrawn with the commit at its destination, and nothing runs until
    /// that is confirmed.
    fn action_commit_start(&mut self) {
        let Some((files, source_rows, origin)) = self.commit_sources() else {
            return;
        };
        self.enter_commit_target(CommitSource::Files(files), source_rows, origin);
    }

    /// `C`: every local change, whatever the cursor or selection — a picker
    /// showing one file would hide the rest of the index it is about to
    /// rewrite. The press starts the read on a worker; the selector that
    /// follows is what asks the shell for the terminal.
    fn action_commit_patch_start(&mut self) {
        if self.snapshot.info.working_changes.is_empty() {
            self.notice = Some("commit: no local changes".to_string());
            return;
        }
        let Some(origin) = self.current_row().map(|r| r.key.clone()) else {
            return;
        };
        let files = vec![self.snapshot.ids.get_unstaged().to_string()];
        let command = format!("loom add -p {}", files.join(" "));
        self.start_pick(command, origin, PickFor::Commit, move |workdir| {
            let repo = git2::Repository::open(workdir)?;
            let filter = staging::filter_paths(&repo, &files)?;
            let entries = staging::collect_file_entries(&repo, workdir, filter.as_deref())?;
            Ok((entries, Vec::new()))
        });
    }

    /// `F`: `f` with the hunks picked first — out of one commit's own diff,
    /// or, on working files or local changes, out of every local change, as
    /// `C` shows them whatever the cursor or selection. The press starts the
    /// read on a worker, as `C` does.
    fn action_fold_patch_start(&mut self) {
        let rows = self.picked_rows();
        let Some(origin) = self.current_row().map(|r| r.key.clone()) else {
            return;
        };
        let mut sources: Vec<String> = rows.iter().filter_map(|r| r.target.clone()).collect();
        if sources.is_empty() || sources.len() != rows.len() {
            self.notice = Some("fold: select source rows first".to_string());
            return;
        }
        let commit = match rows[0].kind {
            RowKind::Commit { oid, .. } => Some(oid),
            RowKind::LocalChanges { .. } | RowKind::WorkingFile { .. } => None,
            _ => {
                self.notice = Some("fold: move to a file, local changes, or a commit".to_string());
                return;
            }
        };
        let targets = match self.fold_targets(&rows, true) {
            Ok(targets) => targets,
            Err(notice) => {
                self.notice = Some(format!("fold: {}", notice));
                return;
            }
        };
        if commit.is_none() {
            sources = vec![self.snapshot.ids.get_unstaged().to_string()];
        }
        let shown: Vec<String> = sources.iter().map(|s| self.sid_of(s)).collect();
        let command = format!("loom fold -p {}", shown.join(" "));
        let purpose = PickFor::Fold {
            sources: sources.clone(),
            targets,
            commit,
        };
        match commit {
            Some(oid) => self.start_pick(command, origin, purpose, move |workdir| {
                let entries = staging::collect_commit_hunks(workdir, &oid.to_string(), &[])?;
                Ok((entries, Vec::new()))
            }),
            None => self.start_pick(command, origin, purpose, move |workdir| {
                let repo = git2::Repository::open(workdir)?;
                let entries = staging::collect_file_entries(&repo, workdir, None)?;
                let stamp = staging::binary_stamp(workdir, &entries);
                Ok((entries, stamp))
            }),
        }
    }

    /// Open `command`'s log entry and start `read` on a worker, over the
    /// snapshot's working tree.
    fn start_pick(
        &mut self,
        command: String,
        origin: String,
        purpose: PickFor,
        read: impl FnOnce(&std::path::Path) -> Result<(Vec<FileEntry>, Stamp)> + Send + 'static,
    ) {
        // Pushed before the worker starts, not when the staging happens: the
        // read is part of this command and its messages have to land here.
        self.log.push(LogEntry {
            command: command.clone(),
            lines: Vec::new(),
        });
        // The repository is opened here rather than from the process cwd: this
        // runs off the loop, and the tree it must agree with is the snapshot's.
        let workdir = self.snapshot.workdir.clone();
        let git_dir = self.snapshot.git_dir.clone();
        let tx = self.request_tx.clone();
        let handle = std::thread::spawn(move || {
            // Without the sink `msg` prints, and this thread would write onto
            // the screen the loop is drawing at the same moment.
            ui::install(tx);
            crate::trace::init(&git_dir, &format!("loom tui: {command}"));
            let result = read(&workdir);
            crate::trace::finalize();
            ui::uninstall();
            result
        });
        self.pick = Some(Pick::Collecting {
            handle,
            origin,
            purpose,
            ticks: 0,
            cancelled: false,
        });
    }

    /// The working files `c` stands for, their gutter marks, and the key of
    /// the row it came from. `None` when there is nothing to commit, after a
    /// notice saying which of the reasons it was — or silently, when there is
    /// no row to stand on.
    fn commit_sources(&mut self) -> Option<(Vec<String>, Sources, String)> {
        let rows = self.picked_rows();
        if rows.is_empty()
            || !rows.iter().all(|r| {
                matches!(
                    r.kind,
                    RowKind::WorkingFile { .. } | RowKind::LocalChanges { .. }
                )
            })
        {
            self.notice = Some("commit: move to local changes or select files".to_string());
            return None;
        }
        if rows
            .iter()
            .any(|r| matches!(r.kind, RowKind::LocalChanges { count: 0 }))
        {
            self.notice = Some("commit: no local changes".to_string());
            return None;
        }
        // A row of these kinds always carries a target; refuse rather than
        // fall through to an argument-less `loom commit`, which would commit
        // the index — the one thing `c` must never do. An unnamed row is the
        // same refusal: `loom commit -i ""` is not an improvement.
        let Some(files) = rows
            .iter()
            .map(|r| r.target.clone().filter(|t| !t.is_empty()))
            .collect::<Option<Vec<String>>>()
        else {
            self.notice = Some("commit: this row has nothing to commit".to_string());
            return None;
        };
        let source_rows = self.mark_sources(&rows);
        let origin = self.current_row().map(|r| r.key.clone())?;
        Some((files, source_rows, origin))
    }

    /// Redraw the tree with the placeholder commit and wait for a destination.
    fn enter_commit_target(&mut self, source: CommitSource, source_rows: Sources, origin: String) {
        // The destinations are read off the drawn tree, so their order is the
        // order `↑`/`↓` walk them in.
        let mut dests = vec![CommitDest::Integration];
        dests.extend(self.rows.iter().filter_map(|r| match &r.kind {
            RowKind::BranchName { name, .. } => Some(CommitDest::Branch(name.clone())),
            _ => None,
        }));
        self.mode = Mode::CommitTarget {
            source,
            source_rows,
            dests,
            index: 0,
            origin,
        };
        // Every placement shares one row key but shows the files this press
        // picked, so an earlier press's entry has to go. Moving the commit
        // afterwards never changes it: only the destination moves.
        self.diff_cache.remove(&PENDING_COMMIT_OID.to_string());
        self.diff.reset();
        self.show_pending_commit();
    }

    /// End the pick as soon as its read lands, and leave afterwards if asked.
    /// The worker is waited for either way: it reads with `git`, and one
    /// outliving its pick would run beside whatever comes next.
    fn cancel_pick(&mut self, then_quit: bool) {
        if let Some(Pick::Collecting { cancelled, .. }) = &mut self.pick {
            *cancelled = true;
            self.quit_after_pick |= then_quit;
        }
    }

    /// Move a pick along one tick. A read that fails, or one `Esc` marked,
    /// ends here too: the pick goes, and the loop only redraws.
    ///
    /// The worker is joined here rather than in [`App::take_over`], so the
    /// loop keeps drawing — a spinner, the tree, a resize — while it reads.
    fn advance_pick(&mut self) -> PickTick {
        match self.pick.take() {
            None => PickTick::Nothing,
            Some(ready @ Pick::Ready { .. }) => {
                self.pick = Some(ready);
                PickTick::Ready { first: false }
            }
            Some(Pick::Collecting {
                handle,
                origin,
                purpose,
                ticks,
                cancelled,
            }) if !handle.is_finished() => {
                self.pick = Some(Pick::Collecting {
                    handle,
                    origin,
                    purpose,
                    ticks: ticks + 1,
                    cancelled,
                });
                PickTick::Redraw
            }
            Some(Pick::Collecting {
                handle,
                origin,
                purpose,
                cancelled,
                ..
            }) => {
                let read = handle.join().unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "reading the hunks crashed — run `loom trace` for what it did"
                    ))
                });
                if cancelled {
                    // Waited for on purpose: the result is dropped, but not the
                    // worker, so nothing of it outlives the pick.
                    self.log_line(Level::Warn, "Cancelled");
                    self.notice = Some(format!("{} cancelled", purpose.label()));
                    if self.quit_after_pick {
                        self.outcome = Some(Outcome::Quit);
                    }
                    return PickTick::Redraw;
                }
                match read {
                    Ok((entries, stamp)) => {
                        self.pick = Some(Pick::Ready {
                            entries,
                            stamp,
                            origin,
                            purpose,
                        });
                        PickTick::Ready { first: true }
                    }
                    Err(e) => {
                        // As a failed action reports: into the entry the press
                        // opened, and without taking an open log off the reader
                        // who is already seeing the line.
                        let text = e.to_string();
                        self.log_line(Level::Error, &text);
                        if !matches!(self.popup, Some(Popup::Log { .. })) {
                            self.show_error(&text, AfterNotice::Nothing);
                        }
                        PickTick::Redraw
                    }
                }
            }
        }
    }

    /// What a pick leaves behind once the selector is done with it: the
    /// placement, or a line saying why there is none. Split from the selector
    /// half, which cannot run without a terminal.
    fn finish_pick(&mut self, staged: Result<bool>, origin: String) {
        match staged {
            Ok(true) => {
                if self.reload() {
                    let source_rows = self.index_sources();
                    self.enter_commit_target(CommitSource::Index, source_rows, origin);
                }
            }
            // Every way out of a pick leaves a line: the entry the press
            // opened is the only record of what `C` did, and one that ends
            // mute cannot be told from one that ran and said nothing.
            Ok(false) => {
                self.log_line(Level::Warn, "Cancelled");
                self.notice = Some("commit cancelled".to_string());
            }
            Err(e) => {
                let text = e.to_string();
                self.log_line(Level::Error, &text);
                // `apply_selections` writes the index in several steps, so a
                // failure can leave it part-way: reload before reporting, or
                // the tree describes an index that is no longer there.
                self.reload();
                self.show_error(&text, AfterNotice::Nothing);
            }
        }
    }

    /// Offer `entries` in the selector on the tree's own terminal and stage
    /// what it keeps — the `loom add -p` half of `C`. `Ok(false)` if it was
    /// cancelled or kept nothing, which leaves the index as it was.
    fn select_and_stage(
        &mut self,
        entries: Vec<FileEntry>,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> Result<bool> {
        let picked = run_hunk_selector_nested(
            entries,
            TuiTheme::from_graph_theme(&self.graph_theme),
            "COMMIT",
            terminal,
        )?;
        let Some(picked) = picked else {
            return Ok(false);
        };
        self.apply_pick(picked)
    }

    /// Stage what the selector kept, the half of `C` that writes the index —
    /// split from the selector, which is the only part needing a terminal.
    /// `Ok(false)` when nothing was kept, which leaves the index as it was.
    fn apply_pick(&mut self, picked: Vec<FileEntry>) -> Result<bool> {
        if !picked
            .iter()
            .any(|file| file.hunks.iter().any(|hunk| hunk.selected))
        {
            return Ok(false);
        }
        // Into the entry the press opened, so both halves of `loom add -p`
        // report under it. Nothing under `apply_selections` may prompt: the
        // thread that would answer is the one sitting here.
        ui::install(self.request_tx.clone());
        let applied =
            staging::apply_selections(&self.snapshot.workdir, &picked, staging::LeftOut::Unstaged);
        ui::uninstall();
        // `C` then commits the index, so an untick unstages, as in `add -p`.
        applied.map(|_| true)
    }

    /// Redraw the tree with the placeholder at the current destination and
    /// put the cursor back on it.
    fn show_pending_commit(&mut self) {
        self.rebuild_rows(&PENDING_COMMIT_OID.to_string());
    }

    /// `↑`/`↓` while placing a commit: the next destination down or up the
    /// tree, stopping at the ends.
    fn move_commit_dest(&mut self, dir: isize) {
        let Mode::CommitTarget { dests, index, .. } = &mut self.mode else {
            return;
        };
        let next = index.saturating_add_signed(dir).min(dests.len() - 1);
        if next == *index {
            return;
        }
        *index = next;
        self.show_pending_commit();
    }

    fn confirm_commit_target(&mut self) -> Option<Action> {
        let Mode::CommitTarget {
            source,
            dests,
            index,
            origin,
            ..
        } = std::mem::replace(&mut self.mode, Mode::Normal)
        else {
            return None;
        };
        // The placeholder row stays while the command runs; a failure has no
        // new commit to land on, so go back to where `c` was pressed.
        self.fallback_cursor = Some(origin);
        Some(Action::Commit {
            source,
            dest: dests[index].clone(),
        })
    }

    fn cancel_commit_target(&mut self) {
        let Mode::CommitTarget { origin, .. } = std::mem::replace(&mut self.mode, Mode::Normal)
        else {
            return;
        };
        self.rebuild_rows(&origin);
        self.diff.reset();
        self.notice = Some("commit cancelled".to_string());
    }

    /// `f`: the selection (else the cursor row) is what folds; `↑`/`↓` then
    /// walk the rows it can fold into, each tagged with what the fold does
    /// there, and nothing runs until one is confirmed.
    fn action_fold_start(&mut self) {
        let rows = self.picked_rows();
        let Some(origin) = self.current_row().map(|r| r.key.clone()) else {
            return;
        };
        let sources: Vec<String> = rows.iter().filter_map(|r| r.target.clone()).collect();
        if sources.is_empty() || sources.len() != rows.len() {
            self.notice = Some("fold: select source rows first".to_string());
            return;
        }
        let source_rows = self.mark_sources(&rows);
        let targets = match self.fold_targets(&rows, false) {
            Ok(targets) => targets,
            Err(notice) => {
                self.notice = Some(format!("fold: {}", notice));
                return;
            }
        };
        self.enter_fold_target(sources, source_rows, targets, origin, None);
    }

    fn enter_fold_target(
        &mut self,
        sources: Vec<String>,
        source_rows: Sources,
        targets: Vec<FoldTarget>,
        origin: String,
        hunks: Option<FoldHunks>,
    ) {
        let index = nearest_target(&self.rows, &targets, self.tree.cursor());
        self.mode = Mode::FoldTarget {
            sources,
            source_rows,
            targets,
            index,
            origin,
            hunks,
        };
        self.show_fold_target();
    }

    /// What an `F` pick leaves behind once the selector is done with it: the
    /// target walk, or a line in the entry the press opened saying why there
    /// is none.
    fn finish_fold_pick(
        &mut self,
        picked: Result<Option<Vec<FileEntry>>>,
        stamp: Stamp,
        sources: Vec<String>,
        targets: Vec<FoldTarget>,
        commit: Option<git2::Oid>,
        origin: String,
    ) {
        let picked = match picked {
            Ok(Some(picked)) if picked.iter().any(|f| f.hunks.iter().any(|h| h.selected)) => picked,
            Ok(_) => {
                self.log_line(Level::Warn, "Cancelled");
                self.notice = Some("fold cancelled".to_string());
                return;
            }
            Err(e) => {
                let text = e.to_string();
                self.log_line(Level::Error, &text);
                self.show_error(&text, AfterNotice::Nothing);
                return;
            }
        };
        let source_rows = self.hunk_sources(commit, &picked);
        let hunks = match commit {
            Some(_) => FoldHunks::Commit(picked),
            None => FoldHunks::Worktree { picked, stamp },
        };
        self.enter_fold_target(sources, source_rows, targets, origin, Some(hunks));
    }

    /// The gutter marks for picked hunks: the files they come from — the
    /// working files, or the source commit with its files covered.
    fn hunk_sources(&self, commit: Option<git2::Oid>, picked: &[FileEntry]) -> Sources {
        let paths: HashSet<&str> = picked
            .iter()
            .filter(|f| f.hunks.iter().any(|h| h.selected))
            .map(|f| f.path.as_str())
            .collect();
        let Some(oid) = commit else {
            return Sources {
                named: paths.iter().map(|p| working_file_key(p)).collect(),
                covered: HashSet::new(),
            };
        };
        let files = self
            .snapshot
            .info
            .commits
            .iter()
            .find(|c| c.oid == oid)
            .map_or(&[][..], |c| &c.files);
        Sources {
            named: HashSet::from([oid.to_string()]),
            covered: files
                .iter()
                .enumerate()
                .filter(|(_, f)| paths.contains(f.path.as_str()))
                .map(|(i, _)| commit_file_key(oid, i))
                .collect(),
        }
    }

    /// The rows `rows` can fold into, in tree order, as `loom fold` accepts
    /// them (Spec 007); the error is the notice for sources with none. Moving
    /// commits to a branch is not a fold here: the tree keeps fold to one
    /// thing going into another.
    ///
    /// With `hunks`, a commit's picked hunks move out of it rather than the
    /// whole commit folding: `[MOVE]` where `f` would say `[AMEND]`.
    fn fold_targets(&self, rows: &[&Row], hunks: bool) -> Result<Vec<FoldTarget>, &'static str> {
        let target = |row: &Row, effect| {
            row.target.clone().map(|arg| FoldTarget {
                key: row.key.clone(),
                arg,
                effect,
            })
        };
        let targets: Vec<FoldTarget> = match rows[0].kind {
            RowKind::LocalChanges { count: 0 } => return Err("no local changes"),
            RowKind::LocalChanges { .. } | RowKind::WorkingFile { .. } => self
                .rows
                .iter()
                .filter(|r| matches!(r.kind, RowKind::Commit { .. }))
                .filter_map(|r| target(r, FoldEffect::Amend))
                .collect(),
            RowKind::Commit { oid: source, .. } => {
                if rows.len() > 1 {
                    return Err("one commit at a time");
                }
                // Opened once for the fixup check; a repo that cannot answer
                // leaves the check to the command.
                let repo = git2::Repository::open(&self.snapshot.workdir).ok();
                let older = |oid: git2::Oid| {
                    repo.as_ref()
                        .is_none_or(|repo| repo.graph_descendant_of(source, oid).unwrap_or(true))
                };
                self.rows
                    .iter()
                    .filter_map(|r| match r.kind {
                        RowKind::LocalChanges { .. } => target(r, FoldEffect::Uncommit),
                        RowKind::Commit { oid, .. } if oid == source => target(r, FoldEffect::Noop),
                        // A fixup goes into a commit the source descends from.
                        RowKind::Commit { oid, .. } if older(oid) => target(
                            r,
                            if hunks {
                                FoldEffect::MoveFile
                            } else {
                                FoldEffect::Amend
                            },
                        ),
                        _ => None,
                    })
                    .collect()
            }
            RowKind::CommitFile { oid: owner, .. } => {
                if rows.len() > 1 {
                    return Err("one commit file at a time");
                }
                self.rows
                    .iter()
                    .filter_map(|r| match r.kind {
                        RowKind::LocalChanges { .. } => target(r, FoldEffect::Uncommit),
                        RowKind::Commit { oid, .. } if oid == owner => target(r, FoldEffect::Noop),
                        RowKind::Commit { .. } => target(r, FoldEffect::MoveFile),
                        _ => None,
                    })
                    .collect()
            }
            RowKind::BranchName { .. } => return Err("a branch cannot be folded"),
            _ => return Err("move to a file, commit, or commit file"),
        };
        if targets.iter().all(|t| t.effect == FoldEffect::Noop) {
            return Err("nothing to fold into");
        }
        Ok(targets)
    }

    /// Redraw the tree for the current fold target and put the cursor on it.
    fn show_fold_target(&mut self) {
        let Mode::FoldTarget { targets, index, .. } = &self.mode else {
            return;
        };
        let key = targets[*index].key.clone();
        self.rebuild_rows(&key);
        self.diff.reset();
    }

    /// `↑`/`↓` while picking a fold target: the next target down or up the
    /// tree, stopping at the ends.
    fn move_fold_target(&mut self, dir: isize) {
        let Mode::FoldTarget { targets, index, .. } = &mut self.mode else {
            return;
        };
        let next = index.saturating_add_signed(dir).min(targets.len() - 1);
        if next == *index {
            return;
        }
        *index = next;
        self.show_fold_target();
    }

    fn confirm_fold_target(&mut self) -> Option<Action> {
        let Mode::FoldTarget { targets, index, .. } = &self.mode else {
            return None;
        };
        let target = targets[*index].clone();
        if target.effect == FoldEffect::Noop {
            self.notice = Some("fold: nothing to do here, pick another target".to_string());
            return None;
        }
        let Mode::FoldTarget { sources, hunks, .. } =
            std::mem::replace(&mut self.mode, Mode::Normal)
        else {
            return None;
        };
        Some(match hunks {
            None => Action::Fold {
                sources,
                target: target.arg,
                hunks: None,
            },
            Some(FoldHunks::Worktree { picked, stamp }) => Action::FoldHunks {
                sources,
                picked,
                stamp,
                target: target.arg,
            },
            Some(FoldHunks::Commit(picked)) => {
                // Numbered against the commit and where its hunks land, as
                // `fold -p` re-reads them; `zz` names no commit.
                let into = (target.effect != FoldEffect::Uncommit).then_some(target.arg.as_str());
                let from = hunk_select::fingerprint(&sources[0], into, &picked);
                let ids = hunk_select::picked_ids(&picked);
                Action::Fold {
                    sources,
                    target: target.arg,
                    hunks: Some(HunkArgs::new(ids, Some(from))),
                }
            }
        })
    }

    fn cancel_fold_target(&mut self) {
        let Mode::FoldTarget { origin, .. } = std::mem::replace(&mut self.mode, Mode::Normal)
        else {
            return;
        };
        self.rebuild_rows(&origin);
        self.diff.reset();
        self.notice = Some("fold cancelled".to_string());
    }

    /// The tag the pending fold puts on `row`, its target.
    fn row_tag(&self, row: &Row) -> Option<&'static str> {
        match &self.mode {
            Mode::FoldTarget { targets, index, .. } => {
                let target = &targets[*index];
                (row.key == target.key).then(|| target.effect.tag())
            }
            Mode::MoveTarget { commit, .. } => (row.key == commit.to_string()).then_some("[MOVE]"),
            _ => None,
        }
    }

    /// `m`: the selected commit (else the cursor's) is moved; `↑`/`↓` then
    /// walk it through every place it can go, the tree redrawn with it there,
    /// and nothing runs until one is confirmed.
    fn action_move_start(&mut self) {
        let rows = self.picked_rows();
        let commit = match rows.as_slice() {
            [row] => match row.kind {
                RowKind::Commit { oid, .. } if row.target.is_some() => oid,
                _ => {
                    self.notice = Some("move: move to a commit".to_string());
                    return;
                }
            },
            [] => return,
            _ => {
                self.notice = Some("move: one commit at a time".to_string());
                return;
            }
        };
        let slots = self.move_slots(commit);
        if slots.len() < 2 {
            self.notice = Some("move: nowhere else to put it".to_string());
            return;
        }
        let index = slots
            .iter()
            .position(|s| *s == MoveSlot::Stay)
            .expect("always offered");
        self.clear_selection();
        self.mode = Mode::MoveTarget {
            commit,
            slots,
            index,
        };
        self.show_move_target();
    }

    /// Every distinct place `commit` can be moved to, in tree order, `Stay`
    /// among them. The candidates are `--above`/`--below` every other commit
    /// and every branch's tip, ranked by the row they sit next to; candidates
    /// drawing the same tree are one place, spelled the first way.
    fn move_slots(&self, commit: git2::Oid) -> Vec<MoveSlot> {
        let info = &self.snapshot.info;
        let parent_of = |oid: git2::Oid| {
            info.commits
                .iter()
                .find(|c| c.oid == oid)
                .and_then(|c| c.parent_oid)
        };
        let parent = parent_of(commit);
        // Three ranks per row: above it, the row itself, below it. A branch
        // tip ranks with the row after its name, behind an `--above` there.
        let mut candidates: Vec<(usize, MoveSlot)> = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            match &row.kind {
                RowKind::Commit { oid, .. } if *oid == commit => {
                    candidates.push((3 * i + 1, MoveSlot::Stay));
                }
                RowKind::Commit { oid, .. } if row.target.is_some() => {
                    // `fold --above/--below` refuses the commit's own place,
                    // even where the move would carry a branch name along.
                    if parent != Some(*oid) {
                        candidates.push((3 * i, MoveSlot::Above(*oid)));
                    }
                    if parent_of(*oid) != Some(commit) {
                        candidates.push((3 * i + 2, MoveSlot::Below(*oid)));
                    }
                }
                RowKind::BranchName { name, .. }
                    if !info
                        .branches
                        .iter()
                        .any(|b| b.name == *name && b.tip_oid == commit) =>
                {
                    candidates.push((3 * i + 3, MoveSlot::Branch(name.clone())));
                }
                _ => {}
            }
        }
        candidates.sort_by_key(|(rank, slot)| (*rank, matches!(slot, MoveSlot::Branch(_))));

        let mut seen = HashSet::from([self.move_signature(commit, &MoveSlot::Stay)]);
        candidates
            .into_iter()
            .map(|(_, slot)| slot)
            .filter(|slot| {
                *slot == MoveSlot::Stay || seen.insert(self.move_signature(commit, slot))
            })
            .collect()
    }

    /// The tree `commit` at `slot` draws, as row keys and connectors: two
    /// slots with the same one are the same move as far as the tree shows.
    fn move_signature(&self, commit: git2::Oid, slot: &MoveSlot) -> Vec<String> {
        let sections = self.snapshot.sections(Some(Preview::Move {
            oid: commit,
            slot: slot.clone(),
        }));
        status_tree::build_rows(&sections, &self.snapshot.ids, &self.expanded)
            .into_iter()
            .map(|r| match r.kind {
                RowKind::Spacer(text) => text.to_string(),
                _ => r.key,
            })
            .collect()
    }

    /// Redraw the tree with the commit at the current slot and put the cursor
    /// on it.
    fn show_move_target(&mut self) {
        let Mode::MoveTarget { commit, .. } = &self.mode else {
            return;
        };
        let key = commit.to_string();
        self.rebuild_rows(&key);
        self.diff.reset();
    }

    /// `↑`/`↓` while moving a commit: the next place up or down the tree,
    /// stopping at the ends.
    fn move_move_target(&mut self, dir: isize) {
        let Mode::MoveTarget { slots, index, .. } = &mut self.mode else {
            return;
        };
        let next = index.saturating_add_signed(dir).min(slots.len() - 1);
        if next == *index {
            return;
        }
        *index = next;
        self.show_move_target();
    }

    fn confirm_move_target(&mut self) -> Option<Action> {
        let Mode::MoveTarget { slots, index, .. } = &self.mode else {
            return None;
        };
        if slots[*index] == MoveSlot::Stay {
            self.notice = Some("move: it is already here, pick another place".to_string());
            return None;
        }
        let Mode::MoveTarget {
            commit,
            mut slots,
            index,
        } = std::mem::replace(&mut self.mode, Mode::Normal)
        else {
            return None;
        };
        // The rewrite renames the commit, so there is no key to follow: the
        // cursor stays on the row the preview drew it at.
        Some(Action::Move {
            commit,
            slot: slots.swap_remove(index),
        })
    }

    fn cancel_move_target(&mut self) {
        let Mode::MoveTarget { commit, .. } = std::mem::replace(&mut self.mode, Mode::Normal)
        else {
            return;
        };
        self.rebuild_rows(&commit.to_string());
        self.diff.reset();
        self.notice = Some("move cancelled".to_string());
    }

    /// `b`: start naming a new branch, drawn in the tree as if it already
    /// existed at the cursor commit or branch tip (its `-t` target), else at
    /// the base. Nothing runs until the name is confirmed.
    fn action_new_branch(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        let origin = row.key.clone();
        let info = &self.snapshot.info;
        let (target, tip) = match &row.kind {
            RowKind::Commit { oid, .. } => (row.target.clone(), *oid),
            RowKind::BranchName { name, .. } => {
                let tip = info
                    .branches
                    .iter()
                    .find(|b| b.name == *name)
                    .map_or(info.upstream.merge_base_oid, |b| b.tip_oid);
                (row.target.clone(), tip)
            }
            _ => (None, info.upstream.merge_base_oid),
        };
        self.mode = Mode::NewBranch {
            target,
            tip,
            origin,
            field: TextField::new(""),
        };
        self.rebuild_rows(&branch_key(NEW_BRANCH_NAME));
        self.diff.reset();
    }

    /// Whether a branch name is being typed over a row in the tree.
    fn editing_name(&self) -> bool {
        matches!(
            self.mode,
            Mode::RenameBranch { .. } | Mode::NewBranch { .. }
        )
    }

    /// Route a key to whichever branch-name field is open.
    fn handle_name_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        match self.mode {
            Mode::RenameBranch { .. } => self.handle_rename_key(code, modifiers),
            Mode::NewBranch { .. } => self.handle_new_branch_key(code, modifiers),
            _ => None,
        }
    }

    /// A key while a new branch's name is being typed on its placeholder row.
    /// Enter creates it; Esc, Ctrl-C, or an empty name drop the row again.
    fn handle_new_branch_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        let Mode::NewBranch {
            target,
            origin,
            field,
            ..
        } = &mut self.mode
        else {
            return None;
        };
        let cancelled = code == KeyCode::Esc
            || (code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL));
        if !cancelled && code != KeyCode::Enter {
            field.handle_key(code, modifiers);
            return None;
        }
        let name = field.value().trim().to_string();
        if cancelled || name.is_empty() {
            let origin = origin.clone();
            self.mode = Mode::Normal;
            self.rebuild_rows(&origin);
            self.diff.reset();
            self.notice = Some("branch cancelled".to_string());
            return None;
        }
        let target = target.clone();
        self.fallback_cursor = Some(origin.clone());
        self.mode = Mode::Normal;
        // Keep the row readable while the command runs, then follow the
        // branch to its real row once the tree reloads.
        let key = branch_key(NEW_BRANCH_NAME);
        if let Some(row) = self.rows.iter_mut().find(|r| r.key == key)
            && let RowKind::BranchName { name: shown, .. } = &mut row.kind
        {
            *shown = name.clone();
        }
        self.next_cursor = Some(branch_key(&name));
        Some(Action::NewBranch { name, target })
    }

    /// The one commit `s` or `S` splits: the commit the rows are, or the one
    /// the commit files among them belong to.
    fn split_source(&self, rows: &[&Row]) -> Result<git2::Oid, &'static str> {
        let mut commits = HashSet::new();
        for row in rows {
            match row.kind {
                RowKind::Commit { oid, .. } | RowKind::CommitFile { oid, .. }
                    if row.target.is_some() =>
                {
                    commits.insert(oid);
                }
                _ => return Err("move to a commit or commit file"),
            }
        }
        match commits.into_iter().collect::<Vec<_>>()[..] {
            [commit] => Ok(commit),
            [] => Err("move to a commit or commit file"),
            _ => Err("one commit at a time"),
        }
    }

    /// `s`: the selected commit files (else the cursor's) become the first
    /// commit, the rest of their commit the second; a commit row is `S`.
    fn action_split(&mut self) -> Option<Action> {
        let rows = self.picked_rows();
        let commit = match self.split_source(&rows) {
            Ok(commit) => commit,
            Err(notice) => {
                self.notice = Some(format!("split: {}", notice));
                return None;
            }
        };
        let files: Vec<String> = rows
            .iter()
            .filter_map(|r| match &r.kind {
                RowKind::CommitFile { path, .. } => {
                    Some(repo::cwd_relative_path(path, &self.snapshot.cwd_prefix))
                }
                _ => None,
            })
            .collect();
        // A commit row names no files to take out: pick its hunks, as `S` does.
        if files.is_empty() {
            self.action_split_patch_start();
            return None;
        }
        let total = self
            .snapshot
            .info
            .commits
            .iter()
            .find(|c| c.oid == commit)
            .map_or(0, |c| c.files.len());
        if total < 2 {
            self.notice = Some("split: one file only, split its hunks with S".to_string());
            return None;
        }
        if files.len() >= total {
            self.notice = Some("split: leave at least one file for the second commit".to_string());
            return None;
        }
        Some(Action::Split {
            commit: commit.to_string(),
            files,
            hunks: None,
        })
    }

    /// `S`: `s` with the hunks picked first, over the commit's whole diff
    /// whatever the cursor or selection, as `C` shows every local change. The
    /// press starts the read on a worker, as `C` does.
    fn action_split_patch_start(&mut self) {
        let rows = self.picked_rows();
        let Some(origin) = self.current_row().map(|r| r.key.clone()) else {
            return;
        };
        let commit = match self.split_source(&rows) {
            Ok(commit) => commit,
            Err(notice) => {
                self.notice = Some(format!("split: {}", notice));
                return;
            }
        };
        let command = format!("loom split -p {}", self.sid_of(&commit.to_string()));
        self.start_pick(command, origin, PickFor::Split { commit }, move |workdir| {
            let entries = staging::collect_commit_hunks(workdir, &commit.to_string(), &[])?;
            Ok((entries, Vec::new()))
        });
    }

    /// Whether an `S` read leaves nothing to split, after saying so: with fewer
    /// than two entries no pick can keep something on each side.
    fn refuse_split_pick(&mut self, entries: &[FileEntry]) -> bool {
        let (line, notice) = match entries.iter().map(|f| f.hunks.len()).sum::<usize>() {
            0 => ("No changes to pick", "split: no hunks to pick"),
            1 => ("Only one hunk", "split: one hunk only, nothing to split"),
            _ => return false,
        };
        self.log_line(Level::Warn, line);
        self.notice = Some(notice.to_string());
        true
    }

    /// The split an `S` pick confirms, handed to `loom split -p` by id (Spec
    /// 019), or `None` after a line in the entry the press opened saying why
    /// there is none. Both halves must keep something, as `split -p` checks.
    fn finish_split_pick(
        &mut self,
        picked: Result<Option<Vec<FileEntry>>>,
        commit: git2::Oid,
    ) -> Option<Action> {
        let picked = match picked {
            Ok(Some(picked)) if picked.iter().any(|f| f.hunks.iter().any(|h| h.selected)) => picked,
            Ok(_) => {
                self.log_line(Level::Warn, "Cancelled");
                self.notice = Some("split cancelled".to_string());
                return None;
            }
            Err(e) => {
                let text = e.to_string();
                self.log_line(Level::Error, &text);
                self.show_error(&text, AfterNotice::Nothing);
                return None;
            }
        };
        if picked.iter().all(|f| f.hunks.iter().all(|h| h.selected)) {
            self.log_line(Level::Warn, "Nothing left for the second commit");
            self.notice = Some("split: leave at least one hunk for the second commit".to_string());
            return None;
        }
        let commit = commit.to_string();
        let from = hunk_select::fingerprint(&commit, None, &picked);
        let ids = hunk_select::picked_ids(&picked);
        Some(Action::Split {
            commit,
            files: Vec::new(),
            hunks: Some(HunkArgs::new(ids, Some(from))),
        })
    }

    /// `d`: drop the selected working files together, else the cursor row (a
    /// commit, branch, working file, or the `[local changes]` header for
    /// `drop zz`). Only files can go together, as in the CLI.
    fn action_drop(&mut self) -> Option<Action> {
        let rows = self.picked_rows();
        let droppable = |row: &Row| {
            matches!(
                row.kind,
                RowKind::Commit { .. }
                    | RowKind::BranchName { .. }
                    | RowKind::WorkingFile { .. }
                    | RowKind::LocalChanges { .. }
            ) && row.target.is_some()
        };
        if rows.is_empty() || !rows.iter().all(|r| droppable(r)) {
            self.notice =
                Some("drop: move to a commit, branch, file, or local changes".to_string());
            return None;
        }
        if rows.len() > 1
            && !rows
                .iter()
                .all(|r| matches!(r.kind, RowKind::WorkingFile { .. }))
        {
            self.notice = Some("drop: only files can be dropped together".to_string());
            return None;
        }
        let targets = rows.iter().filter_map(|r| r.target.clone()).collect();
        Some(Action::Drop { targets })
    }

    /// `a`: absorb the selected working files, else the cursor's, else every
    /// local change. The command itself shows its plan to confirm.
    fn action_absorb(&mut self) -> Option<Action> {
        if self.snapshot.info.working_changes.is_empty() {
            self.notice = Some("absorb: no local changes".to_string());
            return None;
        }
        let rows = self.picked_rows();
        let is_file = |r: &&Row| matches!(r.kind, RowKind::WorkingFile { .. });
        if !self.selected.is_empty()
            && !rows
                .iter()
                .all(|r| is_file(r) || matches!(r.kind, RowKind::LocalChanges { .. }))
        {
            self.notice = Some("absorb: select files or local changes".to_string());
            return None;
        }
        let files = if rows.iter().all(is_file) {
            rows.iter().filter_map(|r| r.target.clone()).collect()
        } else {
            Vec::new()
        };
        Some(Action::Absorb { files })
    }

    /// `r`: reword the commit under the cursor, or start editing the branch
    /// name in place.
    fn action_reword(&mut self) -> Option<Action> {
        let row = self.current_row()?;
        let target = row.target.clone();
        match (&row.kind, target) {
            (RowKind::Commit { .. }, Some(target)) => Some(Action::Reword { target, name: None }),
            (RowKind::BranchName { name, .. }, Some(target)) => {
                let field = TextField::new(name);
                self.mode = Mode::RenameBranch {
                    branch: target,
                    field,
                };
                None
            }
            _ => {
                self.notice = Some("reword: move to a commit or branch".to_string());
                None
            }
        }
    }

    /// A key while the branch name is being edited in the tree. Enter runs
    /// the rename, Esc cancels, everything else edits the field.
    fn handle_rename_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        let Mode::RenameBranch { branch, field } = &mut self.mode else {
            return None;
        };
        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            self.mode = Mode::Normal;
            self.notice = Some("rename cancelled".to_string());
            return None;
        }
        match code {
            KeyCode::Enter => {
                let name = field.value().trim().to_string();
                let branch = branch.clone();
                if name.is_empty() {
                    self.notice = Some("rename: the name cannot be empty".to_string());
                    return None;
                }
                self.mode = Mode::Normal;
                if name == branch {
                    self.notice = Some("rename: name unchanged".to_string());
                    return None;
                }
                // Follow the branch to its new row rather than reloading to
                // the top of the tree.
                self.next_cursor = Some(branch_key(&name));
                Some(Action::Reword {
                    target: branch,
                    name: Some(name),
                })
            }
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.notice = Some("rename cancelled".to_string());
                None
            }
            _ => {
                field.handle_key(code, modifiers);
                None
            }
        }
    }

    // -- rendering ----------------------------------------------------------------

    fn render_tree(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let cursor = self.tree.cursor();
        let editing = match &self.mode {
            Mode::RenameBranch { branch, field } => Some((branch_key(branch), field)),
            Mode::NewBranch { field, .. } => Some((branch_key(NEW_BRANCH_NAME), field)),
            _ => None,
        };
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                ListItem::new(row_line(
                    row,
                    self.theme,
                    &self.snapshot.cwd_prefix,
                    self.row_mark(row),
                    self.row_tag(row),
                    i == cursor,
                    editing
                        .as_ref()
                        .filter(|(key, _)| *key == row.key)
                        .map(|(_, f)| *f),
                ))
            })
            .collect();

        let title = match &self.mode {
            Mode::Normal => " Status ".to_string(),
            Mode::FoldTarget { sources, hunks, .. } => {
                let of = if hunks.is_some() { "hunks of " } else { "" };
                self.title_sources(sources, area.width, |what| {
                    format!(" Fold {}{} into... ", of, what)
                })
            }
            Mode::CommitTarget {
                source,
                dests,
                index,
                ..
            } => {
                let dest = match &dests[*index] {
                    CommitDest::Integration => &self.snapshot.info.branch_name,
                    CommitDest::Branch(name) => name,
                };
                let title = |what: &str| format!(" Commit {} → [{}] ", what, dest);
                match source {
                    CommitSource::Files(files) => self.title_sources(files, area.width, title),
                    CommitSource::Index => title("the index"),
                }
            }
            Mode::MoveTarget { commit, .. } => {
                format!(" Move {} ", self.sid_of(&commit.to_string()))
            }
            Mode::RenameBranch { .. } => " Rename branch ".to_string(),
            Mode::NewBranch { .. } => " New branch ".to_string(),
        };
        let block = pane_block(&title, self.theme, focused);
        self.tree
            .render(frame, area, items, block, self.theme.file_selected);
    }

    fn render_diff(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let key = self.ensure_diff_cached();
        let lines = borrowed_lines(&self.diff_cache[&key]);
        let block = pane_block(" Diff ", self.theme, focused);
        self.diff.render(frame, area, lines, block);
    }

    /// Compute and cache the diff for the cursor row (a git call on first
    /// visit); returns the cache key. Called from the event path so rendering
    /// never shells out.
    fn ensure_diff_cached(&mut self) -> String {
        let key = match self.rows.get(self.tree.cursor()) {
            Some(row) => row.key.clone(),
            None => String::new(),
        };
        if !self.diff_cache.contains_key(&key) {
            let lines = match self.rows.get(self.tree.cursor()) {
                Some(row) => {
                    let text = match &self.mode {
                        Mode::CommitTarget { source, .. }
                            if row.key == PENDING_COMMIT_OID.to_string() =>
                        {
                            match source {
                                CommitSource::Files(files) => {
                                    pending_commit_diff(&self.snapshot, files)
                                }
                                CommitSource::Index => staged_commit_diff(&self.snapshot),
                            }
                        }
                        _ => diff_text(&self.snapshot, row),
                    };
                    colorize_diff(&text, self.theme)
                }
                None => vec![Line::from("")],
            };
            self.diff_cache.insert(key.clone(), lines);
        }
        key
    }
}

/// Borrow cached lines for rendering without copying their strings.
fn borrowed_lines<'a>(lines: &'a [Line<'static>]) -> Vec<Line<'a>> {
    lines
        .iter()
        .map(|line| {
            Line::from(
                line.spans
                    .iter()
                    .map(|s| Span::styled(s.content.as_ref(), s.style))
                    .collect::<Vec<Span<'a>>>(),
            )
            .style(line.style)
        })
        .collect()
}

// ── Shell integration ────────────────────────────────────────────────────

impl ShellApp for App<'_> {
    type Exit = Outcome;

    fn config(&self) -> ShellConfig {
        ShellConfig { split: (45, 55) }
    }

    fn theme(&self) -> &TuiTheme {
        self.theme
    }

    fn quit_exit(&mut self) -> Outcome {
        Outcome::Quit
    }

    fn handle_key(
        &mut self,
        focused: PaneId,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> KeyResult<Outcome> {
        if self.popup.is_some() {
            self.handle_popup_key(code, modifiers);
        } else if self.editing_name() {
            if let Some(action) = self.handle_name_key(code, modifiers) {
                self.start_action(action);
            }
        } else if self.running.is_some() {
            // The log is read-only; everything else waits for the action.
            match code {
                KeyCode::Char('L') => self.open_log(),
                _ => self.notice = Some("an action is running…".to_string()),
            }
        } else if self.pick.is_some() {
            // The selector is about to own the screen, so nothing may start
            // here — including a second `C`. Only a read still running reaches
            // this: one that is done has taken the terminal, since the loop
            // polls before it reads a key. Covering the whole pick regardless
            // keeps that reasoning off the list of things to know.
            match code {
                KeyCode::Char('L') => self.open_log(),
                // Marked, not dropped: the worker reads with `git`, so
                // letting it outlive the pick would put it beside whatever
                // action the freed keyboard starts next, over the same index.
                // The result is thrown away when it lands instead.
                KeyCode::Esc => self.cancel_pick(false),
                // Queued like the cancel above rather than answered now: the
                // worker is what the pick waits for, and leaving on top of it
                // is what the waiting exists to prevent.
                KeyCode::Char('q') => self.cancel_pick(true),
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.cancel_pick(true)
                }
                // Deliberately silent: a notice outranks the mode hint, so
                // saying "still reading" here would replace the spinner that
                // says it, and stay there until the next key.
                _ => {}
            }
        } else {
            self.handle_tree_key(focused, code);
        }
        // Deep handlers (Esc, notice dismissal) report exits through the
        // outcome field.
        match self.outcome.take() {
            Some(outcome) => KeyResult::Exit(outcome),
            None => {
                self.ensure_diff_cached();
                KeyResult::Handled
            }
        }
    }

    fn handle_mouse(&mut self, pane: PaneId, kind: MouseEventKind, pos: Position, area: Rect) {
        match pane {
            PaneId::Left => match kind {
                // A click while a commit is being placed would take the cursor
                // off the placeholder, and one picking a fold target may only
                // land on a target. The wheel is safe: it goes through
                // `move_cursor`, which moves the destination in those modes.
                MouseEventKind::Down(MouseButton::Left)
                    if !matches!(
                        self.mode,
                        Mode::CommitTarget { .. } | Mode::MoveTarget { .. }
                    ) =>
                {
                    let Some(clicked) = self.tree.hit_test(area, pos.y) else {
                        return;
                    };
                    let Some(key) = self
                        .rows
                        .get(clicked)
                        .filter(|r| r.focusable)
                        .map(|r| r.key.clone())
                    else {
                        return;
                    };
                    if let Mode::FoldTarget { targets, index, .. } = &mut self.mode {
                        if let Some(i) = targets.iter().position(|t| t.key == key) {
                            *index = i;
                            self.show_fold_target();
                        }
                    } else {
                        self.tree.set_cursor(clicked);
                        self.diff.reset();
                    }
                }
                MouseEventKind::ScrollUp => self.move_cursor(-1),
                MouseEventKind::ScrollDown => self.move_cursor(1),
                _ => {}
            },
            PaneId::Right => match kind {
                MouseEventKind::ScrollUp => self.diff.scroll_by(-3),
                MouseEventKind::ScrollDown => self.diff.scroll_by(3),
                _ => {}
            },
        }
        self.ensure_diff_cached();
    }

    fn render_pane(&mut self, frame: &mut Frame, pane: PaneId, area: Rect, focused: bool) {
        match pane {
            PaneId::Left => self.render_tree(frame, area, focused),
            PaneId::Right => self.render_diff(frame, area, focused),
        }
    }

    fn render_overlay(&mut self, frame: &mut Frame, area: Rect) {
        match &mut self.popup {
            Some(Popup::Prompt { prompt, .. }) => prompt.render(frame, area, self.theme),
            Some(Popup::Notice { notice, .. }) => notice.render(frame, area, self.theme),
            Some(Popup::Log { scroll }) => {
                popup::render_log(frame, area, &self.log, scroll, self.theme)
            }
            None => {}
        }
    }

    fn modal_active(&self) -> bool {
        // A name field owns the keyboard too: `q` and Tab must reach it.
        self.popup.is_some() || self.running.is_some() || self.editing_name() || self.pick.is_some()
    }

    fn poll_background(&mut self) -> Tick<Outcome> {
        let mut changed = false;
        loop {
            match self.requests.try_recv() {
                Ok(Request::Suspend(ack)) => return Tick::Suspend(ack),
                Ok(request) => {
                    self.handle_request(request);
                    changed = true;
                }
                Err(_) => break,
            }
        }
        // After the drain, never before: what the pick's own worker reported
        // reaches its log entry before the selector covers the screen. A line
        // the action that just ended left queued lands in that entry too —
        // accepted, since it is misfiled rather than lost. No action can still
        // be running — `C` is a tree key — which the early return counts on: it
        // skips the action's own tick and join.
        match self.advance_pick() {
            // The shell hands the live terminal over without tearing it down,
            // so the selector draws straight onto this frame — which must be
            // nobody else's. A popup owns the screen and the keyboard, and
            // would still own them once the selector returned.
            PickTick::Ready { .. } if self.popup.is_none() => return Tick::TakeOver,
            // Held under a popup: the frame still carries the spinner of a read
            // that has finished — offering an `Esc` the popup now takes. One
            // draw clears it; there is nothing to animate after that.
            PickTick::Ready { first } => changed |= first,
            PickTick::Redraw => changed = true,
            PickTick::Nothing => {}
        }
        if let Some(running) = &mut self.running {
            running.ticks += 1;
            if running.handle.is_finished() {
                let running = self.running.take().expect("checked above");
                let result = running.handle.join().unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "the action crashed — run `loom trace` for what it did"
                    ))
                });
                if self.finish_action(result) {
                    self.after_action();
                }
            }
            changed = true;
        }
        if let Some(outcome) = self.outcome.take() {
            return Tick::Exit(outcome);
        }
        if changed { Tick::Redraw } else { Tick::Idle }
    }

    /// The hunk selector for `C` or `F`, drawn on the tree's own terminal.
    /// What `C` keeps is staged at once, as `loom add -p` would; the commit
    /// that follows takes the index, so the tree reloads first to show it.
    /// What `F` keeps waits for its target, staging nothing.
    fn take_over(&mut self, terminal: &mut ratatui::DefaultTerminal) {
        // Only a read that is done is this to consume. Nothing else asks for
        // the terminal, so the other arm is a belt: it puts back rather than
        // drops, since dropping a running read would strand its worker.
        let (entries, stamp, origin, purpose) = match self.pick.take() {
            Some(Pick::Ready {
                entries,
                stamp,
                origin,
                purpose,
            }) => (entries, stamp, origin, purpose),
            other => {
                self.pick = other;
                return;
            }
        };
        match purpose {
            PickFor::Commit => {
                let staged = self.select_and_stage(entries, terminal);
                self.finish_pick(staged, origin);
            }
            PickFor::Split { commit } => {
                if self.refuse_split_pick(&entries) {
                    return;
                }
                let picked = run_hunk_selector_nested(
                    entries,
                    TuiTheme::from_graph_theme(&self.graph_theme),
                    "SPLIT",
                    terminal,
                );
                if let Some(action) = self.finish_split_pick(picked, commit) {
                    self.start_action(action);
                }
            }
            PickFor::Fold {
                sources,
                targets,
                commit,
            } => {
                if entries.is_empty() {
                    self.log_line(Level::Warn, "No changes to pick");
                    self.notice = Some("fold: no hunks to pick".to_string());
                    return;
                }
                let picked = run_hunk_selector_nested(
                    entries,
                    TuiTheme::from_graph_theme(&self.graph_theme),
                    "FOLD",
                    terminal,
                );
                self.finish_fold_pick(picked, stamp, sources, targets, commit, origin);
            }
        }
    }

    /// The worker holds the terminal (an editor); its `Resume` ends the wait.
    /// A worker that dies without one ends it too.
    fn wait_for_resume(&mut self) {
        loop {
            match self.requests.recv_timeout(Duration::from_millis(100)) {
                Ok(Request::Resume) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return;
                }
                Ok(request) => self.handle_request(request),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if self.running.as_ref().is_none_or(|r| r.handle.is_finished()) {
                        return;
                    }
                }
            }
        }
    }

    fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    fn clear_notice(&mut self) {
        self.notice = None;
    }

    fn mode_hint(&self) -> Option<String> {
        if let Some(running) = &self.running {
            let frame = SPINNER_FRAMES[(running.ticks / 2) % SPINNER_FRAMES.len()];
            let detail = running
                .spinner
                .as_deref()
                .map(|s| format!(" — {}", s))
                .unwrap_or_default();
            return Some(format!(" {} {}{}", frame, running.command, detail));
        }
        if let Some(Pick::Collecting {
            ticks,
            cancelled,
            purpose,
            ..
        }) = &self.pick
        {
            let frame = SPINNER_FRAMES[(ticks / 2) % SPINNER_FRAMES.len()];
            let what = match purpose {
                _ if *cancelled => "cancelling: waiting for the read to end",
                PickFor::Fold {
                    commit: Some(_), ..
                }
                | PickFor::Split { .. } => "reading the commit — Esc to cancel",
                _ => "reading the working tree — Esc to cancel",
            };
            return Some(format!(" {} {}: {}", frame, purpose.label(), what));
        }
        match &self.mode {
            Mode::FoldTarget { .. } => {
                Some(" fold: ↑/↓ choose the target, Enter to fold, Esc to cancel".to_string())
            }
            Mode::CommitTarget { .. } => Some(
                " commit: ↑/↓ choose the destination, Enter to commit, Esc to cancel".to_string(),
            ),
            Mode::MoveTarget { .. } => {
                Some(" move: ↑/↓ choose the place, Enter to move, Esc to cancel".to_string())
            }
            Mode::RenameBranch { .. } => Some(
                " rename: type the new branch name, Enter to confirm, Esc to cancel".to_string(),
            ),
            Mode::NewBranch { .. } => Some(
                " branch: type the new branch name, Enter to create, Esc to cancel".to_string(),
            ),
            Mode::Normal => None,
        }
    }

    fn status_hints(&self, _focused: PaneId) -> Vec<Cow<'static, str>> {
        vec![
            "Navigate: ↑/↓".into(),
            "Close/open: ←/→".into(),
            "Select: space".into(),
            "Commit: c/C".into(),
            "Fold: f/F".into(),
            "Move: m".into(),
            "Split: s/S".into(),
            "Branch: b".into(),
            "Drop: d".into(),
            "Absorb: a".into(),
            "Reword: r".into(),
            "Log: L".into(),
            "Refresh: R".into(),
            "Quit: q".into(),
        ]
    }
}

/// The focusable row at or above `index`, else the first focusable one.
fn nearest_focusable(rows: &[Row], index: usize) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let start = index.min(rows.len() - 1);
    rows[..=start]
        .iter()
        .rposition(|r| r.focusable)
        .or_else(|| rows.iter().position(|r| r.focusable))
}

/// Index of the target a fold starts on: the nearest `[NOOP]` one, so commit
/// sources start on themselves and a commit file on its commit, else the one
/// nearest `cursor`, the lower on a tie.
fn nearest_target(rows: &[Row], targets: &[FoldTarget], cursor: usize) -> usize {
    (0..targets.len())
        .min_by_key(|&i| {
            let at = rows
                .iter()
                .position(|r| r.key == targets[i].key)
                .unwrap_or(usize::MAX);
            (
                targets[i].effect != FoldEffect::Noop,
                at.abs_diff(cursor),
                at < cursor,
            )
        })
        .unwrap_or(0)
}

// ── Row rendering ────────────────────────────────────────────────────────

/// Render one tree row as a styled line; `editing` replaces the branch name
/// with the field being typed. The first span is the selection/source gutter.
fn row_line(
    row: &Row,
    theme: &TuiTheme,
    cwd_prefix: &str,
    mark: RowMark,
    tag: Option<&'static str>,
    is_cursor: bool,
    editing: Option<&TextField>,
) -> Line<'static> {
    // On the cursor row the selection background swallows regular dim text.
    let dim = if is_cursor {
        theme.dim_selected
    } else {
        theme.dim
    };
    let mut spans: Vec<Span<'static>> = vec![match mark {
        RowMark::Selected => Span::styled("✓ ", theme.selection),
        RowMark::Source => Span::styled("▸ ", theme.source),
        // Covered, not named: the same glyph, without the source's weight.
        RowMark::Covered => Span::styled("▸ ", dim),
        RowMark::None => Span::raw("  "),
    }];

    let display = |path: &str| crate::core::repo::cwd_relative_path(path, cwd_prefix);

    match &row.kind {
        RowKind::LocalChanges { count } => {
            spans.push(Span::styled("╭─ ", theme.graph));
            spans.push(Span::styled(row.sid.clone(), theme.shortid));
            if let Some(tag) = tag {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(tag, theme.pending_tag));
            }
            spans.push(Span::styled(" [", dim));
            spans.push(Span::styled("local changes", theme.label));
            spans.push(Span::styled("]", dim));
            if *count == 0 {
                spans.push(Span::styled(" no changes", dim));
            } else if !row.expanded {
                spans.push(Span::styled(format!(" ({} files)", count), dim));
            }
        }
        RowKind::WorkingFile {
            path,
            index,
            worktree,
        } => {
            spans.push(Span::styled("│   ", theme.graph));
            spans.push(Span::styled(row.sid.clone(), theme.shortid));
            let group = graph::file_group(*index, *worktree);
            if group == graph::FileGroup::Untracked {
                spans.push(Span::styled(" ⁕ ", theme.untracked));
            } else if group == graph::FileGroup::Conflicted {
                spans.push(Span::styled(
                    " !! ",
                    theme.unstaged_status.add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(index.to_string(), theme.staged_status));
                spans.push(Span::styled(worktree.to_string(), theme.unstaged_status));
                spans.push(Span::raw(" "));
            }
            spans.push(Span::raw(display(path)));
        }
        RowKind::BranchName {
            name,
            remote,
            connector,
            ..
        } => {
            spans.push(Span::styled(format!("{} ", connector), theme.graph));
            if row.key == branch_key(NEW_BRANCH_NAME) {
                // A branch being created has no short ID; say what the row is
                // instead of faking the column.
                spans.push(Span::styled("[CREATE BRANCH]", theme.pending_tag));
            } else {
                spans.push(Span::styled(row.sid.clone(), theme.shortid));
            }
            spans.push(Span::styled(" [", dim));
            match editing {
                Some(field) => spans.extend(field.spans(theme)),
                None => spans.push(Span::styled(name.clone(), theme.branch)),
            }
            spans.push(Span::styled("]", dim));
            match remote {
                Some(RemoteStatus::Synced) => spans.push(Span::styled(" ✓", theme.remote_synced)),
                Some(RemoteStatus::Different) => spans.push(Span::styled(" ↑", theme.remote_ahead)),
                Some(RemoteStatus::Gone) => spans.push(Span::styled(" ✗", theme.remote_gone)),
                None => {}
            }
        }
        RowKind::Commit {
            oid,
            message,
            dot_color,
            file_count,
        } => {
            match dot_color {
                Some(idx) => {
                    spans.push(Span::styled("│", theme.graph));
                    spans.push(Span::styled(
                        "●",
                        theme.branch_dots[idx % theme.branch_dots.len()],
                    ));
                    spans.push(Span::raw("  "));
                }
                None => {
                    spans.push(Span::styled("●", theme.graph));
                    spans.push(Span::raw("   "));
                }
            }
            if *oid == PENDING_COMMIT_OID {
                // A commit being placed has no short ID; say what the row is
                // instead of faking the column.
                spans.push(Span::styled("[CREATE COMMIT]", theme.pending_tag));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(message.clone(), dim));
                return Line::from(spans);
            }
            spans.push(Span::styled(row.sid.clone(), theme.shortid));
            spans.push(Span::raw(graph::id_pad(&row.sid)));
            if let Some(tag) = tag {
                spans.push(Span::styled(tag, theme.pending_tag));
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(message.clone(), theme.message));
            if row.expandable && !row.expanded {
                spans.push(Span::styled(format!(" ({} files)", file_count), dim));
            }
        }
        RowKind::CommitFile {
            path,
            index,
            worktree,
            on_branch,
            ..
        } => {
            let prefix = if *on_branch { "│┊    " } else { "┊     " };
            spans.push(Span::styled(prefix, theme.graph));
            spans.push(Span::styled(row.sid.clone(), theme.shortid));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(index.to_string(), theme.staged_status));
            spans.push(Span::styled(worktree.to_string(), theme.unstaged_status));
            spans.push(Span::raw(" "));
            spans.push(Span::raw(display(path)));
        }
        RowKind::Upstream {
            label,
            base_short_id,
            base_message,
            commits_ahead,
        } => {
            spans.push(Span::styled("●  ", theme.graph));
            spans.push(Span::styled(format!("{} ", base_short_id), dim));
            spans.push(Span::styled("(upstream) ", theme.label));
            spans.push(Span::styled("[", dim));
            spans.push(Span::styled(label.clone(), theme.branch));
            spans.push(Span::styled("] ", dim));
            if *commits_ahead > 0 {
                spans.push(Span::styled(
                    format!(
                        "⏫ {} new commit{} — ",
                        commits_ahead,
                        if *commits_ahead == 1 { "" } else { "s" }
                    ),
                    theme.message,
                ));
            }
            spans.push(Span::styled(base_message.clone(), theme.message));
        }
        RowKind::Context {
            short_hash,
            date,
            message,
        } => {
            spans.push(Span::styled("· ", dim));
            spans.push(Span::styled(format!("{} {} ", short_hash, date), dim));
            spans.push(Span::styled(message.clone(), theme.message));
        }
        RowKind::Spacer(text) => {
            spans.push(Span::styled(*text, theme.graph));
        }
    }

    Line::from(spans)
}

// ── Diff pane content ────────────────────────────────────────────────────

/// Produce the raw diff text for a row by shelling out to git.
fn diff_text(snapshot: &Snapshot, row: &Row) -> String {
    let workdir = &snapshot.workdir;
    // The branch being named owns commits only on paper until it is created.
    if row.key == branch_key(NEW_BRANCH_NAME) {
        return "branch not created yet".to_string();
    }
    let result = match &row.kind {
        RowKind::LocalChanges { count } => {
            if *count == 0 {
                return "no changes".to_string();
            }
            git::diff_head_display(workdir)
        }
        RowKind::WorkingFile {
            path,
            index,
            worktree,
        } => {
            if graph::file_group(*index, *worktree) == graph::FileGroup::Untracked {
                return untracked_file_text(workdir, path);
            }
            git::diff_head_file_display(workdir, path)
        }
        RowKind::BranchName { range, .. } => match range {
            Some((base, tip)) => git::diff_range(workdir, base, tip),
            None => return "branch has no commits of its own".to_string(),
        },
        RowKind::Commit { oid, .. } if *oid == PENDING_COMMIT_OID => {
            return "commit not created yet".to_string();
        }
        RowKind::Commit { oid, .. } => git::show_commit_patch(workdir, &oid.to_string()),
        RowKind::CommitFile { oid, path, .. } => {
            git::show_commit_file(workdir, &oid.to_string(), path)
        }
        RowKind::Upstream {
            label,
            base_short_id,
            base_message,
            commits_ahead,
        } => {
            return format!(
                "upstream: {}\ncommon base: {} {}\n{} new commit(s) on the remote",
                label, base_short_id, base_message, commits_ahead
            );
        }
        RowKind::Context { short_hash, .. } => git::show_commit_patch(workdir, short_hash),
        RowKind::Spacer(_) => return String::new(),
    };
    match result {
        Ok(text) if text.trim().is_empty() => "no changes".to_string(),
        Ok(text) => text,
        Err(e) => format!("error: {}", e),
    }
}

/// What a `C` commit will contain: the index the selector just staged.
fn staged_commit_diff(snapshot: &Snapshot) -> String {
    match git::diff_cached_display(&snapshot.workdir) {
        Ok(text) if text.trim().is_empty() => "no changes".to_string(),
        Ok(text) => text,
        Err(e) => format!("error: {}", e),
    }
}

/// What the commit being placed will contain: the working-tree changes of
/// `files` (short IDs, or `zz` for all of them), as their rows show them.
///
/// `zz` commits through `git add -A`, so the untracked files go in too and
/// the preview must list them — `git diff HEAD` alone would hide exactly the
/// files the row counts.
fn pending_commit_diff(snapshot: &Snapshot, files: &[String]) -> String {
    let workdir = &snapshot.workdir;
    let ids = &snapshot.ids;
    let all = snapshot.is_all_changes(files);
    let or_empty = |result: Result<String>| match result {
        Ok(text) if text.trim().is_empty() => String::new(),
        Ok(text) => text,
        Err(e) => format!("error: {}", e),
    };
    // A yes/no question about one file, not the three-way grouping the tree
    // and the JSON share, so this stays local rather than going through
    // `graph::file_group`.
    let untracked = |c: &repo::FileChange| c.index == '?' && c.worktree == '?';
    let wanted = |c: &repo::FileChange| all || files.iter().any(|f| f == ids.get_file(&c.path));
    let mut out = String::new();

    // One `git` for every tracked file at once: `c` runs this on the event
    // path, and a spawn per selected file freezes the UI for the selection.
    if all {
        out.push_str(&or_empty(git::diff_head_display(workdir)));
    } else {
        let paths: Vec<&str> = snapshot
            .info
            .working_changes
            .iter()
            .filter(|c| wanted(c) && !untracked(c))
            .map(|c| c.path.as_str())
            .collect();
        out.push_str(&or_empty(git::diff_head_files_display(workdir, &paths)));
    }

    // Untracked files have no diff to ask for; `git add -A` commits them, so
    // the preview reads them itself.
    for change in snapshot
        .info
        .working_changes
        .iter()
        .filter(|c| wanted(c) && untracked(c))
    {
        // Before, not after: the tracked diff above may end mid-line (an
        // error message does), and its text must not run into this header.
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&untracked_file_text(workdir, &change.path));
    }
    if out.trim().is_empty() {
        return "no changes".to_string();
    }
    out
}

/// Render an untracked file's content as added lines.
///
/// Only a bounded prefix is read so a large artifact never spikes memory;
/// the binary (NUL byte) check runs on that prefix, like git's own heuristic.
fn untracked_file_text(workdir: &std::path::Path, path: &str) -> String {
    const MAX_LINES: usize = 2000;
    const MAX_BYTES: u64 = 256 * 1024;
    use std::io::Read;

    let full = workdir.join(path);
    let Ok(file) = std::fs::File::open(&full) else {
        return "(unreadable file)".to_string();
    };
    let mut bytes = Vec::new();
    if file.take(MAX_BYTES + 1).read_to_end(&mut bytes).is_err() {
        return "(unreadable file)".to_string();
    }
    let mut truncated = bytes.len() as u64 > MAX_BYTES;
    bytes.truncate(MAX_BYTES as usize);
    if bytes.contains(&0) {
        return "(binary file)".to_string();
    }
    let content = String::from_utf8_lossy(&bytes);
    let mut out = format!("untracked file: {}\n", path);
    for (i, line) in content.lines().enumerate() {
        if i >= MAX_LINES {
            truncated = true;
            break;
        }
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    if truncated {
        out.push_str("...\n");
    }
    out
}

#[cfg(test)]
#[path = "app_test.rs"]
mod tests;
