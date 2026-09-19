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
use crate::core::diff::{DELETED_ENTRY, DiffHunk, SUBMODULE_ENTRY};
use crate::tui::hunk_selector::FileEntry;

/// The `--hunks` / `--hunks-from` pair. The CLI requires each flag with the
/// other, so an empty `ids` means neither was given.
#[derive(Debug, Default, Clone)]
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

/// How a commit-source `-p` picker is answered (spec 019).
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
    /// Whether an entry with no text hunks can be picked. `split` stages binary
    /// and deleted files whole (Spec 013); `fold` cannot move a binary one
    /// (Spec 007).
    pub whole_files: bool,
    /// The resolved hash of the commit the selection lands in, when it is not
    /// the one listed — a revspec here would defeat the check. Part of the
    /// fingerprint: a replay re-resolves it from what the agent typed, and a
    /// relative revspec (`HEAD~2`) can name a different commit by then, so the
    /// ids would be amended into whatever now sits there.
    pub target_hash: Option<String>,
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
            payload.push_str(&entry.hunk.text);
            payload.push('\0');
        }
    }
    let digest =
        Oid::hash_object(ObjectType::Blob, payload.as_bytes()).expect("hashing a blob cannot fail");
    digest.to_string()[..12].to_string()
}

/// One JSON item per hunk, in the order the ids number them.
///
/// Consumes `entries` so each hunk's text is moved into the response.
pub fn items(entries: Vec<FileEntry>, whole_files: bool) -> Vec<HunkItem> {
    let mut items = Vec::new();
    for file in entries {
        for (index, entry) in file.hunks.into_iter().enumerate() {
            let selectable = is_selectable(&entry.hunk, whole_files);
            items.push(HunkItem {
                id: hunk_id(&file.path, index),
                path: file.path.clone(),
                diff: entry.hunk.text,
                selectable,
            });
        }
    }
    items
}

/// Answer the picker with the hunk listing instead of rendering it.
pub fn respond(oid: &str, entries: Vec<FileEntry>, picker: &Picker) -> anyhow::Error {
    let fingerprint = fingerprint(oid, picker.target_hash.as_deref(), &entries);
    let hint = format!(
        "re-run with: {} --hunks <id> [--hunks <id>...] --hunks-from {fingerprint}",
        picker.command
    );
    agent_mode::respond_needs_hunks(items(entries, picker.whole_files), fingerprint, &hint)
}

/// Mark the requested hunks selected.
///
/// Refuses a selection numbered against a different diff rather than guess
/// which hunks the ids now point at. Resolves a path to its first entry, so it
/// is for commit diffs only — working-tree entries repeat a path once staged
/// and once unstaged.
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
        let entry = entries
            .iter_mut()
            .find(|file| file.path == path)
            .and_then(|file| file.hunks.get_mut(index));
        let Some(entry) = entry else {
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
        if !is_selectable(&entry.hunk, picker.whole_files) {
            bail!("`fold -p` cannot move `{id}`: a binary file has no hunk");
        }
        entry.selected = true;
    }

    Ok(())
}

#[cfg(test)]
#[path = "hunk_select_test.rs"]
mod tests;
