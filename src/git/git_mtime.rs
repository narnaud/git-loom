//! Modification times across a rebase (Spec 023).
//!
//! A rebase checks out the upstream, replays every commit and replays its
//! autostash, so every file in the replayed range and every dirty file gets a
//! fresh mtime even when its bytes end up as they started. The record taken
//! before the rebase puts those mtimes back once it is over, on the files whose
//! bytes are unchanged — only timestamps move, never content, and nothing here
//! fails the rebase it serves.
//!
//! The record lives in the thread that took it and dies with the invocation.
//! It is dropped the moment a pause is reported to the user: whatever they
//! build before going on reads intermediate bytes, and an older timestamp put
//! back afterwards would pass that build off as current. A stop the invocation
//! carries past itself (`--skip`, a rerere resolution, its own abort) keeps it.

use std::cell::RefCell;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;

use crate::trace as loom_trace;

struct Record {
    workdir: PathBuf,
    files: Vec<FileRecord>,
}

struct FileRecord {
    path: String,
    mtime: SystemTime,
    /// Blob id of the raw bytes, filters not applied.
    blob: git2::Oid,
}

thread_local! {
    // One rebase at a time per invocation; the TUI runs each command on its
    // own thread, so a record never crosses from one command to the next.
    static RECORD: RefCell<Option<Record>> = const { RefCell::new(None) };
}

/// Record the mtimes the rebase of `upstream..HEAD` (`None`: the whole
/// history) is about to churn. Best-effort: a failure is traced and the rebase
/// runs without a record.
pub fn snapshot_mtimes(workdir: &Path, upstream: Option<&str>) {
    discard_mtimes();
    let note = match take_record(workdir, upstream) {
        Ok(record) => {
            let count = record.files.len();
            RECORD.with(|r| *r.borrow_mut() = Some(record));
            format!("recorded {count} files")
        }
        Err(e) => format!("not recorded: {e:#}"),
    };
    loom_trace::annotate("mtimes", &note);
}

/// Put the recorded mtimes back on the files whose bytes are unchanged, and
/// drop the record. A no-op without one; never fails the caller.
pub fn restore_mtimes() {
    let Some(record) = RECORD.with(|r| r.borrow_mut().take()) else {
        return;
    };
    loom_trace::annotate("mtimes", &restore_record(&record));
}

/// Drop the record without restoring: the tree is being handed to the user.
pub fn discard_mtimes() {
    RECORD.with(|r| r.borrow_mut().take());
}

fn take_record(workdir: &Path, upstream: Option<&str>) -> Result<Record> {
    let range = match upstream {
        Some(upstream) => format!("{upstream}..HEAD"),
        None => "HEAD".to_string(),
    };
    // Per commit rather than `diff <upstream> HEAD`: a file changed and then
    // changed back is rewritten twice by the replay yet absent from that diff.
    // A merge lists what it brought in over its first parent.
    let replayed = super::run_git_stdout(
        workdir,
        &[
            "log",
            "-z",
            "--format=",
            "--name-only",
            "--no-renames",
            "--diff-merges=first-parent",
            &range,
        ],
    )?;
    // What the autostash takes and puts back; untracked files stay put. Both
    // diffs: a file staged with other bytes and put back by hand is absent
    // from the first, and the autostash's reset still rewrites it.
    let dirty = super::run_git_stdout(workdir, &["diff", "--name-only", "-z", "HEAD"])?;
    let staged = super::run_git_stdout(workdir, &["diff", "--cached", "--name-only", "-z"])?;
    let mut paths: Vec<String> = replayed
        .split('\0')
        .chain(dirty.split('\0'))
        .chain(staged.split('\0'))
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    paths.sort_unstable();
    paths.dedup();

    let files = paths
        .into_iter()
        .filter_map(|path| {
            let full = workdir.join(&path);
            let mtime = regular_file_mtime(&full)?;
            let blob = hash_raw(&full)?;
            Some(FileRecord { path, mtime, blob })
        })
        .collect();
    Ok(Record {
        workdir: workdir.to_path_buf(),
        files,
    })
}

fn restore_record(record: &Record) -> String {
    let mut restored = 0;
    let mut skipped = Vec::new();
    for file in &record.files {
        let full = record.workdir.join(&file.path);
        // Stat before hashing: a file the rebase never rewrote needs nothing.
        let Some(current) = regular_file_mtime(&full) else {
            continue;
        };
        if current == file.mtime || hash_raw(&full) != Some(file.blob) {
            continue;
        }
        match File::options()
            .write(true)
            .open(&full)
            .and_then(|f| f.set_modified(file.mtime))
        {
            Ok(()) => restored += 1,
            Err(e) => skipped.push(format!("{} ({e})", file.path)),
        }
    }
    if restored > 0 {
        // The index holds the stat data the rebase wrote; one refresh now
        // saves every later command re-hashing these files.
        let _ = super::run_git(
            &record.workdir,
            &["update-index", "-q", "--unmerged", "--refresh"],
        );
    }
    let mut note = format!("restored {restored} of {} recorded", record.files.len());
    if !skipped.is_empty() {
        note.push_str(&format!("; could not set: {}", skipped.join(", ")));
    }
    note
}

/// The mtime of `path` when it is a regular file; a symlink, a directory
/// (submodule) or a missing path is `None`.
fn regular_file_mtime(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    meta.modified().ok()
}

/// Blob id of the raw bytes of `path`, streamed through libgit2 (follows a
/// symlink, so callers check the file type first).
fn hash_raw(path: &Path) -> Option<git2::Oid> {
    git2::Oid::hash_file(git2::ObjectType::Blob, path).ok()
}

#[cfg(test)]
#[path = "git_mtime_test.rs"]
mod tests;
