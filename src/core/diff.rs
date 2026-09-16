// Placeholder texts for a file that has no `@@` hunks: the whole file is one
// entry in the picker, taken or left entire.
pub(crate) const DELETED_ENTRY: &str = "(file deleted)";
pub(crate) const BINARY_ENTRY: &str = "(binary file)";
pub(crate) const SUBMODULE_ENTRY: &str = "(submodule)";
pub(crate) const EMPTY_ENTRY: &str = "(empty file)";

/// What every real hunk starts with, and no whole-file placeholder does.
const HUNK_HEADER: &str = "@@ -";

/// What a hunk with an empty pre-image starts with, so it is a whole new file
/// rather than a change within one. Both git's forms continue it: `+1,N @@`
/// and, for a one-line file, `+1 @@`. `collect_unstaged_hunks` builds its
/// synthesized header from this so the two cannot drift apart.
pub(crate) const NEW_FILE_HEADER: &str = "@@ -0,0 ";

/// A single hunk extracted from a unified diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffHunk {
    /// The raw diff text for this hunk (starting with the @@ header, ending before the next hunk
    /// or EOF).
    pub text: String,
    /// Original (pre-image) line numbers of modified/deleted lines in this hunk.
    pub modified_lines: Vec<usize>,
}

impl DiffHunk {
    /// Whether this is a real `@@` hunk rather than a whole-file placeholder
    /// standing in for a deletion, a binary blob, a submodule or an empty file.
    /// Those are text too, so the `@@` header is what separates them.
    pub(crate) fn is_text(&self) -> bool {
        self.text.starts_with(HUNK_HEADER)
    }

    /// Whether this hunk is a whole new file rather than a change within one.
    pub(crate) fn is_whole_new_file(&self) -> bool {
        self.text.starts_with(NEW_FILE_HEADER)
    }

    /// Post-image line numbers of the lines this hunk adds; empty for a
    /// placeholder.
    pub(crate) fn added_lines(&self) -> Vec<usize> {
        let mut lines = self.text.lines();
        let Some(mut line_no) = lines.next().and_then(parse_hunk_new_start) else {
            return vec![];
        };
        let mut added = Vec::new();
        for line in lines {
            match line.as_bytes().first() {
                Some(b'+') => {
                    added.push(line_no);
                    line_no += 1;
                }
                Some(b'-' | b'\\') => {}
                _ => line_no += 1,
            }
        }
        added
    }
}

/// Parse a unified diff into individual hunks.
///
/// Each hunk starts at an `@@ -start,count +start,count @@` header and extends
/// until the next hunk header or end of input. The file headers (`--- a/` / `+++ b/`)
/// are excluded from hunk text.
pub(crate) fn parse_hunks(diff: &str) -> Vec<DiffHunk> {
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut current_text = String::new();
    let mut current_modified: Vec<usize> = Vec::new();
    let mut current_orig_line: usize = 0;
    let mut in_hunk = false;

    for line in diff.lines() {
        if line.starts_with(HUNK_HEADER) {
            // Save previous hunk if any
            if in_hunk {
                hunks.push(DiffHunk {
                    text: std::mem::take(&mut current_text),
                    modified_lines: std::mem::take(&mut current_modified),
                });
            }
            // Start new hunk
            current_text = format!("{}\n", line);
            current_modified = Vec::new();
            current_orig_line = parse_hunk_start(line).unwrap_or(1);
            in_hunk = true;
        } else if !in_hunk {
            // File header lines (--- a/, +++ b/, diff --git, etc.) — skip
            continue;
        } else if line.starts_with('-') {
            current_text.push_str(line);
            current_text.push('\n');
            current_modified.push(current_orig_line);
            current_orig_line += 1;
        } else if line.starts_with('+') {
            current_text.push_str(line);
            current_text.push('\n');
            // Added line — doesn't consume an original line number
        } else if line.starts_with('\\') {
            current_text.push_str(line);
            current_text.push('\n');
            // "\ No newline at end of file" — no line number change
        } else {
            // Context line
            current_text.push_str(line);
            current_text.push('\n');
            current_orig_line += 1;
        }
    }

    // Save last hunk
    if in_hunk {
        hunks.push(DiffHunk {
            text: current_text,
            modified_lines: current_modified,
        });
    }

    hunks
}

/// Parse a hunk header to extract the starting line number of the original side.
pub(crate) fn parse_hunk_start(line: &str) -> Option<usize> {
    let line = line.strip_prefix(HUNK_HEADER)?;
    let end = line.find([',', ' '])?;
    line[..end].parse().ok()
}

/// The starting line number of the new side of a hunk header.
fn parse_hunk_new_start(line: &str) -> Option<usize> {
    let line = line.strip_prefix(HUNK_HEADER)?;
    let new = &line[line.find(" +")? + 2..];
    let end = new.find([',', ' '])?;
    new[..end].parse().ok()
}

/// Build a valid unified patch for `git apply` from selected hunks of a single
/// file, dropping any entry that is not a hunk.
///
/// Produces a patch with one file header (`--- a/` / `+++ b/`) followed by
/// the raw text of each hunk (which includes the `@@` header).
///
/// Accepts both `&[DiffHunk]` and `&[&DiffHunk]` via `Borrow`.
pub(crate) fn build_hunk_patch(path: &str, hunks: &[impl std::borrow::Borrow<DiffHunk>]) -> String {
    // Dropped, not asserted: a panic here would escape the caller's rollback.
    // Headers come after, so dropping every hunk yields nothing rather than a
    // bodiless fragment that a `!patch.is_empty()` caller would still apply.
    let mut hunks = hunks.iter().map(|h| h.borrow()).filter(|h| h.is_text());
    let Some(first) = hunks.next() else {
        return String::new();
    };
    let mut patch = format!("--- a/{path}\n+++ b/{path}\n");
    patch.push_str(&first.text);
    for hunk in hunks {
        patch.push_str(&hunk.text);
    }
    patch
}

#[cfg(test)]
#[path = "diff_test.rs"]
mod tests;
