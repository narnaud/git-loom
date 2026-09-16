//! Non-interactive hunk selection for agent mode (see spec 019).
//!
//! The hunk picker is a full-screen TUI, so agent mode answers it as data: a
//! `needs_input` listing every hunk with an id, plus a fingerprint of the diff
//! those ids were numbered against. The agent re-runs the same command with
//! `--hunks <id>` once per id, plus `--hunks-from <fingerprint>`. Ids are
//! positional, so the fingerprint is the only thing standing between a stale
//! selection and silently moving the wrong lines.

use anyhow::{Result, bail};
use git2::{ObjectType, Oid};

use crate::core::agent_mode::{self, HunkItem};
use crate::core::diff::{BINARY_ENTRY, DELETED_ENTRY, DiffHunk, SUBMODULE_ENTRY};
use crate::tui::hunk_selector::{FileEntry, HunkOrigin};

/// The `--hunks` / `--hunks-from` pair. The CLI requires each flag with the
/// other, so an empty `ids` means neither was given.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HunkArgs {
    pub ids: Vec<String>,
    pub from: Option<String>,
}

impl HunkArgs {
    pub fn new(ids: Vec<String>, from: Option<String>) -> Self {
        Self { ids, from }
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// `<command> -p` plus the file filter, shell-quoted: the invocation a hunk
/// listing tells an agent to repeat (spec 019).
pub fn patch_command(command: &str, files: &[String]) -> String {
    let mut out = format!("{command} -p");
    for file in files {
        out.push(' ');
        out.push_str(&quoted(file));
    }
    out
}

/// ` -- <git args>`, shell-quoted, or nothing: the tail a replay hint repeats
/// after every loom argument (Spec 021).
pub fn git_args_suffix(git_args: &[&str]) -> String {
    if git_args.is_empty() {
        return String::new();
    }
    let mut out = " --".to_string();
    for arg in git_args {
        out.push(' ');
        out.push_str(&quoted(arg));
    }
    out
}

/// The working-tree picker for `command`, whose entries are all pickable:
/// `apply_selections` stages a binary or deleted file by path.
///
/// `target_hash` is the commit the selection lands in, for the callers that
/// have one (`fold`); see `Picker::target_hash`.
pub fn worktree_picker(
    hunks: HunkArgs,
    command: String,
    target_hash: Option<&str>,
    git_args: &[&str],
) -> Picker {
    Picker {
        hunks,
        command,
        whole_files: true,
        target_hash: target_hash.map(str::to_string),
        git_args: git_args_suffix(git_args),
    }
}

/// How a `-p` picker is answered (spec 019).
///
/// No `Default`: `whole_files` is a per-command policy, and defaulting it
/// would silently hand a new caller `fold`'s.
#[derive(Debug, Clone)]
pub struct Picker {
    pub hunks: HunkArgs,
    /// The invocation the listing tells an agent to repeat. It MUST carry every
    /// argument that shapes the listing — one left out makes the replay fail
    /// the fingerprint check instead of working.
    pub command: String,
    /// Whether an entry with no text hunks can be picked. `split` and a
    /// working-tree source stage binary and deleted files whole (Spec 013);
    /// a commit-source `fold` cannot move a binary one (Spec 007).
    pub whole_files: bool,
    /// The resolved hash of the commit the selection lands in, when it is not
    /// the one listed — a revspec here would defeat the check. Part of the
    /// fingerprint: a replay re-resolves it from what the agent typed, and a
    /// relative revspec (`HEAD~2`) can name a different commit by then, so the
    /// ids would be amended into whatever now sits there.
    pub target_hash: Option<String>,
    /// The `git_args_suffix` the replay ends with, after the selection flags:
    /// following `--`, they would reach git instead.
    pub git_args: String,
}

/// An argument as it must come back on the replay command line (Spec 019).
///
/// POSIX quoting everywhere, like the sequence editor `weave` builds: an agent
/// replays this through a bash-like shell on Windows too. `shell_escape`'s
/// windows form targets cmd.exe and quotes only around `"`, tab, newline and
/// space, so a `;` or `&` in a path would reach the shell bare.
pub fn quoted(arg: &str) -> String {
    shell_escape::unix::escape(arg.into()).into_owned()
}

/// `<path>:<n>`, `n` counting from 1 within the file, in listing order.
fn hunk_id(path: &str, index: usize) -> String {
    format!("{path}:{}", index + 1)
}

/// Whether this command can take the entry.
///
/// Every real hunk qualifies. Of the whole-file placeholders a submodule and a
/// deletion do, because `fold` moves either as the commit's own whole-file
/// diff (Spec 007); a binary blob or an empty file have nothing to move.
/// `split` takes them all, staging the file whole (Spec 013).
fn is_selectable(hunk: &DiffHunk, whole_files: bool) -> bool {
    whole_files || hunk.is_text() || hunk.text == SUBMODULE_ENTRY || hunk.text == DELETED_ENTRY
}

/// Whether anything in the listing can be picked at all.
pub fn has_selectable(entries: &[FileEntry], whole_files: bool) -> bool {
    entries
        .iter()
        .flat_map(|file| &file.hunks)
        .any(|entry| is_selectable(&entry.hunk, whole_files))
}

/// The ids of the entries picked in `entries`, as `--hunks` takes them, and
/// the paths of the picked entries this command cannot take, which a caller
/// must leave out: [`apply`] refuses their ids.
pub fn picked_ids(entries: &[FileEntry], whole_files: bool) -> (Vec<String>, Vec<String>) {
    let mut ids = Vec::new();
    let mut refused = Vec::new();
    for file in entries {
        for (index, entry) in file.hunks.iter().enumerate() {
            if !entry.selected {
                continue;
            }
            if is_selectable(&entry.hunk, whole_files) {
                ids.push(hunk_id(&file.path, index));
            } else if !refused.contains(&file.path) {
                refused.push(file.path.clone());
            }
        }
    }
    (ids, refused)
}

/// Digest of the numbering a set of ids was taken from, not of the command:
/// `split -p <c>` and `fold -p <c> zz` number a commit identically, and `apply`
/// re-checks what each of them can take.
///
/// Covers both commits it touches and every entry, unselectable ones included,
/// so that moving either end or inserting or removing any entry invalidates
/// the numbering it would have shifted.
pub fn fingerprint(oid: &str, target: Option<&str>, entries: &[FileEntry]) -> String {
    let mut payload = String::from(oid);
    payload.push('\n');
    payload.push_str(target.unwrap_or_default());
    payload.push('\n');
    for file in entries {
        payload.push_str(&file.path);
        payload.push('\0');
        for entry in &file.hunks {
            // What `--hunks` does to an entry depends on whether it is staged.
            payload.push(match entry.origin {
                HunkOrigin::Staged => 'S',
                HunkOrigin::Unstaged => 'U',
                HunkOrigin::Commit => 'C',
            });
            payload.push_str(&entry.hunk.text);
            payload.push('\0');
        }
    }
    let digest =
        Oid::hash_object(ObjectType::Blob, payload.as_bytes()).expect("hashing a blob cannot fail");
    digest.to_string()[..12].to_string()
}

/// How the listing names a new file (Spec 019). Counts the `+` lines: the `@@`
/// header and a `\ No newline` marker are not part of the file.
fn new_file_summary(hunk: &DiffHunk) -> String {
    let lines = hunk
        .text
        .lines()
        .filter(|line| line.starts_with('+'))
        .count();
    format!("(new file, {lines} line(s))")
}

/// One JSON item per hunk, in the order the ids number them.
///
/// Consumes `entries` so each hunk's text is moved into the response.
pub fn items(entries: Vec<FileEntry>, whole_files: bool) -> Vec<HunkItem> {
    let mut items = Vec::new();
    for file in entries {
        // The summary sends the agent to the file, so it may stand in only
        // where the file *is* this entry. Untracked is the one case that
        // guarantees it: `collect_unstaged_hunks` built the text from the
        // bytes it read off disk, and it is that file's only hunk. Anywhere
        // else the text is git's diff of the *indexed* content, which a clean
        // or eol filter makes something else entirely — a one-line LFS pointer
        // for a huge file (Spec 019).
        let untracked = file.is_untracked();
        for (index, entry) in file.hunks.into_iter().enumerate() {
            let selectable = is_selectable(&entry.hunk, whole_files);
            let whole_new_file = untracked && entry.hunk.is_whole_new_file();
            let diff = if whole_new_file {
                new_file_summary(&entry.hunk)
            } else {
                entry.hunk.text
            };
            items.push(HunkItem {
                id: hunk_id(&file.path, index),
                path: file.path.clone(),
                diff,
                selectable,
                staged: entry.selected,
            });
        }
    }
    items
}

/// Refuse a selection that unstages content existing only in the index, in
/// the picker and on a `--hunks` replay alike (Spec 019).
pub(crate) fn refuse_losing_index_content(files: &[FileEntry]) -> Result<()> {
    for file in files {
        for (index, entry) in file.hunks.iter().enumerate() {
            if entry.origin == HunkOrigin::Staged
                && !entry.selected
                && only_in_index(file, &entry.hunk)
            {
                let id = hunk_id(&file.path, index);
                bail!(
                    "Unstaging `{id}` would lose what only the index holds: \
                     the working tree changed `{}` there again\n\
                     Keep `{id}` staged",
                    file.path
                );
            }
        }
    }
    Ok(())
}

/// Whether unstaging `staged` would destroy content: unstaging reverse-applies
/// to the index alone, so a line it added that the working tree changed again
/// exists nowhere else afterwards.
///
/// A staged hunk's added lines and an unstaged hunk's removed ones are both
/// index line numbers, so they compare directly.
fn only_in_index(file: &FileEntry, staged: &DiffHunk) -> bool {
    let mut unstaged = file
        .hunks
        .iter()
        .filter(|h| h.origin == HunkOrigin::Unstaged)
        .map(|h| &h.hunk);
    if staged.text == BINARY_ENTRY {
        return unstaged.next().is_some();
    }
    let added = staged.added_lines();
    if added.is_empty() {
        return false;
    }
    // A placeholder — the file deleted or turned binary — takes every line.
    // `added` is ascending, as the hunk walks its lines in order.
    unstaged.any(|u| {
        !u.is_text()
            || u.modified_lines
                .iter()
                .any(|l| added.binary_search(l).is_ok())
    })
}

/// Answer the picker with the hunk listing instead of rendering it.
pub fn respond(oid: &str, entries: Vec<FileEntry>, picker: &Picker) -> anyhow::Error {
    let fingerprint = fingerprint(oid, picker.target_hash.as_deref(), &entries);
    let hint = format!(
        "re-run with: {} --hunks <id> [--hunks <id>...] --hunks-from {fingerprint}{}",
        picker.command, picker.git_args
    );
    agent_mode::respond_needs_hunks(items(entries, picker.whole_files), fingerprint, &hint)
}

/// Replace the selection with the requested hunks.
///
/// The ids are the whole answer, exactly as confirming the picker with those
/// entries ticked would be: anything already selected — a staged working-tree
/// hunk — and left out of them ends up deselected.
///
/// Refuses a selection numbered against a different diff rather than guess
/// which hunks the ids now point at.
pub fn apply(oid: &str, entries: &mut [FileEntry], picker: &Picker) -> Result<()> {
    let current = fingerprint(oid, picker.target_hash.as_deref(), entries);
    let args = &picker.hunks;
    match args.from.as_deref() {
        Some(from) if from == current => {}
        Some(from) => bail!(
            "The hunks changed since the listing fingerprinted {from} (now {current})\n\
             Re-run with -p alone to list them again"
        ),
        None => bail!("--hunks requires --hunks-from <fingerprint>"),
    }

    // Resolve every id before touching the selection: a refused one leaves the
    // entries as they were, so nothing is deselected on the way to an error.
    let mut picked = Vec::with_capacity(args.ids.len());
    for id in &args.ids {
        // Strictly `<path>:<n>` counting from 1. `:0` and a signed `:+1` are
        // ids loom never emits, so honoring them would accept a selection it
        // never numbered.
        let parsed = id.rsplit_once(':').and_then(|(path, n)| {
            let digits = !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
            digits
                .then(|| n.parse::<usize>().ok())
                .flatten()
                .filter(|n| *n > 0)
                .map(|n| (path, n - 1))
        });
        let Some((path, index)) = parsed else {
            bail!("Invalid hunk id `{id}`\nIds look like `src/main.rs:1`");
        };
        let found = entries
            .iter()
            .position(|file| file.path == path)
            .filter(|&file_index| index < entries[file_index].hunks.len());
        let Some(file_index) = found else {
            // A comma list is the natural guess, and its whole value arrives
            // here as one unknown id. Say so instead of making the agent
            // re-list to find out.
            let separator = if id.contains(',') {
                "\nPass one `--hunks` per id — they are not comma-separated"
            } else {
                ""
            };
            bail!("No hunk `{id}` in this diff{separator}");
        };
        if !is_selectable(&entries[file_index].hunks[index].hunk, picker.whole_files) {
            bail!("`fold -p` cannot move `{id}`: a binary file has no hunk");
        }
        picked.push((file_index, index));
    }

    for file in entries.iter_mut() {
        for entry in &mut file.hunks {
            entry.selected = false;
        }
    }
    for (file_index, index) in picked {
        entries[file_index].hunks[index].selected = true;
    }

    Ok(())
}

#[cfg(test)]
#[path = "hunk_select_test.rs"]
mod tests;
