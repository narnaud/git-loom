//! The text a session shows beside its tree: the diff of a row, read from
//! git, and what each of its lines is.

use anyhow::Result;

use crate::core::graph;
use crate::core::repo::{self, FileChange};
use crate::git;
use crate::session::snapshot::{NEW_BRANCH_NAME, Snapshot};
use crate::session::tree::{PENDING_COMMIT_OID, Row, RowKind, branch_key};

/// What a line of `git diff`/`git show` output is, for coloring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffLine {
    /// `diff --git`, `+++`, `---`.
    FileHeader,
    /// `index …`.
    Index,
    Added,
    Removed,
    /// `@@ … @@`.
    HunkHeader,
    Context,
    /// The `commit <hash>` line opening `git show` output.
    CommitLine,
    /// The rest of that opening: author, date, message, diffstat.
    CommitHeader,
}

/// Classify one line of full `git diff`/`git show` output.
pub(crate) fn diff_line(line: &str) -> DiffLine {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff --git") {
        DiffLine::FileHeader
    } else if line.starts_with("index ") {
        DiffLine::Index
    } else {
        hunk_line(line)
    }
}

/// Classify one line inside a hunk body. Distinct from [`diff_line`]: hunk
/// bodies carry no file headers, so a body line whose content begins with
/// `++` (from an added line reading `++ x`) must stay an added line.
pub(crate) fn hunk_line(line: &str) -> DiffLine {
    if line.starts_with('+') {
        DiffLine::Added
    } else if line.starts_with('-') {
        DiffLine::Removed
    } else if line.starts_with("@@") {
        DiffLine::HunkHeader
    } else {
        DiffLine::Context
    }
}

/// Each line of diff text with its kind.
///
/// `git show` output opens with a header (commit line, author, date, the
/// commit message, diffstat) whose lines would otherwise read as context —
/// the message is indented like context lines. Everything from the start of
/// a `commit `-led text up to the first `diff --git` is header instead.
pub(crate) fn classify(text: &str) -> impl Iterator<Item = (DiffLine, &str)> {
    let mut in_header = text.starts_with("commit ");
    text.lines().map(move |raw| {
        if in_header && raw.starts_with("diff --git") {
            in_header = false;
        }
        let kind = if !in_header {
            diff_line(raw)
        } else if raw.starts_with("commit ") {
            DiffLine::CommitLine
        } else {
            DiffLine::CommitHeader
        };
        (kind, raw)
    })
}

/// `line` with each tab shown as `width` spaces: the terminal grid has no
/// tab stops, and a diff's one-column prefix would shift them anyway.
pub(crate) fn expand_tabs(line: &str, width: usize) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        match c {
            '\t' => out.extend(std::iter::repeat_n(' ', width)),
            c => out.push(c),
        }
    }
    out
}

/// Produce the raw diff text for a row by shelling out to git.
pub(crate) fn diff_text(snapshot: &Snapshot, row: &Row) -> String {
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
            return working_file_diff(
                workdir,
                &FileChange {
                    path: path.clone(),
                    index: *index,
                    worktree: *worktree,
                },
            );
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

/// A working file against HEAD: a conflicted one with its markers, an
/// untracked one as added lines.
pub(crate) fn working_file_diff(workdir: &std::path::Path, file: &FileChange) -> String {
    if graph::file_group(file.index, file.worktree) == graph::FileGroup::Untracked {
        return untracked_file_text(workdir, &file.path);
    }
    match git::diff_head_file_display(workdir, &file.path) {
        Ok(text) if text.trim().is_empty() => "no changes".to_string(),
        Ok(text) => text,
        Err(e) => format!("error: {}", e),
    }
}

/// What a `C` commit will contain: the index the selector just staged.
pub(crate) fn staged_commit_diff(snapshot: &Snapshot) -> String {
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
pub(crate) fn pending_commit_diff(snapshot: &Snapshot, files: &[String]) -> String {
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
pub(crate) fn untracked_file_text(workdir: &std::path::Path, path: &str) -> String {
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
